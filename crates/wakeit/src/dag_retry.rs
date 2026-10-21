//! Retry scheduling is Gateway-owned; attempts remain ordinary child Runs.
use crate::{
    config::{TaskConfig, duration},
    protocol, store,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rand::Rng;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub fn delay(retry: u32, factor: f64) -> std::time::Duration {
    let seconds = 5u64
        .saturating_mul(1u64 << retry.saturating_sub(1).min(4))
        .min(60);
    std::time::Duration::from_millis((seconds as f64 * 1000. * factor.clamp(0.8, 1.)) as u64)
}

pub async fn prepare(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    parent: Uuid,
    config: &TaskConfig,
    now: DateTime<Utc>,
) -> Result<()> {
    let rows = sqlx::query("SELECT r.*,n.config AS node_config,(SELECT count(*) FROM runs a WHERE a.parent_run_id=r.parent_run_id AND a.node_id=r.node_id) AS attempt_count FROM dag_run_nodes n JOIN runs r ON r.id=n.execution_id WHERE n.parent_run_id=$1 AND r.state IN ('failed','timed_out') AND r.finished_at IS NOT NULL AND NOT r.cancel_requested AND EXISTS(SELECT 1 FROM events e WHERE e.run_id=r.id AND e.seq=r.last_state_seq AND e.kind='state' AND ((e.data->>'retryable')='true' OR (r.state='timed_out' AND e.data->>'state'='timed_out'))) ORDER BY n.position").bind(parent).fetch_all(&mut **tx).await?;
    for row in rows {
        let node: crate::dag::Node = serde_json::from_value(row.get("node_config"))?;
        let count: i64 = row.get("attempt_count");
        if count > i64::from(node.max_retries) {
            continue;
        }
        let at = now
            + chrono::Duration::from_std(delay(
                count as u32,
                rand::thread_rng().gen_range(0.8..=1.),
            ))?;
        let deadline = at + chrono::Duration::from_std(duration(&config.execution.start_grace)?)?;
        let previous: Uuid = row.get("id");
        let id = Uuid::new_v4();
        let snapshot: Option<Value> = row.get("execution_snapshot");
        let sources = snapshot
            .as_ref()
            .and_then(|s| s.get("input_sources"))
            .cloned()
            .unwrap_or(json!([]));
        sqlx::query("INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source,parent_run_id,node_id,retry_of,request_key,execution_snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'dag_node',$9,$10,$11,$12,$13)")
            .bind(workspace).bind(id).bind(&config.id).bind(row.get::<i64,_>("task_revision")).bind(row.get::<Uuid,_>("runner_id")).bind(row.get::<Value,_>("config")).bind(at).bind(deadline).bind(parent).bind(&node.id).bind(previous).bind(format!("dag-retry:{previous}")).bind(json!({"input_sources":sources})).execute(&mut **tx).await?;
        sqlx::query("UPDATE dag_run_nodes SET execution_id=$3,state='queued',reason=NULL,finished_at=NULL WHERE parent_run_id=$1 AND node_id=$2").bind(parent).bind(&node.id).bind(id).execute(&mut **tx).await?;
        crate::dag_store::event(tx, workspace, parent, "node", json!({"node_id":node.id,"execution_id":id,"retry_of":previous,"state":"queued","scheduled_at":at})).await?;
    }
    dispatch(tx, workspace, parent, now).await
}

pub async fn dispatch(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    parent: Uuid,
    now: DateTime<Utc>,
) -> Result<()> {
    let rows=sqlx::query("SELECT r.* FROM runs r JOIN dag_run_nodes n ON n.execution_id=r.id WHERE r.parent_run_id=$1 AND r.state='queued' AND r.retry_of IS NOT NULL AND NOT r.cancel_requested AND r.scheduled_at <= $2 AND r.deadline >= $2 ORDER BY r.id FOR UPDATE OF r").bind(parent).bind(now).fetch_all(&mut **tx).await?;
    for row in rows {
        let id: Uuid = row.get("id");
        let runner: Uuid = row.get("runner_id");
        store::enqueue(
            tx,
            workspace,
            protocol::work_subject(runner),
            format!("start:{id}"),
            serde_json::to_value(protocol::Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: serde_json::from_value(row.get("config"))?,
                deadline: row.get("deadline"),
            })?,
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_backoff() {
        assert_eq!(delay(1, 1.).as_secs(), 5);
        assert_eq!(delay(2, 1.).as_secs(), 10);
        assert_eq!(delay(3, 1.).as_secs(), 20);
        assert_eq!(delay(10, 1.).as_secs(), 60);
        assert_eq!(delay(u32::MAX, 1.).as_secs(), 60);
        assert_eq!(delay(1, 0.).as_secs(), 4);
    }
}
