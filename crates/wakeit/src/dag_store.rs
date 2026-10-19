//! Gateway-owned orchestration. Runners only receive ordinary version-1 assignments.
use crate::{
    config::{TaskConfig, duration},
    dag::{self, ValidationIssue},
    protocol, store,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::BTreeMap;
use uuid::Uuid;

pub async fn runner(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    c: &TaskConfig,
) -> Result<Uuid> {
    let row=sqlx::query("SELECT id,agents FROM runners WHERE workspace_id=$1 AND (id::text=$2 OR name=$2) AND NOT revoked")
        .bind(workspace).bind(&c.runner).fetch_optional(&mut **tx).await?.context("runner does not exist or is revoked")?;
    let agents: Value = row.get("agents");
    let agent = agents
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == c.agent))
        .context("agent is not advertised by this runner")?;
    anyhow::ensure!(
        c.session.is_none() || agent["session_supported"] == true,
        "agent does not advertise session support"
    );
    crate::config::resolve_parameters(
        &serde_json::from_value(agent.get("parameters").cloned().unwrap_or(json!({})))?,
        &c.parameters,
    )?;
    Ok(row.get("id"))
}
pub async fn validation(
    pool: &PgPool,
    workspace: Uuid,
    c: &TaskConfig,
) -> Result<Vec<ValidationIssue>> {
    let mut result = c.dag.as_ref().map(|d| d.issues()).unwrap_or_default();
    if let Err(e) = c.validate()
        && result.is_empty()
    {
        result.push(ValidationIssue {
            path: if duration(&c.execution.timeout).is_err() {
                "execution.timeout"
            } else if duration(&c.execution.start_grace).is_err() {
                "execution.start_grace"
            } else if c
                .schedule
                .as_ref()
                .is_some_and(|s| crate::schedule::validate(s).is_err())
            {
                "schedule"
            } else {
                "config"
            }
            .into(),
            node_id: None,
            message: e.to_string(),
            cycle: vec![],
        });
    }
    if !result.is_empty() {
        return Ok(result);
    }
    let mut tx = pool.begin().await?;
    if let Some(d) = &c.dag {
        for n in &d.nodes {
            if let Err(e) = runner(&mut tx, workspace, &n.execution(c, n.prompt.clone())).await {
                result.push(ValidationIssue {
                    path: format!("dag.nodes.{}.runner", n.id),
                    node_id: Some(n.id.clone()),
                    message: e.to_string(),
                    cycle: vec![],
                });
            }
        }
    } else if let Err(e) = runner(&mut tx, workspace, c).await {
        result.push(ValidationIssue {
            path: "runner".into(),
            node_id: None,
            message: e.to_string(),
            cycle: vec![],
        });
    }
    Ok(result)
}
pub async fn save_nodes(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    c: &TaskConfig,
) -> Result<()> {
    sqlx::query("DELETE FROM dag_nodes WHERE workspace_id=$1 AND task_id=$2")
        .bind(workspace)
        .bind(&c.id)
        .execute(&mut **tx)
        .await?;
    if let Some(d) = &c.dag {
        for n in &d.nodes {
            let runner = runner(tx, workspace, &n.execution(c, n.prompt.clone())).await?;
            sqlx::query(
                "INSERT INTO dag_nodes(workspace_id,task_id,node_id,runner_id) VALUES($1,$2,$3,$4)",
            )
            .bind(workspace)
            .bind(&c.id)
            .bind(&n.id)
            .bind(runner)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}
pub async fn seed(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    id: Uuid,
    c: &TaskConfig,
) -> Result<()> {
    if let Some(d) = &c.dag {
        for (position, n) in d.nodes.iter().enumerate() {
            let runner:Uuid=sqlx::query_scalar("SELECT runner_id FROM dag_nodes WHERE workspace_id=$1 AND task_id=$2 AND node_id=$3").bind(workspace).bind(&c.id).bind(&n.id).fetch_one(&mut **tx).await?;
            sqlx::query("INSERT INTO dag_run_nodes(workspace_id,parent_run_id,node_id,position,runner_id,config) VALUES($1,$2,$3,$4,$5,$6)").bind(workspace).bind(id).bind(&n.id).bind(position as i32).bind(runner).bind(serde_json::to_value(n)?).execute(&mut **tx).await?;
        }
    }
    Ok(())
}

/// Task → graph → execution is the common lock order for claims, events and orchestration.
pub async fn lock_parent(
    tx: &mut Transaction<'_, Postgres>,
    execution: Uuid,
) -> Result<Option<Uuid>> {
    let row = sqlx::query("SELECT parent_run_id,workspace_id,task_id FROM runs WHERE id=$1")
        .bind(execution)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let parent: Option<Uuid> = row.get("parent_run_id");
    if let Some(parent) = parent {
        sqlx::query("SELECT id FROM tasks WHERE workspace_id=$1 AND id=$2 FOR UPDATE")
            .bind(row.get::<Uuid, _>("workspace_id"))
            .bind(row.get::<String, _>("task_id"))
            .execute(&mut **tx)
            .await?;
        sqlx::query("SELECT id FROM runs WHERE id=$1 FOR UPDATE")
            .bind(parent)
            .execute(&mut **tx)
            .await?;
    }
    Ok(parent)
}
pub async fn event(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    id: Uuid,
    kind: &str,
    data: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO events(workspace_id,run_id,seq,kind,data) SELECT $1,$2,COALESCE(MAX(seq),0)+1,$3,$4 FROM events WHERE run_id=$2")
        .bind(workspace).bind(id).bind(kind).bind(data).execute(&mut **tx).await?;
    Ok(())
}
pub async fn cancel_nodes(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    parent: Uuid,
) -> Result<()> {
    sqlx::query("UPDATE dag_run_nodes SET state='cancelled',reason='graph cancelled',finished_at=now() WHERE parent_run_id=$1 AND execution_id IS NULL AND state='waiting'").bind(parent).execute(&mut **tx).await?;
    let rows=sqlx::query("SELECT id,runner_id,state FROM runs WHERE parent_run_id=$1 AND finished_at IS NULL ORDER BY id FOR UPDATE").bind(parent).fetch_all(&mut **tx).await?;
    for row in rows {
        let id: Uuid = row.get("id");
        let state: String = row.get("state");
        if state == "queued" {
            sqlx::query("UPDATE runs SET state='cancelled',cancel_requested=true,reason='graph cancelled',finished_at=now() WHERE id=$1").bind(id).execute(&mut **tx).await?;
        } else {
            sqlx::query("UPDATE runs SET cancel_requested=true WHERE id=$1")
                .bind(id)
                .execute(&mut **tx)
                .await?;
            store::enqueue(
                tx,
                workspace,
                protocol::control_subject(row.get("runner_id")),
                format!("dag-cancel:{parent}:{id}"),
                json!({"version":1,"run_id":id,"action":"cancel"}),
            )
            .await?;
        }
    }
    Ok(())
}

pub async fn advance(pool: &PgPool) -> Result<()> {
    let parents=sqlx::query("SELECT id,task_id,workspace_id FROM runs WHERE config ? 'dag' AND finished_at IS NULL AND parent_run_id IS NULL ORDER BY created_at LIMIT 100").fetch_all(pool).await?;
    for parent in parents {
        let id: Uuid = parent.get("id");
        let workspace: Uuid = parent.get("workspace_id");
        let mut tx = pool.begin().await?;
        let locked = sqlx::query(
            "SELECT id FROM tasks WHERE workspace_id=$1 AND id=$2 FOR UPDATE SKIP LOCKED",
        )
        .bind(workspace)
        .bind(parent.get::<String, _>("task_id"))
        .fetch_optional(&mut *tx)
        .await?;
        if locked.is_none() {
            continue;
        }
        let graph = sqlx::query("SELECT * FROM runs WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        if graph
            .get::<Option<DateTime<Utc>>, _>("finished_at")
            .is_some()
        {
            continue;
        }
        let c: TaskConfig = serde_json::from_value(graph.get("config"))?;
        let dag = c.dag.as_ref().context("graph definition missing")?;
        let mut stop = graph.get::<bool, _>("cancel_requested");
        let mut reason: Option<String> = graph.get("reason");
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx)
            .await?;
        let started: Option<DateTime<Utc>> = graph.get("started_at");
        if !stop
            && started.is_some_and(|s| {
                now - s
                    > chrono::Duration::from_std(duration(&c.execution.timeout).unwrap()).unwrap()
            })
        {
            stop = true;
            reason = Some("graph timeout".into());
            sqlx::query("UPDATE runs SET cancel_requested=true,reason='graph timeout' WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        if stop {
            cancel_nodes(&mut tx, workspace, id).await?;
        }
        let rows=sqlx::query("SELECT n.*,r.result,r.state AS execution_state,r.reason AS execution_reason,r.started_at AS execution_started,r.finished_at AS execution_finished FROM dag_run_nodes n LEFT JOIN runs r ON r.id=n.execution_id WHERE n.parent_run_id=$1 ORDER BY n.position").bind(id).fetch_all(&mut *tx).await?;
        let mut states = BTreeMap::new();
        let mut results = BTreeMap::new();
        let mut reasons = BTreeMap::new();
        for row in &rows {
            let node: String = row.get("node_id");
            let state = row
                .get::<Option<String>, _>("execution_state")
                .unwrap_or_else(|| row.get("state"));
            let why = row
                .get::<Option<String>, _>("execution_reason")
                .or_else(|| row.get("reason"));
            if state != row.get::<String, _>("state") {
                sqlx::query("UPDATE dag_run_nodes SET state=$3,reason=$4,started_at=$5,finished_at=$6 WHERE parent_run_id=$1 AND node_id=$2").bind(id).bind(&node).bind(&state).bind(&why).bind(row.get::<Option<DateTime<Utc>>,_>("execution_started")).bind(row.get::<Option<DateTime<Utc>>,_>("execution_finished")).execute(&mut *tx).await?;
                event(
                    &mut tx,
                    workspace,
                    id,
                    "node",
                    json!({"node_id":node,"state":state,"reason":why}),
                )
                .await?;
            }
            if let Some(result) = row.get::<Option<Value>, _>("result") {
                results.insert(node.clone(), result);
            }
            states.insert(node.clone(), state);
            reasons.insert(node, why);
        }
        if !stop {
            // Repeat only for newly impossible descendants; no successful result is inferred.
            loop {
                let mut changed = false;
                for row in &rows {
                    let node_id: String = row.get("node_id");
                    if states.get(&node_id).is_none_or(|s| s != "waiting") {
                        continue;
                    }
                    let n = dag
                        .nodes
                        .iter()
                        .find(|n| n.id == node_id)
                        .context("node snapshot missing")?;
                    let failed = n.pre.iter().find(|p| {
                        states
                            .get(*p)
                            .is_some_and(|s| protocol::terminal(s) && s != "succeeded")
                    });
                    let terminal = failed.map(|failed| {
                        ("skipped", format!("Predecessor did not succeed: {failed}"))
                    });
                    if let Some((state, why)) = terminal {
                        sqlx::query("UPDATE dag_run_nodes SET state=$3,reason=$4,finished_at=now() WHERE parent_run_id=$1 AND node_id=$2").bind(id).bind(&node_id).bind(state).bind(&why).execute(&mut *tx).await?;
                        states.insert(node_id.clone(), state.into());
                        reasons.insert(node_id.clone(), Some(why.clone()));
                        event(
                            &mut tx,
                            workspace,
                            id,
                            "node",
                            json!({"node_id":node_id,"state":state,"reason":why}),
                        )
                        .await?;
                        changed = true;
                        continue;
                    }
                    if !n
                        .pre
                        .iter()
                        .all(|p| states.get(p).is_some_and(|s| s == "succeeded"))
                    {
                        continue;
                    }
                    let prompt = dag::render(&n.prompt, &results);
                    let revoked: bool =
                        sqlx::query_scalar("SELECT revoked FROM runners WHERE id=$1")
                            .bind(row.get::<Uuid, _>("runner_id"))
                            .fetch_one(&mut *tx)
                            .await?;
                    let prompt = if revoked {
                        Err(anyhow::anyhow!("Runner has been revoked"))
                    } else {
                        prompt
                    };
                    match prompt {
                        Err(e) => {
                            let why = e.to_string();
                            sqlx::query("UPDATE dag_run_nodes SET state='failed',reason=$3,finished_at=now() WHERE parent_run_id=$1 AND node_id=$2").bind(id).bind(&node_id).bind(&why).execute(&mut *tx).await?;
                            states.insert(node_id.clone(), "failed".into());
                            reasons.insert(node_id.clone(), Some(why.clone()));
                            event(
                                &mut tx,
                                workspace,
                                id,
                                "node",
                                json!({"node_id":node_id,"state":"failed","reason":why}),
                            )
                            .await?;
                            changed = true;
                        }
                        Ok(prompt) => {
                            let execution_id = Uuid::new_v4();
                            let runner: Uuid = row.get("runner_id");
                            let execution = n.execution(&c, prompt);
                            let deadline = now
                                + chrono::Duration::from_std(duration(&c.execution.start_grace)?)?;
                            sqlx::query("INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source,parent_run_id,node_id,execution_snapshot) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'dag_node',$9,$10,$11)")
                                .bind(workspace).bind(execution_id).bind(&c.id).bind(graph.get::<i64,_>("task_revision")).bind(runner).bind(serde_json::to_value(&execution)?).bind(now).bind(deadline).bind(id).bind(&n.id).bind(json!({"input_sources":dag::references(&n.prompt)?})).execute(&mut *tx).await?;
                            sqlx::query("UPDATE dag_run_nodes SET execution_id=$3,state='queued' WHERE parent_run_id=$1 AND node_id=$2").bind(id).bind(&node_id).bind(execution_id).execute(&mut *tx).await?;
                            store::enqueue(
                                &mut tx,
                                workspace,
                                protocol::work_subject(runner),
                                format!("start:{execution_id}"),
                                serde_json::to_value(protocol::Assignment {
                                    version: 1,
                                    run_id: execution_id,
                                    runner_id: runner,
                                    task: execution,
                                    deadline,
                                })?,
                            )
                            .await?;
                            states.insert(node_id.clone(), "queued".into());
                            event(
                                &mut tx,
                                workspace,
                                id,
                                "node",
                                json!({"node_id":node_id,"state":"queued"}),
                            )
                            .await?;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        let derived = dag::outcome(dag, &states);
        let terminal = protocol::terminal(derived);
        let new_state = if stop && terminal {
            if reason.as_deref() == Some("graph timeout") {
                "timed_out"
            } else {
                "cancelled"
            }
        } else if derived == "running" && started.is_none() {
            "queued"
        } else {
            derived
        };
        let terminal = protocol::terminal(new_state);
        let result = if terminal {
            let selected = dag.outputs();
            Some(
                json!({"type":"dag","outputs":dag.nodes.iter().filter(|n|selected.contains(&n.id)).map(|n|json!({"node_id":n.id,"name":n.name,"state":states.get(&n.id),"reason":reasons.get(&n.id).cloned().flatten(),"result":results.get(&n.id)})).collect::<Vec<_>>()}),
            )
        } else {
            None
        };
        if reason.is_none() && matches!(new_state, "failed" | "warning") {
            reason = Some(
                states
                    .iter()
                    .filter(|(_, s)| protocol::terminal(s) && s.as_str() != "succeeded")
                    .map(|(id, s)| {
                        format!(
                            "{}: {}",
                            dag.nodes
                                .iter()
                                .find(|n| &n.id == id)
                                .map_or(id.as_str(), |n| n.name.as_str()),
                            reasons.get(id).and_then(|v| v.as_deref()).unwrap_or(s)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        let previous: String = graph.get("state");
        if previous != new_state || terminal {
            sqlx::query("UPDATE runs SET state=$2,reason=$3,result=COALESCE($4,result),finished_at=CASE WHEN $5 THEN now() ELSE NULL END WHERE id=$1").bind(id).bind(new_state).bind(&reason).bind(&result).bind(terminal).execute(&mut *tx).await?;
            event(
                &mut tx,
                workspace,
                id,
                "state",
                json!({"state":new_state,"reason":reason,"result":result}),
            )
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

pub async fn summaries(pool: &PgPool, workspace: Uuid, id: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT n.config || jsonb_build_object('execution_id',n.execution_id,'state',COALESCE(r.state,n.state),'reason',COALESCE(r.reason,n.reason),'started_at',COALESCE(r.started_at,n.started_at),'finished_at',COALESCE(r.finished_at,n.finished_at)) FROM dag_run_nodes n LEFT JOIN runs r ON r.id=n.execution_id WHERE n.workspace_id=$1 AND n.parent_run_id=$2 ORDER BY n.position").bind(workspace).bind(id).fetch_all(pool).await?)
}
