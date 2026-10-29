//! Manual recovery is an atomic change to a frozen graph, never a new graph round.
use crate::{
    auth::workspace::{User, reauthorize},
    config::{Overlap, TaskConfig, duration},
    dag, protocol,
    state::App,
    store,
    store::Error,
    store::dag as dag_store,
};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction, postgres::PgRow};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A claim-time Session race must leave a manual attempt eligible for another explicit retry.
pub fn session_conflict_state(origin: &str) -> &'static str {
    if origin == "manual" {
        "failed"
    } else {
        "skipped"
    }
}

pub fn expired(config: &TaskConfig, window: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    window
        .zip(config.execution.timeout.as_deref())
        .is_some_and(|(start, limit)| {
            duration(limit)
                .is_ok_and(|limit| (now - start).to_std().is_ok_and(|elapsed| elapsed >= limit))
        })
}
pub fn input_executions(prompt: &dag::Prompt, rows: &[PgRow]) -> anyhow::Result<Value> {
    Ok(json!(
        dag::references(prompt)?
            .into_iter()
            .map(|node| {
                let execution = rows
                    .iter()
                    .find(|r| r.get::<String, _>("node_id") == node)
                    .and_then(|r| r.get::<Option<Uuid>, _>("execution_id"));
                json!({"node_id":node,"execution_id":execution})
            })
            .collect::<Vec<_>>()
    ))
}
pub fn source_snapshot(prompt: &dag::Prompt, rows: &[PgRow]) -> anyhow::Result<Value> {
    Ok(
        json!({"input_sources":dag::references(prompt)?,"input_executions":input_executions(prompt, rows)?}),
    )
}
struct Graph {
    parent: PgRow,
    config: TaskConfig,
    rows: Vec<PgRow>,
    archived: bool,
    busy: bool,
}
impl Graph {
    fn definition(&self) -> &dag::Dag {
        self.config.dag.as_ref().expect("validated DAG")
    }
    fn row(&self, node: &str) -> anyhow::Result<&PgRow> {
        self.rows
            .iter()
            .find(|r| r.get::<String, _>("node_id") == node)
            .ok_or_else(Error::missing)
    }
    fn state<'a>(&self, row: &'a PgRow) -> &'a str {
        row.get::<Option<&str>, _>("execution_state")
            .unwrap_or_else(|| row.get("state"))
    }
    fn stopped(&self, row: &PgRow) -> bool {
        self.parent.get::<bool, _>("cancel_requested")
            || matches!(
                self.parent.get::<&str, _>("state"),
                "cancelled" | "timed_out"
            )
            || matches!(
                row.get::<Option<&str>, _>("effective_cause"),
                Some("graph_cancel" | "graph_timeout")
            )
    }
    fn affected(&self, node: &str) -> Vec<&PgRow> {
        self.rows
            .iter()
            .filter(|row| {
                let id: &str = row.get("node_id");
                self.definition().ancestors(id).contains(node)
                    && matches!(
                        (
                            self.state(row),
                            row.get::<Option<&str>, _>("effective_cause")
                        ),
                        ("skipped", Some("dependency"))
                            | (
                                "cancelled" | "skipped",
                                Some("graph_cancel" | "graph_timeout")
                            )
                    )
            })
            .collect()
    }
    fn prompt(&self, node: &dag::Node) -> anyhow::Result<String> {
        let results = self
            .rows
            .iter()
            .filter(|r| self.state(r) == "succeeded")
            .filter_map(|r| {
                r.get::<Option<Value>, _>("result")
                    .map(|v| (r.get::<String, _>("node_id"), v))
            })
            .collect();
        dag::render(&node.prompt, &results)
    }
    fn action(&self, node: &str) -> Result<&'static str, String> {
        let row = self
            .row(node)
            .map_err(|_| "Node does not exist".to_string())?;
        self.config.validate().map_err(|e| e.to_string())?;
        if self.archived {
            return Err("Task is archived".into());
        }
        if self.rows.iter().any(|r| self.state(r) == "unknown") {
            return Err("Resolve unknown executions before retrying".into());
        }
        let stopped = self.stopped(row);
        if self
            .parent
            .get::<Option<DateTime<Utc>>, _>("finished_at")
            .is_none()
            && (self.parent.get::<bool, _>("cancel_requested")
                || expired(
                    &self.config,
                    self.parent.get("window_started_at"),
                    Utc::now(),
                ))
        {
            return Err("Wait until the stopped graph has settled".into());
        }
        if (stopped
            || self
                .parent
                .get::<Option<DateTime<Utc>>, _>("finished_at")
                .is_some())
            && self.rows.iter().any(|r| {
                r.get::<Option<Uuid>, _>("execution_id").is_some()
                    && r.get::<Option<DateTime<Utc>>, _>("execution_finished")
                        .is_none()
                    && (self
                        .parent
                        .get::<Option<DateTime<Utc>>, _>("finished_at")
                        .is_some()
                        || r.get::<Option<i64>, _>("execution_generation").unwrap_or(0)
                            < self.parent.get::<i64, _>("recovery_generation"))
            })
        {
            return Err("Wait until all previous executions have stopped".into());
        }
        if self
            .parent
            .get::<Option<DateTime<Utc>>, _>("finished_at")
            .is_some()
            && self.busy
            && self.config.execution.overlap == Overlap::Skip
        {
            return Err("Task already has another active run".into());
        }
        let definition = self
            .definition()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or("Node snapshot missing")?;
        if !definition
            .pre
            .iter()
            .all(|p| self.row(p).is_ok_and(|r| self.state(r) == "succeeded"))
        {
            return Err("All predecessors must succeed before retrying".into());
        }
        let state = self.state(row);
        if row.get::<Option<Uuid>, _>("execution_id").is_some()
            && matches!(state, "failed" | "timed_out" | "interrupted" | "cancelled")
            && row
                .get::<Option<DateTime<Utc>>, _>("execution_finished")
                .is_none()
        {
            return Err("Wait until the previous execution has stopped".into());
        }
        let action = if state == "queued"
            && row.get::<Option<&str>, _>("attempt_origin") == Some("automatic")
        {
            "accelerate"
        } else if matches!(state, "failed" | "timed_out" | "interrupted")
            || (state == "cancelled" && stopped)
        {
            if stopped { "resume" } else { "retry" }
        } else {
            return Err(format!("Node in state {state} cannot be retried"));
        };
        if row.get::<bool, _>("runner_revoked") {
            return Err("Runner has been revoked".into());
        }
        self.prompt(definition).map_err(|e| e.to_string())?;
        // Validate retained successful inputs and outputs before reopening. Never recompute them.
        let affected: BTreeSet<_> = self
            .affected(node)
            .iter()
            .map(|r| r.get::<String, _>("node_id"))
            .chain(std::iter::once(node.to_string()))
            .collect();
        let mut needed: BTreeSet<_> = self.definition().outputs().into_iter().collect();
        for n in self
            .definition()
            .nodes
            .iter()
            .filter(|n| affected.contains(&n.id))
        {
            needed.extend(dag::references(&n.prompt).map_err(|e| e.to_string())?);
        }
        for id in needed {
            if let Ok(r) = self.row(&id)
                && self.state(r) == "succeeded"
            {
                let result = r.get::<Option<Value>, _>("result").unwrap_or(Value::Null);
                dag::render(
                    &format!("{{{{ nodes.{id}.result }}}}"),
                    &BTreeMap::from([(id, result)]),
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(action)
    }
}
async fn load(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    parent: Uuid,
) -> anyhow::Result<Graph> {
    let parent=sqlx::query("SELECT * FROM runs WHERE workspace_id=$1 AND id=$2 AND parent_run_id IS NULL AND config ? 'dag'").bind(workspace).bind(parent).fetch_optional(&mut **tx).await?.ok_or_else(Error::missing)?;
    let task=sqlx::query("SELECT archived_at,EXISTS(SELECT 1 FROM runs r WHERE r.workspace_id=$1 AND r.task_id=$2 AND r.id<>$3 AND r.parent_run_id IS NULL AND r.finished_at IS NULL) AS busy FROM tasks WHERE workspace_id=$1 AND id=$2").bind(workspace).bind(parent.get::<String,_>("task_id")).bind(parent.get::<Uuid,_>("id")).fetch_one(&mut **tx).await?;
    let config: TaskConfig = serde_json::from_value(parent.get("config"))?;
    let rows=sqlx::query("SELECT n.*,device.revoked AS runner_revoked,r.state AS execution_state,r.finished_at AS execution_finished,r.result,r.attempt_origin,r.recovery_generation AS execution_generation,r.retry_of,r.scheduled_at,COALESCE(r.stop_cause,n.stop_cause) AS effective_cause FROM dag_run_nodes n LEFT JOIN runs r ON r.id=n.execution_id JOIN runners device ON device.id=n.runner_id AND device.workspace_id=n.workspace_id WHERE n.workspace_id=$1 AND n.parent_run_id=$2 ORDER BY n.position").bind(workspace).bind(parent.get::<Uuid,_>("id")).fetch_all(&mut **tx).await?;
    Ok(Graph {
        parent,
        config,
        rows,
        archived: task
            .get::<Option<DateTime<Utc>>, _>("archived_at")
            .is_some(),
        busy: task.get("busy"),
    })
}
pub async fn info(s: &App, workspace: Uuid, parent: Uuid, node: &str) -> anyhow::Result<Value> {
    let mut tx = s.pool.begin().await?;
    let graph = load(&mut tx, workspace, parent).await?;
    graph.row(node)?;
    let (action, reason) = match graph.action(node) {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(e)),
    };
    let affected:Vec<_>=graph.affected(node).iter().map(|r|json!({"id":r.get::<String,_>("node_id"),"name":r.get::<Value,_>("config")["name"],"state":graph.state(r),"action":"resume"})).collect();
    Ok(json!({"action":action,"reason":reason,"affected":affected}))
}
pub async fn retry(
    s: &App,
    actor: &User,
    key: &str,
    parent: Uuid,
    node: &str,
    execution_id: Option<Uuid>,
    resume_stopped: bool,
) -> anyhow::Result<Value> {
    let workspace = actor.workspace()?;
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, actor, "operator").await?;
    let task=sqlx::query_scalar::<_,String>("SELECT task_id FROM runs WHERE workspace_id=$1 AND id=$2 AND parent_run_id IS NULL AND config ? 'dag'").bind(workspace).bind(parent).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    sqlx::query("SELECT id FROM tasks WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
        .bind(workspace)
        .bind(task)
        .execute(&mut *tx)
        .await?;
    sqlx::query("SELECT id FROM runs WHERE id=$1 FOR UPDATE")
        .bind(parent)
        .execute(&mut *tx)
        .await?;
    let graph = load(&mut tx, workspace, parent).await?;
    let row = graph.row(node)?;
    let response = |id: Uuid| json!({"id":id,"parent_run_id":parent,"node_id":node});
    if let Some(previous)=sqlx::query("SELECT * FROM dag_retry_requests WHERE workspace_id=$1 AND user_id=$2 AND parent_run_id=$3 AND node_id=$4 AND request_key=$5").bind(workspace).bind(actor.id).bind(parent).bind(node).bind(key).fetch_optional(&mut *tx).await? {
        if previous.get::<Option<Uuid>,_>("expected_execution")!=execution_id || previous.get::<bool,_>("resume_stopped")!=resume_stopped {return Err(Error::conflict("Idempotency-Key was already used with another request"));}
        return Ok(response(previous.get("execution_id")));
    }
    let current: Option<Uuid> = row.get("execution_id");
    let action = graph.action(node).map_err(Error::conflict)?;
    let same = current == execution_id;
    let auto_successor = action == "accelerate"
        && row.get::<Option<Uuid>, _>("retry_of") == execution_id
        && execution_id.is_some();
    if !same && !auto_successor {
        return Err(Error::conflict(
            "Execution changed; refresh before retrying",
        ));
    }
    if graph.stopped(row) && !resume_stopped {
        return Err(Error::conflict(
            "Explicit resume_stopped is required to resume stopped execution",
        ));
    }
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let id = if action == "accelerate" {
        let id = current.expect("queued attempt has identity");
        let deadline =
            now + chrono::Duration::from_std(duration(&graph.config.execution.start_grace)?)?;
        sqlx::query("UPDATE runs SET scheduled_at=$2,deadline=$3 WHERE id=$1 AND state='queued'")
            .bind(id)
            .bind(now)
            .bind(deadline)
            .execute(&mut *tx)
            .await?;
        crate::store::dag_retry::dispatch(&mut tx, workspace, parent, now).await?;
        id
    } else {
        let definition = graph
            .definition()
            .nodes
            .iter()
            .find(|n| n.id == node)
            .ok_or_else(Error::missing)?;
        let execution = definition.execution(
            &graph.config,
            graph
                .prompt(definition)
                .map_err(|e| Error::conflict(e.to_string()))?,
        );
        // The frozen Runner UUID wins over a display name that may since have changed.
        if let Some(session) = &execution.session {
            let target: Uuid = row.get("runner_id");
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
                .bind(format!(
                    "session:{target}:{}:{}",
                    execution.agent, session.id
                ))
                .execute(&mut *tx)
                .await?;
            crate::store::terminals::refresh_session(
                s,
                &mut tx,
                target,
                &execution.agent,
                &session.id,
            )
            .await?;
            let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE runner_id=$1 AND config->>'agent'=$2 AND COALESCE(session->>'id',config->'session'->>'id')=$3 AND state IN ('starting','running','unknown')) OR EXISTS(SELECT 1 FROM terminals WHERE runner_id=$1 AND agent=$2 AND session_id=$3 AND state<>'closed')")
                .bind(target).bind(&execution.agent).bind(&session.id).fetch_one(&mut *tx).await?;
            if busy {
                return Err(Error::conflict("Session is currently in use"));
            }
        }
        let mut runner_check = execution.clone();
        runner_check.runner = row.get::<Uuid, _>("runner_id").to_string();
        let runner = crate::store::runners::runner(&mut tx, workspace, &runner_check)
            .await
            .map_err(|e| {
                if e.is::<crate::store::runners::RunnerBindingError>() {
                    Error::conflict(e.to_string())
                } else {
                    e
                }
            })?;
        let reopen = graph
            .parent
            .get::<Option<DateTime<Utc>>, _>("finished_at")
            .is_some();
        let generation = graph.parent.get::<i64, _>("recovery_generation") + i64::from(reopen);
        if reopen {
            dag_store::event(&mut tx,workspace,parent,"recovery",json!({"generation":generation,"previous_state":graph.parent.get::<String,_>("state"),"previous_finished_at":graph.parent.get::<Option<DateTime<Utc>>,_>("finished_at"),"node_id":node})).await?;
        }
        sqlx::query("UPDATE runs SET state=CASE WHEN $2 THEN 'queued' ELSE state END,cancel_requested=false,reason=NULL,result=NULL,finished_at=NULL,recovery_generation=$3,window_started_at=CASE WHEN $2 THEN NULL ELSE window_started_at END WHERE id=$1").bind(parent).bind(reopen).bind(generation).execute(&mut *tx).await?;
        for descendant in graph.affected(node) {
            sqlx::query("UPDATE dag_run_nodes SET execution_id=NULL,state='waiting',stop_cause=NULL,reason=NULL,started_at=NULL,finished_at=NULL WHERE parent_run_id=$1 AND node_id=$2").bind(parent).bind(descendant.get::<String,_>("node_id")).execute(&mut *tx).await?;
        }
        let id = Uuid::new_v4();
        let deadline =
            now + chrono::Duration::from_std(duration(&graph.config.execution.start_grace)?)?;
        sqlx::query("INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source,parent_run_id,node_id,retry_of,execution_snapshot,attempt_origin,recovery_generation) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'dag_node',$9,$10,$11,$12,'manual',$13)")
            .bind(workspace).bind(id).bind(&graph.config.id).bind(graph.parent.get::<i64,_>("task_revision")).bind(runner).bind(serde_json::to_value(&execution)?).bind(now).bind(deadline).bind(parent).bind(node).bind(current).bind(source_snapshot(&definition.prompt,&graph.rows)?).bind(generation).execute(&mut *tx).await?;
        sqlx::query("UPDATE dag_run_nodes SET execution_id=$3,state='queued',stop_cause=NULL,reason=NULL,started_at=NULL,finished_at=NULL WHERE parent_run_id=$1 AND node_id=$2").bind(parent).bind(node).bind(id).execute(&mut *tx).await?;
        store::outbox::enqueue(
            &mut tx,
            workspace,
            protocol::work_subject(runner),
            format!("start:{id}"),
            serde_json::to_value(protocol::Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: execution,
                deadline,
            })?,
        )
        .await?;
        dag_store::event(&mut tx,workspace,parent,"node",json!({"node_id":node,"execution_id":id,"retry_of":current,"state":"queued","attempt_origin":"manual","recovery_generation":generation})).await?;
        id
    };
    sqlx::query("INSERT INTO dag_retry_requests(workspace_id,user_id,parent_run_id,node_id,request_key,expected_execution,resume_stopped,execution_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(workspace).bind(actor.id).bind(parent).bind(node).bind(key).bind(execution_id).bind(resume_stopped).bind(id).execute(&mut *tx).await?;
    store::audit(
        &mut tx,
        actor.id,
        Some(workspace),
        "run.node.retry",
        &parent.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(response(id))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_manual_session_race_is_recoverable_without_changing_ordinary_claims() {
        assert_eq!(session_conflict_state("manual"), "failed");
        assert_eq!(session_conflict_state("initial"), "skipped");
        assert_eq!(session_conflict_state("automatic"), "skipped");
    }
    #[test]
    fn ordinary_defaults_stay_concrete_for_legacy_runners() {
        for value in [json!({}), json!({"timeout":null}), json!({"timeout":"2h"})] {
            let c:TaskConfig=serde_json::from_value(json!({"id":"ordinary","name":"Ordinary","runner":"device","agent":"pi","cwd":"/workspace","prompt":"Run","execution":value})).unwrap();
            c.validate().unwrap();
            assert!(serde_json::to_value(&c).unwrap()["execution"]["timeout"].is_string());
        }
    }
    #[test]
    fn total_deadline_is_optional_and_uses_recovery_window() {
        let mut c: TaskConfig = serde_json::from_value(
            json!({"version":1,"id":"graph","name":"Graph","dag":{"nodes":[]}}),
        )
        .unwrap();
        let now = Utc::now();
        assert!(!expired(&c, Some(now - chrono::Duration::hours(2)), now));
        c.execution.timeout = Some("1h".into());
        assert!(expired(&c, Some(now - chrono::Duration::hours(2)), now));
        assert!(!expired(&c, Some(now - chrono::Duration::minutes(1)), now));
        assert!(!expired(&c, None, now));
    }
}
