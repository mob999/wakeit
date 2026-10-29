use crate::{
    config::{Overlap, TaskConfig, duration},
    protocol::{self, Assignment},
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::store::{audit, outbox::enqueue};
pub(super) struct NewRun<'a> {
    pub(super) workspace: Uuid,
    pub(super) task: &'a TaskConfig,
    pub(super) revision: i64,
    pub(super) runner: Option<Uuid>,
    pub(super) at: DateTime<Utc>,
    pub(super) source: &'a str,
    pub(super) retry: Option<Uuid>,
    pub(super) key: Option<String>,
    pub(super) state: &'a str,
    pub(super) reason: Option<&'a str>,
}

pub(super) async fn create_run(
    tx: &mut Transaction<'_, Postgres>,
    run: NewRun<'_>,
) -> Result<Uuid> {
    let NewRun {
        workspace,
        task,
        revision,
        runner,
        at,
        source,
        retry,
        key,
        state,
        reason,
    } = run;
    if source == "schedule"
        && let Some(id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM runs WHERE workspace_id=$3 AND task_id=$1 AND scheduled_at=$2 AND source='schedule'",
        )
        .bind(&task.id)
        .bind(at)
        .bind(workspace).fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    let deadline = at + chrono::Duration::from_std(duration(&task.execution.start_grace)?)?;
    sqlx::query("INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source,retry_of,request_key,state,reason,finished_at) VALUES($13,$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,CASE WHEN $11='skipped' THEN now() ELSE NULL END)")
        .bind(id).bind(&task.id).bind(revision).bind(runner).bind(serde_json::to_value(task)?).bind(at).bind(deadline).bind(source).bind(retry).bind(key).bind(state).bind(reason).bind(workspace).execute(&mut **tx).await?;
    sqlx::query("UPDATE runs r SET template_origin=t.template_origin FROM tasks t WHERE r.id=$1 AND t.workspace_id=$2 AND t.id=$3")
        .bind(id).bind(workspace).bind(&task.id).execute(&mut **tx).await?;
    if task.dag.is_some() {
        crate::store::dag::seed(tx, workspace, id, task).await?;
    }
    if state == "queued"
        && let Some(runner) = runner
    {
        enqueue(
            tx,
            workspace,
            protocol::work_subject(runner),
            format!("start:{id}"),
            serde_json::to_value(Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: task.clone(),
                deadline,
            })?,
        )
        .await?;
    }
    Ok(id)
}

pub async fn trigger(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    task_id: &str,
    user: Uuid,
    retry: Option<Uuid>,
    key: &str,
) -> Result<Uuid> {
    let mut tx = pool.begin().await?;
    let id = trigger_tx(&mut tx, workspace, credential, task_id, user, retry, key).await?;
    tx.commit().await?;
    Ok(id)
}

pub(crate) async fn trigger_tx(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    credential: &str,
    task_id: &str,
    user: Uuid,
    retry: Option<Uuid>,
    key: &str,
) -> Result<Uuid> {
    crate::store::workspaces::lock(tx, workspace).await?;
    crate::auth::workspace::check_member(tx, credential, workspace, "operator").await?;
    let request_key = format!(
        "{user}:{}:{key}",
        retry
            .map(|i| i.to_string())
            .unwrap_or_else(|| task_id.into())
    );
    let row = sqlx::query("SELECT * FROM tasks WHERE workspace_id=$2 AND id=$1 FOR UPDATE")
        .bind(task_id)
        .bind(workspace)
        .fetch_optional(&mut **tx)
        .await?
        .context("task not found")?;
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM runs WHERE workspace_id=$2 AND request_key=$1",
    )
    .bind(&request_key)
    .bind(workspace)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Ok(id);
    }
    ensure!(
        row.get::<Option<DateTime<Utc>>, _>("archived_at").is_none(),
        "task is archived; restore before triggering"
    );
    let task: TaskConfig = serde_json::from_value(row.get("config"))?;
    if let Some(runner) = row.get::<Option<Uuid>, _>("runner_id") {
        let revoked: bool = sqlx::query_scalar("SELECT revoked FROM runners WHERE id=$1")
            .bind(runner)
            .fetch_one(&mut **tx)
            .await?;
        ensure!(!revoked, "assigned runner has been revoked");
    }
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$2 AND task_id=$1 AND parent_run_id IS NULL AND state IN ('queued','starting','running','unknown'))").bind(task_id).bind(workspace).fetch_one(&mut **tx).await?;
    ensure!(
        !busy || task.execution.overlap == Overlap::Allow,
        "task already has an active run"
    );
    if let Some(previous) = retry {
        let state: String = sqlx::query_scalar(
            "SELECT state FROM runs WHERE workspace_id=$3 AND id=$1 AND task_id=$2 AND parent_run_id IS NULL",
        )
        .bind(previous)
        .bind(task_id)
        .bind(workspace)
        .fetch_one(&mut **tx)
        .await?;
        ensure!(
            protocol::terminal(&state),
            "resolve the previous run before retrying"
        );
    }
    let now = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut **tx)
        .await?;
    let id = create_run(
        tx,
        NewRun {
            workspace,
            task: &task,
            revision: row.get("revision"),
            runner: row.get::<Option<Uuid>, _>("runner_id"),
            at: now,
            source: if retry.is_some() { "retry" } else { "manual" },
            retry,
            key: Some(request_key),
            state: "queued",
            reason: None,
        },
    )
    .await?;
    audit(tx, user, Some(workspace), "run.trigger", &id.to_string()).await?;
    Ok(id)
}
