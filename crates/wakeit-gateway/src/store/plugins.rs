//! Durable extension events, subscriptions, operations and remote decision gates.
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;
use wakeit_core::extensions::{Actor, Decision, Invocation};

pub fn matches(filter: &Value, data: &Value) -> bool {
    filter.as_object().is_none_or(|f| {
        f.iter().all(|(key, expected)| {
            let pointer = format!("/{}", key.split('.').collect::<Vec<_>>().join("/"));
            let actual = data.pointer(&pointer).unwrap_or(&Value::Null);
            expected
                .as_array()
                .map(|values| values.contains(actual))
                .unwrap_or(expected == actual)
        })
    })
}
pub async fn route(pool: &PgPool, workspace: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    let events=sqlx::query("SELECT * FROM plugin_events WHERE workspace_id=$1 AND NOT routed ORDER BY committed_at,id LIMIT 100 FOR UPDATE SKIP LOCKED").bind(workspace).fetch_all(&mut *tx).await?;
    for e in events {
        let id: Uuid = e.get("id");
        let event_type: String = e.get("event_type");
        let data: Value = e.get("data");
        let bindings=sqlx::query("SELECT b.*,i.version,i.revision AS instance_revision,i.declaration FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.workspace_id=$1 AND b.enabled AND i.enabled AND EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=i.workspace_id AND m.user_id=i.managed_by AND m.active AND u.active AND m.role='admin') AND b.kind='event' AND (b.event=$2 OR b.event='*' OR (right(b.event,2)='.*' AND left($2,length(b.event)-1)=left(b.event,length(b.event)-1))) AND b.created_at<=$3 ORDER BY b.position,b.id")
            .bind(workspace).bind(event_type).bind(e.get::<DateTime<Utc>,_>("committed_at")).fetch_all(&mut *tx).await?;
        for b in bindings {
            if !matches(&b.get::<Value, _>("filter"), &data) {
                continue;
            }
            sqlx::query("INSERT INTO plugin_deliveries(workspace_id,event_id,binding_id,instance_id,runner_id,expires_at,context) VALUES($1,$2,$3,$4,$5,LEAST($6,now()+make_interval(secs=>$7)), $8) ON CONFLICT DO NOTHING")
                .bind(workspace).bind(id).bind(b.get::<Uuid,_>("id")).bind(b.get::<Uuid,_>("instance_id")).bind(b.get::<Uuid,_>("runner_id")).bind(e.get::<DateTime<Utc>,_>("expires_at")).bind(b.get::<i32,_>("expires_seconds") as f64).bind(json!({"version":b.get::<String,_>("version"),"revision":b.get::<i64,_>("instance_revision"),"handler":b.get::<String,_>("handler"),"binding_revision":b.get::<i64,_>("revision")})).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE plugin_events SET routed=true WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn poll(pool: &PgPool, workspace: Uuid, runner: Uuid) -> Result<Value> {
    route(pool, workspace).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE plugin_deliveries SET state='expired',error='delivery expired' WHERE runner_id=$1 AND state IN ('pending','retry_wait','running') AND expires_at<=now()").bind(runner).execute(&mut *tx).await?;
    let rows=sqlx::query("SELECT d.*,to_jsonb(e.*)-'routed'-'expires_at' AS event,b.timeout_seconds,b.max_attempts FROM plugin_deliveries d JOIN plugin_bindings b ON b.id=d.binding_id JOIN plugin_instances i ON i.id=d.instance_id JOIN plugin_events e ON e.id=d.event_id WHERE d.runner_id=$1 AND d.workspace_id=$2 AND d.state IN ('pending','retry_wait','running') AND d.available_at<=now() AND (d.locked_until IS NULL OR d.locked_until<now()) AND d.expires_at>now() AND b.enabled AND i.enabled AND EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=i.workspace_id AND m.user_id=i.managed_by AND m.active AND u.active AND m.role='admin') AND NOT EXISTS(SELECT 1 FROM plugin_deliveries earlier JOIN plugin_events previous ON previous.id=earlier.event_id WHERE earlier.binding_id=d.binding_id AND previous.aggregate_id=e.aggregate_id AND previous.aggregate_type=e.aggregate_type AND previous.aggregate_revision<e.aggregate_revision AND earlier.state IN ('pending','retry_wait','running','uncertain')) ORDER BY e.committed_at,d.id LIMIT 8 FOR UPDATE OF d SKIP LOCKED").bind(runner).bind(workspace).fetch_all(&mut *tx).await?;
    let mut items = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let context: Value = row.get("context");
        let mut event: Value = row.get("event");
        if let Some(data) = event["data"].as_object_mut() {
            data.remove("result");
            data.remove("config");
        }
        event["snapshot_ref"] = json!({"event_id":row.get::<Uuid,_>("event_id")});
        event["idempotency_key"] = json!(id);
        let deadline =
            Utc::now() + chrono::Duration::seconds(row.get::<i32, _>("timeout_seconds") as i64);
        let invocation = Invocation {
            id: row.get("invocation_id"),
            operation_id: id,
            instance_id: row.get("instance_id"),
            kind: "event".into(),
            handler: context["handler"]
                .as_str()
                .context("handler missing")?
                .into(),
            input: event,
            deadline,
            expected_revision: context["revision"].as_i64(),
            actor: Actor {
                kind: "event".into(),
                id: None,
                roles: vec![],
            },
            causation_id: Some(row.get("event_id")),
        };
        sqlx::query("UPDATE plugin_deliveries SET state='running',attempt=attempt+1,locked_until=$2,updated_at=now() WHERE id=$1").bind(id).bind(deadline+chrono::Duration::seconds(10)).execute(&mut *tx).await?;
        items.push(json!({"id":id,"action":"invoke","params":{"invocation":invocation}}));
    }
    let operations=sqlx::query("SELECT * FROM plugin_operations WHERE runner_id=$1 AND workspace_id=$2 AND state IN ('pending','retry_wait','running') AND available_at<=now() AND expires_at>now() AND (locked_until IS NULL OR locked_until<now()) ORDER BY created_at,id LIMIT 8 FOR UPDATE SKIP LOCKED").bind(runner).bind(workspace).fetch_all(&mut *tx).await?;
    for row in operations {
        let id: Uuid = row.get("id");
        let request: Value = row.get("request");
        sqlx::query("UPDATE plugin_operations SET state='running',attempt=attempt+1,locked_until=LEAST(expires_at,now()+interval '60 seconds'),updated_at=now() WHERE id=$1").bind(id).execute(&mut *tx).await?;
        items.push(json!({"id":id,"action":request["action"],"params":request["params"]}));
    }
    tx.commit().await?;
    let bindings:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b.*)-'workspace_id'-'created_at' FROM plugin_bindings b WHERE runner_id=$1 AND workspace_id=$2 AND kind='hook' ORDER BY position,id").bind(runner).bind(workspace).fetch_all(pool).await?;
    let cancellations:Vec<String>=sqlx::query_scalar("SELECT request#>>'{params,invocation,id}' FROM plugin_operations WHERE runner_id=$1 AND workspace_id=$2 AND state='cancelled' AND request#>>'{params,invocation,id}' IS NOT NULL ORDER BY updated_at DESC LIMIT 100").bind(runner).bind(workspace).fetch_all(pool).await?;
    let revoked:Vec<Uuid>=sqlx::query_scalar("SELECT i.id FROM plugin_instances i WHERE i.runner_id=$1 AND i.workspace_id=$2 AND i.enabled AND i.managed_by IS NOT NULL AND NOT EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.user_id=i.managed_by AND m.workspace_id=i.workspace_id AND m.active AND m.role='admin' AND u.active)")
        .bind(runner).bind(workspace).fetch_all(pool).await?;
    Ok(
        json!({"items":items,"bindings":bindings,"cancellations":cancellations,"revoked_instances":revoked}),
    )
}
pub async fn acknowledge(
    pool: &PgPool,
    workspace: Uuid,
    runner: Uuid,
    value: &Value,
) -> Result<()> {
    let id: Uuid = value["id"]
        .as_str()
        .context("operation ID required")?
        .parse()?;
    let state = value["state"].as_str().context("state required")?;
    ensure!(
        ["succeeded", "failed", "uncertain", "cancelled"].contains(&state),
        "invalid acknowledgement"
    );
    let mut tx = pool.begin().await?;
    let operation=sqlx::query("SELECT kind,request FROM plugin_operations WHERE id=$1 AND runner_id=$2 AND workspace_id=$3 FOR UPDATE").bind(id).bind(runner).bind(workspace).fetch_optional(&mut *tx).await?;
    if state == "succeeded"
        && let Some(operation) = &operation
        && operation.get::<String, _>("kind") == "reprocess_result"
    {
        let request: Value = operation.get("request");
        let run: Uuid = serde_json::from_value(request["params"]["run_id"].clone())?;
        let current=sqlx::query("SELECT state,execution_snapshot,parent_run_id,node_id FROM runs WHERE id=$1 AND workspace_id=$2 AND runner_id=$3 FOR UPDATE").bind(run).bind(workspace).bind(runner).fetch_one(&mut *tx).await?;
        ensure!(
            current.get::<String, _>("state") == "failed"
                && current
                    .get::<Option<Value>, _>("execution_snapshot")
                    .is_some_and(|s| s["failure_phase"] == "postprocess"),
            "result processing state changed"
        );
        let result = value["result"]["result"].clone();
        sqlx::query("UPDATE runs SET state='succeeded',reason=NULL,result=$2,execution_snapshot=(execution_snapshot-'failure_phase')||$3 WHERE id=$1").bind(run).bind(&result).bind(json!({"effective_result_ref":value["result"]["result_ref"]})).execute(&mut *tx).await?;
        sqlx::query("SELECT plugin_append_event($1,'result.published','run',$2,$3,$4)")
            .bind(workspace)
            .bind(run.to_string())
            .bind(json!({"run_id":run,"result":result,"operation_id":id}))
            .bind(runner)
            .execute(&mut *tx)
            .await?;
        if let Some(parent) = current.get::<Option<Uuid>, _>("parent_run_id") {
            let node: String = current.get("node_id");
            let config: Value = sqlx::query_scalar(
                "SELECT config FROM runs WHERE id=$1 AND workspace_id=$2 FOR UPDATE",
            )
            .bind(parent)
            .bind(workspace)
            .fetch_one(&mut *tx)
            .await?;
            let task: wakeit_core::config::TaskConfig = serde_json::from_value(config)?;
            let mut affected = std::collections::HashSet::from([node.clone()]);
            if let Some(dag) = task.dag {
                loop {
                    let before = affected.len();
                    for n in &dag.nodes {
                        if n.pre.iter().any(|p| affected.contains(p)) {
                            affected.insert(n.id.clone());
                        }
                    }
                    if affected.len() == before {
                        break;
                    }
                }
            }
            sqlx::query("UPDATE dag_run_nodes SET state='succeeded',reason=NULL WHERE parent_run_id=$1 AND node_id=$2 AND execution_id=$3").bind(parent).bind(&node).bind(run).execute(&mut *tx).await?;
            for descendant in affected.into_iter().filter(|n| n != &node) {
                sqlx::query("UPDATE dag_run_nodes SET state='waiting',execution_id=NULL,reason=NULL,finished_at=NULL,stop_cause=NULL WHERE parent_run_id=$1 AND node_id=$2 AND state='skipped' AND stop_cause='dependency'").bind(parent).bind(descendant).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE runs SET state='queued',reason=NULL,result=NULL,finished_at=NULL,recovery_generation=recovery_generation+1,window_started_at=NULL WHERE id=$1").bind(parent).execute(&mut *tx).await?;
        }
    }
    let n=sqlx::query("UPDATE plugin_operations SET state=$4,result=$5,error=$6,updated_at=now(),revision=revision+1,locked_until=NULL WHERE id=$1 AND runner_id=$2 AND workspace_id=$3 AND state IN ('running','pending','retry_wait')").bind(id).bind(runner).bind(workspace).bind(state).bind(value.get("result")).bind(value["error"].as_str()).execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        sqlx::query("UPDATE plugin_deliveries d SET state=CASE WHEN $4='failed' AND $7 AND d.attempt<b.max_attempts THEN 'retry_wait' ELSE $4 END,available_at=now()+make_interval(secs=>LEAST(3600,GREATEST(1,COALESCE($8,LEAST(300,power(2,d.attempt))::double precision)))),result=$5,error=$6,locked_until=NULL,updated_at=now() FROM plugin_bindings b WHERE d.binding_id=b.id AND d.id=$1 AND d.runner_id=$2 AND d.workspace_id=$3 AND d.state='running'").bind(id).bind(runner).bind(workspace).bind(state).bind(value.get("result")).bind(value["error"].as_str()).bind(value["retryable"]==true).bind(value["retry_after"].as_f64()).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
#[expect(
    clippy::too_many_arguments,
    reason = "Durable operation identity matches the wire envelope"
)]
pub async fn enqueue(
    pool: &PgPool,
    workspace: Uuid,
    runner: Uuid,
    instance: Option<Uuid>,
    actor: Uuid,
    key: &str,
    kind: &str,
    request: Value,
    expires: DateTime<Utc>,
) -> Result<Uuid> {
    let proposed_id = if kind == "action" {
        request["params"]["invocation"]["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(Uuid::new_v4)
    } else {
        Uuid::new_v4()
    };
    let id:Uuid=sqlx::query_scalar("INSERT INTO plugin_operations(id,workspace_id,runner_id,instance_id,kind,request_key,actor_id,request,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(workspace_id,request_key) DO UPDATE SET request_key=excluded.request_key RETURNING id").bind(proposed_id).bind(workspace).bind(runner).bind(instance).bind(kind).bind(key).bind(actor).bind(request).bind(expires).fetch_one(pool).await?;
    Ok(id)
}
#[derive(Debug)]
pub(crate) struct GatePending;
impl std::fmt::Display for GatePending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("plugin decision is pending")
    }
}
impl std::error::Error for GatePending {}
pub async fn gate(
    pool: &PgPool,
    workspace: Uuid,
    actor: Uuid,
    key: &str,
    stage: &str,
    proposal: Value,
) -> Result<Value> {
    gate_inner(pool, workspace, actor, key, stage, proposal, true).await
}
async fn gate_inner(
    pool: &PgPool,
    workspace: Uuid,
    actor: Uuid,
    key: &str,
    stage: &str,
    mut proposal: Value,
    wait: bool,
) -> Result<Value> {
    let bindings=sqlx::query("SELECT b.*,i.revision AS instance_revision FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.workspace_id=$1 AND b.kind='hook' AND b.event=$2 AND b.enabled AND i.enabled AND EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=i.workspace_id AND m.user_id=i.managed_by AND m.active AND u.active AND m.role='admin') ORDER BY b.position,b.id").bind(workspace).bind(stage).fetch_all(pool).await?;
    for b in bindings {
        if !matches(&b.get::<Value, _>("filter"), &proposal) {
            continue;
        }
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(&proposal)?));
        let call_key = format!(
            "hook:{key}:{stage}:{}:{}:{}:{digest}",
            b.get::<Uuid, _>("id"),
            b.get::<i64, _>("revision"),
            b.get::<i64, _>("instance_revision")
        );
        let deadline =
            Utc::now() + chrono::Duration::seconds(b.get::<i32, _>("timeout_seconds") as i64);
        let root:Option<Uuid>=sqlx::query_scalar("SELECT id FROM plugin_operations WHERE workspace_id=$1 AND request_key=$2 AND kind='business'").bind(workspace).bind(format!("business:{actor}:{key}")).fetch_optional(pool).await?;
        let invocation = Invocation {
            id: Uuid::new_v4(),
            operation_id: root.unwrap_or_else(Uuid::new_v4),
            instance_id: b.get("instance_id"),
            kind: "hook".into(),
            handler: b.get("handler"),
            input: proposal.clone(),
            deadline,
            expected_revision: Some(b.get("instance_revision")),
            actor: Actor {
                kind: "user".into(),
                id: Some(actor.to_string()),
                roles: vec![],
            },
            causation_id: None,
        };
        let id = enqueue(
            pool,
            workspace,
            b.get("runner_id"),
            Some(b.get("instance_id")),
            actor,
            &call_key,
            "hook",
            json!({"action":"invoke","params":{"invocation":invocation}}),
            deadline,
        )
        .await?;
        if let Some(root) = root {
            sqlx::query("INSERT INTO plugin_hook_calls(id,workspace_id,operation_id,binding_id,input_digest,binding_revision,instance_revision,proposal) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING").bind(id).bind(workspace).bind(root).bind(b.get::<Uuid,_>("id")).bind(&digest).bind(b.get::<i64,_>("revision")).bind(b.get::<i64,_>("instance_revision")).bind(&proposal).execute(pool).await?;
        }
        let online:bool=sqlx::query_scalar("SELECT (NOT revoked AND last_seen>now()-interval '60 seconds') IS TRUE FROM runners WHERE id=$1 AND workspace_id=$2").bind(b.get::<Uuid,_>("runner_id")).bind(workspace).fetch_one(pool).await?;
        if !online {
            match b.get::<String, _>("failure_policy").as_str() {
                "continue" => {
                    sqlx::query("UPDATE plugin_operations SET state='skipped',error='Hook device unavailable; configured to skip',updated_at=now() WHERE id=$1 AND state='pending'").bind(id).execute(pool).await?;
                    continue;
                }
                "reject" => {
                    sqlx::query("UPDATE plugin_operations SET state='rejected',error='Hook device unavailable',updated_at=now() WHERE id=$1 AND state='pending'").bind(id).execute(pool).await?;
                    anyhow::bail!("Hook device is offline");
                }
                _ => {}
            }
        }
        loop {
            let row = sqlx::query(
                "SELECT state,result,error,expires_at FROM plugin_operations WHERE id=$1 AND workspace_id=$2",
            )
            .bind(id)
            .bind(workspace)
            .fetch_one(pool)
            .await?;
            let state: String = row.get("state");
            if state == "skipped" && b.get::<String, _>("failure_policy") == "continue" {
                break;
            }
            if state == "succeeded" {
                let decision: Decision = serde_json::from_value(row.get("result"))?;
                wakeit_core::extensions::apply_decision(stage, &mut proposal, decision)?;
                break;
            }
            if ["failed", "cancelled", "uncertain", "rejected"].contains(&state.as_str())
                || Utc::now() >= row.get::<DateTime<Utc>, _>("expires_at")
            {
                if b.get::<String, _>("failure_policy") == "continue" {
                    break;
                }
                anyhow::bail!(
                    "hook_unavailable: {}",
                    row.get::<Option<String>, _>("error")
                        .unwrap_or_else(|| "extension deadline exceeded".into())
                );
            }
            if !wait {
                return Err(GatePending.into());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.id=$1 AND b.revision=$2 AND i.revision=$3 AND b.enabled AND i.enabled)").bind(b.get::<Uuid,_>("id")).bind(b.get::<i64,_>("revision")).bind(b.get::<i64,_>("instance_revision")).fetch_one(pool).await?;
        ensure!(valid, "hook binding changed during operation");
    }
    Ok(proposal)
}

