use crate::{config::TaskConfig, schedule};
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::store::audit;
pub async fn apply_task(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    c: &TaskConfig,
    expected: Option<i64>,
    user: Uuid,
    editor_layout: Option<&crate::editor_layout::EditorLayout>,
) -> Result<Option<Value>> {
    let mut tx = pool.begin().await?;
    let result = apply_task_tx(
        &mut tx,
        workspace,
        credential,
        c,
        expected,
        user,
        editor_layout,
    )
    .await?;
    tx.commit().await?;
    Ok(result)
}

pub(crate) async fn apply_task_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    credential: &str,
    c: &TaskConfig,
    expected: Option<i64>,
    user: Uuid,
    editor_layout: Option<&crate::editor_layout::EditorLayout>,
) -> Result<Option<Value>> {
    let mut normalized = c.clone();
    if let Some(schedule) = &mut normalized.schedule
        && schedule.timezone.is_empty()
    {
        let settings: Value = sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&mut **tx)
            .await?;
        schedule.timezone = settings["timezone"]
            .as_str()
            .unwrap_or("Asia/Shanghai")
            .into();
    }
    let c = &normalized;
    c.validate()?;
    if let Some(layout) = editor_layout {
        layout.validate(c)?;
    }
    crate::store::workspaces::lock(tx, workspace).await?;
    crate::auth::workspace::check_member(tx, credential, workspace, "operator").await?;
    // Serialize creation too; an absent row cannot be protected with FOR UPDATE.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:{}", c.id))
        .execute(&mut **tx)
        .await?;
    let old = sqlx::query(
        "SELECT revision,config,next_at,enabled,archived_at,editor_layout FROM tasks WHERE id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(&c.id)
    .bind(workspace).fetch_optional(&mut **tx)
    .await?;
    ensure!(
        old.as_ref()
            .is_none_or(|r| r.get::<Option<DateTime<Utc>>, _>("archived_at").is_none()),
        "task is archived; restore before editing"
    );
    let current = old.as_ref().map(|r| r.get::<i64, _>("revision"));
    if current != expected && !(current.is_none() && expected == Some(0)) {
        return Ok(None);
    }
    if let Some(old) = &old {
        ensure!(
            old.get::<Value, _>("config").get("dag").is_some() == c.dag.is_some(),
            "task type cannot be changed"
        );
    }
    let runner = if c.dag.is_some() {
        None
    } else {
        Some(crate::store::runners::runner(tx, workspace, c).await?)
    };
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut **tx)
        .await?;
    let config = serde_json::to_value(c)?;
    let unchanged = old.as_ref().is_some_and(|r| {
        r.get::<Value, _>("config")["schedule"] == config["schedule"]
            && r.get::<bool, _>("enabled") == c.enabled
    });
    let next_at = if unchanged {
        old.as_ref()
            .and_then(|r| r.get::<Option<DateTime<Utc>>, _>("next_at"))
    } else if c.enabled {
        match &c.schedule {
            Some(s) => {
                if let Some(at) = s.at {
                    Some(at)
                } else {
                    schedule::next(s, now, false)?
                }
            }
            None => None,
        }
    } else {
        None
    };
    let layout = if let Some(layout) = editor_layout {
        Some(layout.clone())
    } else {
        old.as_ref()
            .and_then(|row| row.get::<Option<Value>, _>("editor_layout"))
            .map(serde_json::from_value::<crate::editor_layout::EditorLayout>)
            .transpose()?
    }
    .and_then(|layout| layout.retain_nodes(c))
    .map(serde_json::to_value)
    .transpose()?;
    let row:Value=sqlx::query_scalar("INSERT INTO tasks(workspace_id,id,runner_id,config,enabled,next_at,created_by,updated_by,editor_layout) VALUES($7,$1,$2,$3,$4,$5,$6,$6,$8) ON CONFLICT(workspace_id,id) DO UPDATE SET runner_id=excluded.runner_id,config=excluded.config,enabled=excluded.enabled,next_at=excluded.next_at,editor_layout=excluded.editor_layout,revision=tasks.revision+1,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(tasks.*)")
        .bind(&c.id).bind(runner).bind(config).bind(c.enabled).bind(next_at).bind(user).bind(workspace).bind(layout).fetch_one(&mut **tx).await?;
    crate::store::dag::save_nodes(tx, workspace, c).await?;
    if !c.enabled {
        sqlx::query("UPDATE runs SET state='cancelled',reason='task paused',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND parent_run_id IS NULL AND state='queued' AND source='schedule'").bind(&c.id).bind(workspace).execute(&mut **tx).await?;
    }
    audit(tx, user, Some(workspace), "task.apply", &c.id).await?;
    Ok(Some(row))
}

pub async fn set_archived(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    id: &str,
    expected: i64,
    archived: bool,
    user: Uuid,
) -> Result<Option<Value>> {
    let mut tx = pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    crate::auth::workspace::check_member(&mut tx, credential, workspace, "operator").await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:{id}"))
        .execute(&mut *tx)
        .await?;
    let Some(row) = sqlx::query(
        "SELECT to_jsonb(t.*) AS item FROM tasks t WHERE workspace_id=$2 AND id=$1 FOR UPDATE",
    )
    .bind(id)
    .bind(workspace)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let old: Value = row.get("item");
    if old["archived_at"].is_null() == !archived {
        return Ok(Some(old));
    }
    ensure!(
        old["revision"].as_i64() == Some(expected),
        "task changed; reload before saving"
    );
    let result = sqlx::query_scalar("UPDATE tasks SET archived_at=CASE WHEN $2 THEN now() ELSE NULL END,enabled=false,config=jsonb_set(config,'{enabled}','false'),next_at=NULL,revision=revision+1,updated_by=$3,updated_at=now() WHERE workspace_id=$4 AND id=$1 RETURNING to_jsonb(tasks.*)").bind(id).bind(archived).bind(user).bind(workspace).fetch_one(&mut *tx).await?;
    if archived {
        sqlx::query("UPDATE runs SET state='cancelled',cancel_requested=true,reason='task archived',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND parent_run_id IS NULL AND state='queued'").bind(id).bind(workspace).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        user,
        Some(workspace),
        if archived {
            "task.archive"
        } else {
            "task.restore"
        },
        id,
    )
    .await?;
    tx.commit().await?;
    Ok(Some(result))
}
