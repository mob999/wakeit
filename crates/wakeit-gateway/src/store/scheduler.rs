use crate::{
    config::{Misfire, Overlap, TaskConfig, duration},
    schedule,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::runs::{NewRun, create_run};
pub async fn tick(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let rows=sqlx::query("SELECT * FROM tasks WHERE enabled AND archived_at IS NULL AND next_at<=now() AND EXISTS(SELECT 1 FROM workspaces w WHERE w.id=tasks.workspace_id AND NOT w.is_demo) ORDER BY next_at LIMIT 50 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for row in rows {
        let workspace: Uuid = row.get("workspace_id");
        let task: TaskConfig = serde_json::from_value(row.get("config"))?;
        let Some(s) = &task.schedule else { continue };
        let first: DateTime<Utc> = row.get("next_at");
        let due = if let Some(e) = &s.every {
            schedule::latest_interval(first, now, e)?
        } else if s.cron.is_some() {
            schedule::previous_cron(s, now)?.max(first)
        } else {
            first
        };
        let next = if s.every.is_some() {
            schedule::next(s, due, false)?
        } else {
            schedule::next(s, now, false)?
        };
        sqlx::query("UPDATE tasks SET next_at=$2 WHERE workspace_id=$3 AND id=$1")
            .bind(&task.id)
            .bind(next)
            .bind(workspace)
            .execute(&mut *tx)
            .await?;
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$2 AND task_id=$1 AND parent_run_id IS NULL AND state IN ('starting','running','unknown'))").bind(&task.id).bind(workspace).fetch_one(&mut *tx).await?;
        let late = (now - due).to_std().unwrap_or_default();
        let reason = if busy && task.execution.overlap == Overlap::Skip {
            Some("overlap")
        } else if late > duration(&task.execution.start_grace)? {
            Some("start deadline elapsed")
        } else if task.execution.misfire == Misfire::Skip && late.as_secs() > 5 {
            Some("misfire")
        } else {
            None
        };
        // A task row lock is also taken by claim, so superseding cannot race a start.
        if task.execution.misfire == Misfire::Latest {
            sqlx::query("UPDATE runs SET state='skipped',reason='superseded by latest occurrence',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND source='schedule' AND state='queued'").bind(&task.id).bind(workspace).execute(&mut *tx).await?;
        }
        create_run(
            &mut tx,
            NewRun {
                workspace,
                task: &task,
                revision: row.get("revision"),
                runner: row.get::<Option<Uuid>, _>("runner_id"),
                at: due,
                source: "schedule",
                retry: None,
                key: None,
                state: if reason.is_some() {
                    "skipped"
                } else {
                    "queued"
                },
                reason,
            },
        )
        .await?;
    }
    tx.commit().await?;
    sqlx::query("UPDATE runs SET state='skipped',reason='start deadline elapsed',finished_at=now() WHERE state='queued' AND NOT (config ? 'dag') AND deadline<now()").execute(pool).await?;
    sqlx::query("UPDATE runs SET state='skipped',reason='misfire',finished_at=now() WHERE state='queued' AND source='schedule' AND config->'execution'->>'misfire'='skip' AND scheduled_at<now()-interval '5 seconds'").execute(pool).await?;
    // A missing heartbeat is ambiguous, not permission to start another execution.
    sqlx::query("UPDATE runs SET state='unknown',reason='runner heartbeat lost' WHERE runner_id IS NOT NULL AND state IN ('starting','running') AND COALESCE(last_seen,started_at)<now()-interval '90 seconds'").execute(pool).await?;
    sqlx::query("UPDATE runs n SET state='cancelled',cancel_requested=true,reason='graph did not start',finished_at=now() FROM runs p WHERE n.parent_run_id=p.id AND p.finished_at IS NOT NULL AND p.started_at IS NULL AND n.state='queued'").execute(pool).await?;
    sqlx::query("UPDATE dag_run_nodes n SET state='skipped',reason=COALESCE(p.reason,p.state),finished_at=p.finished_at FROM runs p WHERE n.parent_run_id=p.id AND p.finished_at IS NOT NULL AND p.started_at IS NULL AND n.execution_id IS NULL AND n.finished_at IS NULL").execute(pool).await?;
    crate::store::dag::advance(pool).await?;
    Ok(())
}