/// Prepare an already reserved assignment without holding business or outbox locks.
/// The scheduler's unique occurrence and existing claim path retain admission authority.
pub async fn prepare_assignment(
    pool: &PgPool,
    workspace: Uuid,
    key: &str,
    payload: &Value,
) -> Result<Value> {
    let Some(run) = payload["run_id"]
        .as_str()
        .and_then(|v| v.parse::<Uuid>().ok())
    else {
        return Ok(payload.clone());
    };
    if payload.get("task").is_none() {
        return Ok(payload.clone());
    }
    let stages=sqlx::query("SELECT DISTINCT b.event,i.managed_by FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.workspace_id=$1 AND b.kind='hook' AND b.enabled AND i.enabled AND b.event IN ('run.create.prepare','run.retry.prepare','schedule.fire.prepare','dag.node.dispatch.prepare')").bind(workspace).fetch_all(pool).await?;
    if stages.is_empty() {
        return Ok(payload.clone());
    }
    let row=sqlx::query("SELECT state,claim_id,parent_run_id,source,retry_of,config,task_id FROM runs WHERE workspace_id=$1 AND id=$2").bind(workspace).bind(run).fetch_optional(pool).await?.context("assignment run missing")?;
    if row.get::<String, _>("state") != "queued" || row.get::<Option<Uuid>, _>("claim_id").is_some()
    {
        return Ok(payload.clone());
    }
    let source: String = row.get("source");
    let parent: Option<Uuid> = row.get("parent_run_id");
    let mut proposal = json!({"run_id":run,"task_id":row.get::<String,_>("task_id"),"runner_id":payload["runner_id"],"prompt":payload["task"]["prompt"],"parameters":payload["task"]["parameters"]});
    let order = if parent.is_some() {
        vec!["dag.node.dispatch.prepare"]
    } else if source == "schedule" {
        vec!["schedule.fire.prepare", "run.create.prepare"]
    } else if row.get::<Option<Uuid>, _>("retry_of").is_some() {
        vec!["run.retry.prepare", "run.create.prepare"]
    } else {
        vec!["run.create.prepare"]
    };
    for stage in order {
        if let Some(binding) = stages.iter().find(|b| b.get::<String, _>("event") == stage) {
            proposal = gate_inner(
                pool,
                workspace,
                binding.get("managed_by"),
                key,
                stage,
                proposal,
                false,
            )
            .await?;
        }
    }
    let mut effective: crate::protocol::Assignment = serde_json::from_value(payload.clone())?;
    effective.task.prompt = serde_json::from_value(proposal["prompt"].clone())?;
    effective.task.parameters = serde_json::from_value(proposal["parameters"].clone())?;
    effective.task.validate()?;
    let mut tx = pool.begin().await?;
    let current = sqlx::query(
        "SELECT config,state,claim_id FROM runs WHERE id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(run)
    .bind(workspace)
    .fetch_one(&mut *tx)
    .await?;
    ensure!(
        current.get::<Value, _>("config") == row.get::<Value, _>("config"),
        "assignment changed during Hook decision"
    );
    ensure!(
        current.get::<String, _>("state") == "queued"
            && current.get::<Option<Uuid>, _>("claim_id").is_none(),
        "assignment is no longer available"
    );
    sqlx::query("UPDATE runs SET execution_snapshot=COALESCE(execution_snapshot,'{}'::jsonb)||$2 WHERE id=$1").bind(run).bind(json!({"plugin_input":proposal})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(serde_json::to_value(effective)?)
}

pub async fn maintain(pool: &PgPool) -> Result<()> {
    sqlx::query("UPDATE plugin_events e SET expires_at=LEAST(e.expires_at,e.committed_at+make_interval(days=>(s.data->>'run_retention_days')::integer)) FROM settings s WHERE e.workspace_id=s.workspace_id AND e.expires_at>e.committed_at+make_interval(days=>(s.data->>'run_retention_days')::integer)").execute(pool).await?;
    sqlx::query("DELETE FROM plugin_operations o WHERE updated_at<now()-interval '90 days' AND state NOT IN ('pending','running','retry_wait') AND NOT EXISTS(SELECT 1 FROM plugin_operation_tokens t WHERE t.operation_id=o.id AND t.expires_at>now())").execute(pool).await?;

    let mut tx = pool.begin().await?;
    let devices=sqlx::query("SELECT r.id,r.workspace_id,r.name,(NOT r.revoked AND r.last_seen>now()-interval '60 seconds') IS TRUE AS online,p.online AS previous FROM runners r LEFT JOIN plugin_device_presence p ON p.runner_id=r.id WHERE p.runner_id IS NULL OR p.online IS DISTINCT FROM ((NOT r.revoked AND r.last_seen>now()-interval '60 seconds') IS TRUE) FOR UPDATE OF r SKIP LOCKED").fetch_all(&mut *tx).await?;
    for device in devices {
        let online: bool = device.get("online");
        let runner: Uuid = device.get("id");
        if device.get::<Option<bool>, _>("previous").is_some() {
            sqlx::query("SELECT plugin_append_event($1,$2,'device',$3,$4,$5)")
                .bind(device.get::<Uuid, _>("workspace_id"))
                .bind(if online {
                    "device.online"
                } else {
                    "device.offline"
                })
                .bind(runner.to_string())
                .bind(json!({"id":runner,"name":device.get::<String,_>("name")}))
                .bind(runner)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("INSERT INTO plugin_device_presence(runner_id,online) VALUES($1,$2) ON CONFLICT(runner_id) DO UPDATE SET online=excluded.online,updated_at=now()").bind(runner).bind(online).execute(&mut *tx).await?;
    }
    tx.commit().await?;

    sqlx::query("UPDATE plugin_operations SET state='expired',error='operation expired',revision=revision+1,updated_at=now() WHERE expires_at<=now() AND state IN ('pending','running','retry_wait')").execute(pool).await?;
    sqlx::query("UPDATE plugin_deliveries SET state='expired',error='delivery expired',updated_at=now() WHERE expires_at<=now() AND state IN ('pending','running','retry_wait','uncertain')").execute(pool).await?;
    sqlx::query("DELETE FROM plugin_api_tokens WHERE expires_at<now()")
        .execute(pool)
        .await?;
    sqlx::query(
        "UPDATE plugin_auth_flows SET status='expired' WHERE status='pending' AND expires_at<now()",
    )
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM plugin_events e WHERE expires_at<now() AND NOT EXISTS(SELECT 1 FROM plugin_deliveries d WHERE d.event_id=e.id AND d.state IN ('pending','running','retry_wait'))").execute(pool).await?;
    Ok(())
}

pub async fn business_operations(s: &crate::state::App) -> Result<()> {
    let mut tx = s.pool.begin().await?;
    let rows=sqlx::query("SELECT * FROM plugin_operations WHERE kind='business' AND state IN ('pending','running') AND expires_at>now() AND (locked_until IS NULL OR locked_until<now()) ORDER BY created_at,id LIMIT 4 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for row in &rows {
        sqlx::query("UPDATE plugin_operations SET state='running',locked_until=LEAST(expires_at,now()+interval '30 seconds'),updated_at=now() WHERE id=$1").bind(row.get::<Uuid,_>("id")).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    for row in rows {
        let s = s.clone();
        tokio::spawn(async move {
            let id: Uuid = row.get("id");
            let workspace: Uuid = row.get("workspace_id");
            let actor: Uuid = row.get("actor_id");
            let request: Value = row.get("request");
            let result:Result<(u16,Value)>=async{
                let secret=crate::auth::credentials::secret();let hash=crate::auth::credentials::hash(&secret);
                sqlx::query("INSERT INTO plugin_operation_tokens(hash,operation_id,actor_id,workspace_id,expires_at) VALUES($1,$2,$3,$4,$5)").bind(&hash).bind(id).bind(actor).bind(workspace).bind(row.get::<DateTime<Utc>,_>("expires_at")).execute(&s.pool).await?;
                let listen=s.config.listen.replace("0.0.0.0","127.0.0.1").replace("[::]","[::1]");
                let path=request["path"].as_str().context("operation path missing")?;
                let client=reqwest::Client::new();
                let response=client.request(request["method"].as_str().context("operation method missing")?.parse()?,format!("http://{listen}/api/v1/workspaces/{workspace}{path}")).bearer_auth(secret).header("Idempotency-Key",request["idempotency_key"].as_str().context("request key missing")?).json(&request["body"]).timeout(std::time::Duration::from_secs(900)).send();
                tokio::pin!(response);
                let mut renewal=tokio::time::interval(std::time::Duration::from_secs(10));
                let response=loop{tokio::select!{response=&mut response=>break response,_=renewal.tick()=>{sqlx::query("UPDATE plugin_operations SET locked_until=LEAST(expires_at,now()+interval '30 seconds') WHERE id=$1 AND state='running'").bind(id).execute(&s.pool).await?;}}};
                sqlx::query("DELETE FROM plugin_operation_tokens WHERE hash=$1").bind(hash).execute(&s.pool).await?;
                let response=response?;let status=response.status().as_u16();Ok((status,response.json().await?))
            }.await;
            match result {
                Ok((status, result)) => {
                    let state = if (200..300).contains(&status) {
                        "committed"
                    } else if status == 409 {
                        "conflict"
                    } else {
                        "rejected"
                    };
                    let _=sqlx::query("UPDATE plugin_operations SET state=$2,result=$3,error=$4,revision=revision+1,updated_at=now(),locked_until=NULL WHERE id=$1 AND state='running'").bind(id).bind(state).bind(&result).bind(result["error"].as_str()).execute(&s.pool).await;
                }
                Err(error) => {
                    let _=sqlx::query("UPDATE plugin_operations SET state='failed',error=$2,updated_at=now(),locked_until=NULL WHERE id=$1 AND state='running'").bind(id).bind(error.to_string()).execute(&s.pool).await;
                }
            }
        });
    }
    Ok(())
}
