//! Workspace plugin management and authenticated device transport. Secrets use live RPC only.
use super::{
    access::{device, user, workspace_header},
    http::{ApiJson, Error, Path, Result, request_key},
};
use crate::{state::App, store::plugins as store};
use anyhow::Context;
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
use wakeit_core::extensions::{Actor, Binding, Invocation};

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Control {
    pub action: String,
    #[serde(default)]
    pub params: Value,
}
pub(crate) async fn catalog(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let cached:Option<Value>=sqlx::query_scalar("SELECT catalog||jsonb_build_object('updated_at',updated_at) FROM plugin_catalogs WHERE workspace_id=$1 AND runner_id=$2").bind(workspace).bind(id).fetch_optional(&s.pool).await?;
    Ok(Json(cached.unwrap_or_else(
        || json!({"packages":[],"instances":[],"state":"unavailable"}),
    )))
}
pub(crate) fn hook_key(headers: &HeaderMap) -> String {
    headers
        .get("Idempotency-Key")
        .and_then(|s| s.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}
async fn rpc(
    s: &App,
    workspace: Uuid,
    runner: Uuid,
    action: &str,
    payload: Value,
) -> Result<Value> {
    use futures::StreamExt;
    let runtime:Option<Uuid>=sqlx::query_scalar("SELECT runtime_id FROM runners WHERE id=$1 AND workspace_id=$2 AND NOT revoked AND last_seen>now()-interval '60 seconds'").bind(runner).bind(workspace).fetch_optional(&s.pool).await?.flatten();
    let runtime =
        runtime.ok_or_else(|| Error(StatusCode::SERVICE_UNAVAILABLE, "Runner offline".into()))?;
    let request = crate::protocol::QueryRequest {
        version: 1,
        id: Uuid::new_v4(),
        runtime_id: runtime,
        action: action.into(),
        payload,
    };
    let reply = format!(
        "{}.plugins.{}",
        crate::protocol::reply_prefix(runner),
        request.id.simple()
    );
    let mut sub = s
        .client
        .subscribe(reply.clone())
        .await
        .map_err(anyhow::Error::from)?;
    s.client
        .publish_with_reply(
            format!("wakeit.v1.runner.{runner}.plugins"),
            reply,
            serde_json::to_vec(&request)?.into(),
        )
        .await
        .map_err(anyhow::Error::from)?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(35), async {
        while let Some(message) = sub.next().await {
            if message.status == Some(async_nats::StatusCode::NO_RESPONDERS) {
                return Err(Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Runner does not support extensions".into(),
                ));
            }
            let reply: crate::protocol::QueryReply = serde_json::from_slice(&message.payload)?;
            if reply.id != request.id || reply.runtime_id != runtime {
                continue;
            }
            if let Some(e) = reply.error {
                return Err(Error::bad(format!("{}: {}", e.code, e.message)));
            }
            return Ok(reply.data.unwrap_or(Value::Null));
        }
        Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Runner disconnected".into(),
        ))
    })
    .await;
    let _ = sub.unsubscribe().await;
    result.map_err(|_| {
        Error(
            StatusCode::GATEWAY_TIMEOUT,
            "Plugin operation timed out; check operation status before retrying".into(),
        )
    })?
}
pub(crate) async fn control(
    State(s): State<App>,
    h: HeaderMap,
    Path(runner): Path<Uuid>,
    ApiJson(body): ApiJson<Control>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let read = ["catalog", "activity", "operation"].contains(&body.action.as_str());
    let action_call = ["invoke", "interaction", "cancel"].contains(&body.action.as_str());
    let actor = user(
        &s,
        &h,
        if read {
            "viewer"
        } else if action_call {
            "operator"
        } else {
            "admin"
        },
        !read,
    )
    .await?;
    if actor.role != "admin" && body.action == "cancel" {
        let call: Uuid = serde_json::from_value(body.params["invocation_id"].clone())
            .map_err(|_| Error::bad("invocation ID required"))?;
        let owned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_operations WHERE id=$1 AND workspace_id=$2 AND runner_id=$3 AND actor_id=$4 AND kind='action')").bind(call).bind(workspace).bind(runner).bind(actor.id).fetch_one(&s.pool).await?;
        if !owned {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "invocation belongs to another actor".into(),
            ));
        }
    }
    if !read && body.action == "invoke" {
        let mut invocation: Invocation = serde_json::from_value(body.params["invocation"].clone())
            .map_err(|e| Error::bad(e.to_string()))?;
        if invocation.kind != "action" {
            return Err(Error::bad(
                "Only declared actions can be invoked through WebUI",
            ));
        }
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_instances WHERE id=$1 AND runner_id=$2 AND workspace_id=$3 AND enabled)").bind(invocation.instance_id).bind(runner).bind(workspace).fetch_one(&s.pool).await?;
        if !valid {
            return Err(Error::missing());
        }
        invocation.actor = Actor {
            kind: "user".into(),
            id: Some(actor.id.to_string()),
            roles: vec![actor.role.clone()],
        };
        let id = store::enqueue(
            &s.pool,
            workspace,
            runner,
            Some(invocation.instance_id),
            actor.id,
            &request_key(&h)?,
            "action",
            json!({"action":"invoke","params":{"invocation":invocation}}),
            Utc::now() + chrono::Duration::hours(1),
        )
        .await?;
        return Ok(Json(
            json!({"id":id,"state":"pending","revision":1,"result":null,"error":null}),
        ));
    }
    let params = body.params.clone();
    let mut value = rpc(&s, workspace, runner, &body.action, body.params).await?;
    if actor.role != "admin"
        && body.action == "catalog"
        && let Some(instances) = value["instances"].as_array_mut()
    {
        for instance in instances {
            if let Some(object) = instance.as_object_mut() {
                object.remove("config");
            }
        }
    }
    if actor.role == "viewer"
        && body.action == "activity"
        && let Some(object) = value.as_object_mut()
    {
        object.remove("logs");
        object.remove("interactions");
    }
    if body.action == "remove" {
        let id: Uuid = serde_json::from_value(params["instance_id"].clone())
            .map_err(|_| Error::bad("instance required"))?;
        sqlx::query("UPDATE plugin_bindings SET enabled=false,revision=revision+1 WHERE instance_id=$1 AND workspace_id=$2").bind(id).bind(workspace).execute(&s.pool).await?;
        sqlx::query("UPDATE plugin_instances SET state='removed',enabled=false WHERE id=$1 AND workspace_id=$2").bind(id).bind(workspace).execute(&s.pool).await?;
    }
    if body.action == "authorize" {
        let id: Uuid = params["instance_id"]
            .as_str()
            .ok_or_else(|| Error::bad("instance required"))?
            .parse()
            .map_err(|_| Error::bad("invalid instance"))?;
        sqlx::query("UPDATE plugin_instances SET grants=$4 WHERE id=$1 AND runner_id=$2 AND workspace_id=$3").bind(id).bind(runner).bind(workspace).bind(&params["grants"]).execute(&s.pool).await?;
        sqlx::query("UPDATE plugin_bindings SET enabled=false,revision=revision+1 WHERE instance_id=$1 AND workspace_id=$2 AND NOT ($3::jsonb ? CASE WHEN kind='hook' THEN 'hooks:'||event ELSE 'events:read' END)").bind(id).bind(workspace).bind(&params["grants"]).execute(&s.pool).await?;
    }
    if body.action == "create" {
        let instance: wakeit_core::extensions::Instance = serde_json::from_value(value.clone())?;
        sqlx::query("INSERT INTO plugin_instances(id,workspace_id,runner_id,source,extension,name,version,revision,grants,declaration,managed_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT(id) DO NOTHING")
            .bind(instance.id).bind(workspace).bind(runner).bind(instance.source).bind(instance.extension).bind(instance.name).bind(instance.version).bind(instance.revision).bind(json!(instance.grants)).bind(json!(instance.declaration)).bind(actor.id).execute(&s.pool).await?;
    }
    Ok(Json(value))
}
#[derive(Deserialize, Default, schemars::JsonSchema)]
pub(crate) struct List {
    pub instance: Option<Uuid>,
    pub event: Option<String>,
    pub before: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}
pub(crate) async fn bindings(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<List>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let items:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(b.*)-'workspace_id'-'created_at' FROM plugin_bindings b WHERE workspace_id=$1 AND ($2::uuid IS NULL OR instance_id=$2) ORDER BY position,id").bind(workspace).bind(q.instance).fetch_all(&s.pool).await?;
    Ok(Json(json!({"items":items})))
}
pub(crate) async fn bind(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Binding>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "admin", true).await?;
    body.validate()
        .map_err(|error| Error::bad(error.to_string()))?;
    let instance=sqlx::query("SELECT declaration,grants FROM plugin_instances WHERE workspace_id=$1 AND runner_id=$2 AND id=$3").bind(workspace).bind(body.runner_id).bind(body.instance_id).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    let declaration: Value = instance.get("declaration");
    let grants: Vec<String> = serde_json::from_value(instance.get("grants"))?;
    let permission = if body.kind == "hook" {
        format!("hooks:{}", body.event)
    } else {
        "events:read".into()
    };
    if !grants.contains(&permission) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "handler capability is not granted".into(),
        ));
    }
    let section = if body.kind == "hook" {
        "hooks"
    } else if body.kind == "event" {
        "handlers"
    } else {
        return Err(Error::bad("invalid binding kind"));
    };
    let resource = declaration[section]
        .get(&body.handler)
        .ok_or_else(|| Error::bad("handler not declared"))?;
    if body.kind == "hook" && resource["stage"] != body.event {
        return Err(Error::bad("hook stage mismatch"));
    }
    if body.kind == "event"
        && !resource["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .filter_map(Value::as_str)
                .any(|pattern| wakeit_core::extensions::event_matches(pattern, &body.event))
        })
    {
        return Err(Error::bad("event not declared by handler"));
    }
    let row:Option<Value>=sqlx::query_scalar("INSERT INTO plugin_bindings(id,workspace_id,instance_id,runner_id,kind,event,handler,enabled,filter,position,failure_policy,timeout_seconds,max_attempts,expires_seconds) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT(id) DO UPDATE SET enabled=excluded.enabled,filter=excluded.filter,position=excluded.position,failure_policy=excluded.failure_policy,timeout_seconds=excluded.timeout_seconds,max_attempts=excluded.max_attempts,expires_seconds=excluded.expires_seconds,revision=plugin_bindings.revision+1 WHERE plugin_bindings.workspace_id=$2 AND plugin_bindings.revision=$15 RETURNING to_jsonb(plugin_bindings.*)-'workspace_id'-'created_at'")
        .bind(body.id).bind(workspace).bind(body.instance_id).bind(body.runner_id).bind(body.kind).bind(body.event).bind(body.handler).bind(body.enabled).bind(json!(body.filter)).bind(body.position).bind(serde_json::to_value(body.failure_policy)?.as_str().unwrap_or("reject")).bind(body.timeout_seconds as i32).bind(body.max_attempts as i32).bind(body.expires_seconds as i32).bind(body.revision).fetch_optional(&s.pool).await?;
    Ok(Json(row.ok_or_else(|| {
        Error::conflict("binding revision conflict")
    })?))
}
pub(crate) async fn events(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<List>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let items:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e.*)-'routed'-'data'||jsonb_build_object('data',data-'result'-'config') FROM plugin_events e WHERE workspace_id=$1 AND ($2::text IS NULL OR event_type=$2) AND ($3::timestamptz IS NULL OR committed_at<$3) ORDER BY committed_at DESC,id DESC LIMIT $4").bind(workspace).bind(q.event).bind(q.before).bind(q.limit.unwrap_or(50).clamp(1,200)).fetch_all(&s.pool).await?;
    Ok(Json(json!({"items":items})))
}
pub(crate) async fn event(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let item:Value=sqlx::query_scalar("SELECT to_jsonb(e.*)-'routed' FROM plugin_events e WHERE id=$1 AND workspace_id=$2 AND expires_at>now()").bind(id).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    Ok(Json(item))
}
fn operation_receipt(mut value: Value) -> Value {
    if let Some(message) = value["error"].as_str().map(str::to_owned) {
        let code = value["result"]["code"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                value["state"]
                    .as_str()
                    .unwrap_or("operation_failed")
                    .to_owned()
            });
        value["error"] = json!({"code":code,"message":message,"retryable":false,"paths":[]});
    }
    value
}
pub(crate) async fn operations(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<List>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(o.*)-'request' FROM plugin_operations o WHERE workspace_id=$1 AND ($2::uuid IS NULL OR instance_id=$2) ORDER BY created_at DESC,id DESC LIMIT 100").bind(workspace).bind(q.instance).fetch_all(&s.pool).await?;
    Ok(Json(
        json!({"items":rows.into_iter().map(operation_receipt).collect::<Vec<_>>()}),
    ))
}
pub(crate) async fn operation(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let item: Value = sqlx::query_scalar(
        "SELECT to_jsonb(o.*)-'request' FROM plugin_operations o WHERE id=$1 AND workspace_id=$2",
    )
    .bind(id)
    .bind(workspace)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(Error::missing)?;
    Ok(Json(operation_receipt(item)))
}
pub(crate) async fn deliveries(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<List>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let items:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(d.*) FROM plugin_deliveries d WHERE workspace_id=$1 AND ($2::uuid IS NULL OR instance_id=$2) ORDER BY created_at DESC,id DESC LIMIT 100").bind(workspace).bind(q.instance).fetch_all(&s.pool).await?;
    Ok(Json(json!({"items":items})))
}
pub(crate) async fn delivery_action(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Control>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "admin", true).await?;
    match body.action.as_str() {
        "retry" => {
            sqlx::query("UPDATE plugin_deliveries SET state='pending',available_at=now(),locked_until=NULL WHERE id=$1 AND workspace_id=$2 AND state IN ('failed','retry_wait')").bind(id).bind(workspace).execute(&s.pool).await?;
        }
        "discard" | "resolve" => {
            sqlx::query("UPDATE plugin_deliveries SET state=$3,locked_until=NULL WHERE id=$1 AND workspace_id=$2").bind(id).bind(workspace).bind(if body.action=="discard"{"cancelled"}else{"succeeded"}).execute(&s.pool).await?;
        }
        "replay" => {
            let replay:Uuid=sqlx::query_scalar("INSERT INTO plugin_deliveries(workspace_id,event_id,binding_id,instance_id,runner_id,replay_id,expires_at,context) SELECT workspace_id,event_id,binding_id,instance_id,runner_id,gen_random_uuid(),now()+interval '7 days',context FROM plugin_deliveries WHERE id=$1 AND workspace_id=$2 RETURNING id").bind(id).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
            return Ok(Json(json!({"id":replay,"state":"pending"})));
        }
        _ => return Err(Error::bad("unsupported delivery action")),
    }
    Ok(Json(json!({"id":id,"state":"accepted"})))
}
async fn device_workspace(s: &App, h: &HeaderMap) -> Result<(Uuid, Uuid)> {
    let runner = device(s, h).await?;
    let workspace = sqlx::query_scalar("SELECT workspace_id FROM runners WHERE id=$1")
        .bind(runner)
        .fetch_one(&s.pool)
        .await?;
    Ok((runner, workspace))
}
pub(crate) async fn poll(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(mut body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (runner, workspace) = device_workspace(&s, &h).await?;
    let runtime: Uuid = serde_json::from_value(body["runtime_id"].clone())
        .map_err(|_| Error::bad("Runner runtime required"))?;
    let current: bool =
        sqlx::query_scalar("SELECT runtime_id=$2 FROM runners WHERE id=$1 AND NOT revoked")
            .bind(runner)
            .bind(runtime)
            .fetch_one(&s.pool)
            .await?;
    if !current {
        return Err(Error::conflict("Runner lease changed"));
    }
    let active: Vec<String> =
        serde_json::from_value(body["active_invocations"].clone()).unwrap_or_default();
    if !active.is_empty() {
        sqlx::query("UPDATE plugin_operations SET locked_until=LEAST(expires_at,now()+interval '60 seconds') WHERE runner_id=$1 AND state='running' AND request#>>'{params,invocation,id}'=ANY($2)").bind(runner).bind(&active).execute(&s.pool).await?;
        sqlx::query("UPDATE plugin_deliveries SET locked_until=LEAST(expires_at,now()+interval '60 seconds') WHERE runner_id=$1 AND state='running' AND invocation_id::text=ANY($2)").bind(runner).bind(&active).execute(&s.pool).await?;
    }
    if let Some(instances) = body["instances"].as_array_mut() {
        for instance in instances {
            let Some(id) = instance["id"].as_str().and_then(|v| v.parse::<Uuid>().ok()) else {
                continue;
            };
            sqlx::query("UPDATE plugin_instances SET state=$4,enabled=$5,revision=$6,version=$7,updated_at=now() WHERE id=$1 AND runner_id=$2 AND workspace_id=$3").bind(id).bind(runner).bind(workspace).bind(instance["state"].as_str().unwrap_or("unavailable")).bind(instance["enabled"]==true).bind(instance["revision"].as_i64().unwrap_or(1)).bind(instance["version"].as_str().unwrap_or("unknown")).execute(&s.pool).await?;
            instance.as_object_mut().map(|i| i.remove("config"));
        }
    }
    sqlx::query("INSERT INTO plugin_catalogs(runner_id,workspace_id,catalog) VALUES($1,$2,$3) ON CONFLICT(runner_id) DO UPDATE SET catalog=excluded.catalog,updated_at=now()").bind(runner).bind(workspace).bind(body).execute(&s.pool).await?;
    Ok(Json(store::poll(&s.pool, workspace, runner).await?))
}
pub(crate) async fn ack(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (runner, workspace) = device_workspace(&s, &h).await?;
    let runtime: Uuid = serde_json::from_value(body["runtime_id"].clone())
        .map_err(|_| Error::bad("Runner runtime required"))?;
    let current: bool =
        sqlx::query_scalar("SELECT runtime_id=$2 FROM runners WHERE id=$1 AND NOT revoked")
            .bind(runner)
            .bind(runtime)
            .fetch_one(&s.pool)
            .await?;
    if !current {
        return Err(Error::conflict("Runner lease changed"));
    }
    store::acknowledge(&s.pool, workspace, runner, &body).await?;
    Ok(Json(json!({"ok":true})))
}
fn segment(params: &Value, key: &str) -> anyhow::Result<String> {
    let value = params[key].as_str().context(format!("{key} required"))?;
    anyhow::ensure!(
        !value.is_empty() && !value.contains(['/', '\\', '?', '#']) && value != "..",
        "invalid resource ID"
    );
    Ok(value.into())
}
/// Only declared SDK business methods become HTTP calls; plugins cannot choose arbitrary URLs.
fn business_route(
    method: &str,
    p: &Value,
) -> anyhow::Result<(&'static str, String, Option<Value>)> {
    let resource = || segment(p, "id");
    let node = || segment(p, "node");
    let body = p.get("body").cloned();
    Ok(match method {
        "members.update" => ("PUT", format!("/members/{}", resource()?), body),
        "members.remove" => ("DELETE", format!("/members/{}", resource()?), None),
        "members.invite" => ("POST", "/invites".into(), body),
        "templates.from_task" => ("GET", format!("/tasks/{}/template", resource()?), None),
        "templates.validate" => ("POST", "/task-template-preview".into(), body),
        "runtime.cancel" => (
            "POST",
            format!("/plugins/operations/{}", resource()?),
            Some(json!({"action":"cancel"})),
        ),
        "runtime.operation" => ("GET", format!("/plugins/operations/{}", resource()?), None),
        "agents.capabilities" => ("GET", "/agents".into(), None),
        "executions.events" => ("GET", format!("/runs/{}/events", resource()?), None),
        "workspace.get" => ("GET", "/settings".into(), None),
        "workspace.update" => ("PUT", "/settings".into(), body),
        "members.list" => ("GET", "/members".into(), None),
        "tasks.enable" | "tasks.disable" => ("PUT", format!("/tasks/{}", resource()?), None),
        "tasks.list" => ("GET", "/tasks".into(), None),
        "tasks.get" => ("GET", format!("/tasks/{}", resource()?), None),
        "tasks.validate" => ("POST", "/tasks/validate".into(), body),
        "tasks.create" | "tasks.update" => ("PUT", format!("/tasks/{}", resource()?), body),
        "tasks.trigger" => (
            "POST",
            format!("/tasks/{}/runs", resource()?),
            Some(json!({})),
        ),
        "tasks.archive" | "tasks.restore" => (
            "POST",
            format!(
                "/tasks/{}/{}",
                resource()?,
                method.split('.').next_back().unwrap()
            ),
            body,
        ),
        "templates.list" => ("GET", "/task-templates".into(), None),
        "templates.get" => ("GET", format!("/task-templates/{}", resource()?), None),
        "templates.create" | "templates.update" => {
            ("PUT", format!("/task-templates/{}", resource()?), body)
        }
        "templates.preview"
        | "templates.instantiate"
        | "templates.archive"
        | "templates.restore" => (
            "POST",
            format!(
                "/task-templates/{}/{}",
                resource()?,
                method.split('.').next_back().unwrap()
            ),
            body,
        ),
        "schedule.preview" => ("POST", "/schedule/preview".into(), body),
        "runs.list" => ("GET", "/runs".into(), None),
        "runs.get" => ("GET", format!("/runs/{}", resource()?), None),
        "runs.events" => ("GET", format!("/runs/{}/events", resource()?), None),
        "runs.cancel" | "runs.retry" | "runs.reconcile" => (
            "POST",
            format!(
                "/runs/{}/{}",
                resource()?,
                if method == "runs.reconcile" {
                    "resolve"
                } else {
                    method.split('.').next_back().unwrap()
                }
            ),
            body,
        ),
        "dag.node" => (
            "GET",
            format!("/runs/{}/nodes/{}", resource()?, node()?),
            None,
        ),
        "dag.events" => (
            "GET",
            format!("/runs/{}/nodes/{}/events", resource()?, node()?),
            None,
        ),
        "dag.retry" | "dag.reconcile" => (
            "POST",
            format!(
                "/runs/{}/nodes/{}/{}",
                resource()?,
                node()?,
                if method == "dag.retry" {
                    "retry"
                } else {
                    "resolve"
                }
            ),
            body,
        ),
        "devices.list" => ("GET", "/runners".into(), None),
        "agents.list" => ("GET", "/agents".into(), None),
        "devices.revoke" => (
            "POST",
            format!("/runners/{}/revoke", resource()?),
            Some(json!({})),
        ),
        "models.list" => (
            "POST",
            format!(
                "/runners/{}/agents/{}/models/query",
                segment(p, "runner")?,
                segment(p, "agent")?
            ),
            body,
        ),
        "sessions.list" | "sessions.inspect" => (
            "POST",
            format!(
                "/runners/{}/agents/{}/sessions/{}",
                segment(p, "runner")?,
                segment(p, "agent")?,
                if method == "sessions.list" {
                    "query"
                } else {
                    "inspect"
                }
            ),
            body,
        ),
        "results.reprocess" => (
            "POST",
            format!("/runs/{}/result/retry", resource()?),
            Some(json!({})),
        ),
        "files.preview" => ("POST", format!("/runs/{}/files/preview", resource()?), body),
        "events.list" => ("GET", "/plugins/events".into(), None),
        "events.get" | "results.snapshot" => {
            ("GET", format!("/plugins/events/{}", resource()?), None)
        }
        "plugins.list" => (
            "GET",
            format!("/runners/{}/plugins", segment(p, "runner")?),
            None,
        ),
        "plugins.control" => (
            "POST",
            format!("/runners/{}/plugins/control", segment(p, "runner")?),
            body,
        ),
        "events.bindings" => ("GET", "/plugins/bindings".into(), None),
        "events.subscribe" | "hooks.bind" => ("POST", "/plugins/bindings".into(), body),
        "events.deliveries" => ("GET", "/plugins/deliveries".into(), None),
        "events.replay" => ("POST", "/plugins/replay".into(), body),
        "events.retry" => (
            "POST",
            format!("/plugins/deliveries/{}", resource()?),
            Some(json!({"action":method.split('.').next_back().unwrap()})),
        ),
        _ => anyhow::bail!("unsupported host API method: {method}"),
    })
}
pub(crate) async fn host_api(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (runner, workspace) = device_workspace(&s, &h).await?;
    let instance: Uuid = body["instance_id"]
        .as_str()
        .ok_or_else(|| Error::bad("instance required"))?
        .parse()
        .map_err(|_| Error::bad("invalid instance"))?;
    let row=sqlx::query("SELECT i.* FROM plugin_instances i JOIN workspace_members m ON m.workspace_id=i.workspace_id AND m.user_id=i.managed_by WHERE i.id=$1 AND i.runner_id=$2 AND i.workspace_id=$3 AND i.enabled AND m.active AND m.role='admin'").bind(instance).bind(runner).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(||Error(StatusCode::FORBIDDEN,"extension permission revoked".into()))?;
    let method = body["method"]
        .as_str()
        .ok_or_else(|| Error::bad("method required"))?;
    let grants: Vec<String> = serde_json::from_value(row.get("grants"))?;
    if method == "files.device_read" {
        if !grants.contains(&"files:read".into()) {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "file access not granted".into(),
            ));
        }
        let target: Uuid = serde_json::from_value(body["params"]["runner"].clone())
            .map_err(|_| Error::bad("source Runner required"))?;
        let mut request = body["params"].clone();
        if let Some(params) = request.as_object_mut() {
            params.remove("invocation_id");
        }
        return Ok(Json(
            rpc(&s, workspace, target, "file_read", request).await?,
        ));
    }
    let (verb, mut path, mut payload) =
        business_route(method, &body["params"]).map_err(|e| Error::bad(e.to_string()))?;
    let namespace = method.split('.').next().unwrap_or("");
    let required = format!(
        "{namespace}:{}",
        if verb == "GET"
            || ["validate", "preview", "list", "inspect"]
                .contains(&method.rsplit('.').next().unwrap_or(""))
        {
            "read"
        } else {
            "write"
        }
    );
    if !grants.contains(&method.into()) && !grants.contains(&required) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "extension capability not granted".into(),
        ));
    }
    if let Some(query) = body["params"]["query"].as_object() {
        let mut url = reqwest::Url::parse("http://localhost/")?;
        for (k, v) in query {
            url.query_pairs_mut()
                .append_pair(k, v.as_str().unwrap_or(&v.to_string()));
        }
        if let Some(q) = url.query() {
            path.push('?');
            path.push_str(q);
        }
    }
    let mut principal = row.get::<Uuid, _>("managed_by");
    if let Some(invocation) = body["params"]["invocation_id"].as_str() {
        let actor:Option<Uuid>=sqlx::query_scalar("SELECT actor_id FROM plugin_operations WHERE workspace_id=$1 AND instance_id=$2 AND request#>>'{params,invocation,id}'=$3 AND state='running'").bind(workspace).bind(instance).bind(invocation).fetch_optional(&s.pool).await?.flatten();
        if let Some(actor) = actor {
            principal = actor;
        } else {
            let delivered:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_deliveries WHERE workspace_id=$1 AND instance_id=$2 AND invocation_id::text=$3 AND state='running')").bind(workspace).bind(instance).bind(invocation).fetch_one(&s.pool).await?;
            if !delivered {
                return Err(Error(
                    StatusCode::FORBIDDEN,
                    "host API invocation is not active".into(),
                ));
            }
        }
    }
    if ["tasks.enable", "tasks.disable"].contains(&method) {
        let id = segment(&body["params"], "id").map_err(|e| Error::bad(e.to_string()))?;
        let row = sqlx::query("SELECT config,revision FROM tasks WHERE workspace_id=$1 AND id=$2")
            .bind(workspace)
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(Error::missing)?;
        let revision: i64 = row.get("revision");
        if body["params"]["expected_revision"]
            .as_i64()
            .is_some_and(|r| r != revision)
        {
            return Err(Error::conflict("task revision conflict"));
        }
        let mut config: Value = row.get("config");
        config["enabled"] = json!(method == "tasks.enable");
        payload = Some(json!({"config":config,"expected_revision":revision}));
    }
    let key = body["params"]["request_key"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            format!(
                "plugin:{}:{method}:{}",
                body["params"]["invocation_id"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
                hex::encode(Sha256::digest(body["params"].to_string()))
            )
        });
    let invocation = body["params"]["invocation_id"]
        .as_str()
        .and_then(|s| s.parse::<Uuid>().ok());
    let context = if let Some(invocation) = invocation {
        sqlx::query("SELECT e.id,COALESCE((e.data->>'_causation_depth')::integer,0) AS depth FROM plugin_deliveries d JOIN plugin_events e ON e.id=d.event_id WHERE d.invocation_id=$1 AND d.instance_id=$2 AND d.state='running'").bind(invocation).bind(instance).fetch_optional(&s.pool).await?
    } else {
        None
    };
    let cause = context.as_ref().map(|r| r.get::<Uuid, _>("id"));
    let depth = context
        .as_ref()
        .map(|r| r.get::<i32, _>("depth") + 1)
        .unwrap_or(0);
    if depth > 32 {
        return Err(Error::conflict(
            "plugin causal chain exceeded 32 operations",
        ));
    }
    let token = crate::auth::credentials::secret();
    let hash = crate::auth::credentials::hash(&token);
    sqlx::query("INSERT INTO plugin_api_tokens(hash,instance_id,actor_id,workspace_id,invocation_id,causation_id,causation_depth,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,now()+interval '60 seconds')").bind(&hash).bind(instance).bind(principal).bind(workspace).bind(invocation).bind(cause).bind(depth).execute(&s.pool).await?;
    let listen = s
        .config
        .listen
        .replace("0.0.0.0", "127.0.0.1")
        .replace("[::]", "[::1]");
    let url = format!("http://{listen}/api/v1/workspaces/{workspace}{path}");
    let mut request = reqwest::Client::new()
        .request(verb.parse()?, url)
        .bearer_auth(token)
        .header("Idempotency-Key", key)
        .header("X-Wakeit-Plugin", instance.to_string());
    if let Some(payload) = payload {
        request = request.json(&payload);
    }
    let result = request.send().await;
    let response = match result {
        Ok(r) => r,
        Err(e) => {
            let _ = sqlx::query("DELETE FROM plugin_api_tokens WHERE hash=$1")
                .bind(hash)
                .execute(&s.pool)
                .await;
            return Err(e.into());
        }
    };
    let status = response.status();
    let value: Value = response.json().await?;
    sqlx::query("DELETE FROM plugin_api_tokens WHERE hash=$1")
        .bind(hash)
        .execute(&s.pool)
        .await?;
    if !status.is_success() {
        return Err(Error(
            status,
            value["error"].as_str().unwrap_or("host API failed").into(),
        ));
    }
    if verb != "GET"
        && !["validate", "preview", "list", "inspect", "get"]
            .contains(&method.rsplit('.').next().unwrap_or(""))
    {
        if status == StatusCode::ACCEPTED {
            return Ok(Json(value));
        }
        if value.get("state").is_some() && value.get("revision").is_some() {
            return Ok(Json(value));
        }
        return Ok(Json(
            json!({"id":Uuid::new_v4(),"state":"committed","revision":value["revision"].as_i64().unwrap_or(1),"result":value,"error":null}),
        ));
    }
    Ok(Json(value))
}

pub(crate) async fn device_events(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (runner, workspace) = device_workspace(&s, &h).await?;
    let items = body["items"]
        .as_array()
        .ok_or_else(|| Error::bad("event items required"))?;
    if items.len() > 100 {
        return Err(Error::bad("event batch too large"));
    }
    let mut tx = s.pool.begin().await?;
    for event in items {
        let id: Uuid = event["id"]
            .as_str()
            .ok_or_else(|| Error::bad("event ID required"))?
            .parse()
            .map_err(|_| Error::bad("invalid event ID"))?;
        let kind = event["event_type"]
            .as_str()
            .ok_or_else(|| Error::bad("event type required"))?;
        let instance = event["instance_id"]
            .as_str()
            .and_then(|s| s.parse::<Uuid>().ok());
        if let Some(instance) = instance {
            let owner:Option<(String,String)>=sqlx::query_as("SELECT source,extension FROM plugin_instances WHERE id=$1 AND runner_id=$2 AND workspace_id=$3 AND enabled").bind(instance).bind(runner).bind(workspace).fetch_optional(&mut *tx).await?;
            let (source, extension) =
                owner.ok_or_else(|| Error::bad("event instance not authorized"))?;
            if !kind.starts_with(&format!("plugin:{source}#{extension}/")) {
                return Err(Error::bad("plugin cannot emit core events"));
            }
        } else if ![
            "execution.",
            "result.",
            "session.",
            "artifact.",
            "file.",
            "instance.",
            "plugin.",
            "credential.",
            "auth.",
            "action.",
        ]
        .iter()
        .any(|prefix| kind.starts_with(prefix))
        {
            return Err(Error::bad("device cannot emit this event type"));
        }
        if let Some(run) = event["data"]["run_id"]
            .as_str()
            .and_then(|s| s.parse::<Uuid>().ok())
        {
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id=$1 AND runner_id=$2 AND workspace_id=$3)").bind(run).bind(runner).bind(workspace).fetch_one(&mut *tx).await?;
            if !exists {
                return Err(Error::bad("event run not owned by device"));
            }
        }
        let key = event["data"]["run_id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| instance.unwrap_or(runner).to_string());
        if kind == "artifact.registered" {
            let data = &event["data"];
            let artifact: Uuid = serde_json::from_value(data["artifact_id"].clone())
                .map_err(|_| Error::bad("artifact ID required"))?;
            let owner: Uuid = serde_json::from_value(data["instance_id"].clone())
                .map_err(|_| Error::bad("artifact instance required"))?;
            let owned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_instances WHERE id=$1 AND runner_id=$2 AND workspace_id=$3)").bind(owner).bind(runner).bind(workspace).fetch_one(&mut *tx).await?;
            if !owned {
                return Err(Error::bad("artifact belongs to another device"));
            }
            let expiry: Option<DateTime<Utc>> = serde_json::from_value(data["expires_at"].clone())
                .map_err(|_| Error::bad("invalid artifact expiry"))?;
            sqlx::query("INSERT INTO plugin_artifacts(id,workspace_id,instance_id,runner_id,name,format,reference,metadata,expires_at) VALUES($1,$2,$3,$4,$5,'file',$6,$7,$8) ON CONFLICT(id) DO NOTHING").bind(artifact).bind(workspace).bind(owner).bind(runner).bind(data["name"].as_str().unwrap_or("Artifact")).bind(&data["reference"]).bind(&data["metadata"]).bind(expiry).execute(&mut *tx).await?;
        }
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_events WHERE id=$1)")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            continue;
        }
        let revision:i64=sqlx::query_scalar("INSERT INTO plugin_aggregate_revisions(workspace_id,aggregate_type,aggregate_id,revision) VALUES($1,'execution',$2,1) ON CONFLICT(workspace_id,aggregate_type,aggregate_id) DO UPDATE SET revision=plugin_aggregate_revisions.revision+1 RETURNING revision").bind(workspace).bind(&key).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO plugin_events(id,workspace_id,event_type,aggregate_type,aggregate_id,aggregate_revision,source_runner_id,origin,data) VALUES($1,$2,$3,'execution',$4,$5,$6,$7,$8)").bind(id).bind(workspace).bind(kind).bind(key).bind(revision).bind(runner).bind(json!({"kind":if instance.is_some(){"plugin"}else{"runner"},"id":instance.unwrap_or(runner),"roles":[]})).bind(&event["data"]).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn auth_register(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Value>,
) -> Result<Json<Value>> {
    let (runner, workspace) = device_workspace(&s, &h).await?;
    let id: Uuid = body["id"]
        .as_str()
        .ok_or_else(|| Error::bad("flow ID required"))?
        .parse()
        .map_err(|_| Error::bad("invalid flow ID"))?;
    let instance: Uuid = body["instance_id"]
        .as_str()
        .ok_or_else(|| Error::bad("instance ID required"))?
        .parse()
        .map_err(|_| Error::bad("invalid instance"))?;
    let hash = body["state_hash"]
        .as_str()
        .ok_or_else(|| Error::bad("state hash required"))?;
    if hash.len() != 64 {
        return Err(Error::bad("invalid state hash"));
    }
    let expires: DateTime<Utc> = serde_json::from_value(body["expires_at"].clone())
        .map_err(|_| Error::bad("invalid expiry"))?;
    sqlx::query("INSERT INTO plugin_auth_flows(id,workspace_id,runner_id,instance_id,state_hash,expires_at) SELECT $1,$2,$3,$4,$5,LEAST($6,now()+interval '15 minutes') WHERE EXISTS(SELECT 1 FROM plugin_instances WHERE id=$4 AND runner_id=$3 AND workspace_id=$2 AND enabled)").bind(id).bind(workspace).bind(runner).bind(instance).bind(hash).bind(expires).execute(&s.pool).await?;
    Ok(Json(
        json!({"id":id,"callback_url":format!("{}/api/v1/plugins/auth/callback",s.config.public_url.trim_end_matches('/'))}),
    ))
}
#[derive(Deserialize)]
pub(crate) struct AuthCallback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}
pub(crate) async fn auth_callback(
    State(s): State<App>,
    Query(q): Query<AuthCallback>,
) -> Result<axum::response::Html<&'static str>> {
    use sha2::{Digest, Sha256};
    let row=sqlx::query("SELECT * FROM plugin_auth_flows WHERE state_hash=$1 AND status='pending' AND expires_at>now()").bind(hex::encode(Sha256::digest(q.state.as_bytes()))).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    if q.error.is_some() {
        sqlx::query("UPDATE plugin_auth_flows SET status='failed' WHERE id=$1")
            .bind(row.get::<Uuid, _>("id"))
            .execute(&s.pool)
            .await?;
        return Err(Error::bad("Authorization was not completed"));
    }
    rpc(&s,row.get("workspace_id"),row.get("runner_id"),"auth_callback",json!({"instance_id":row.get::<Uuid,_>("instance_id"),"state":q.state,"code":q.code.ok_or_else(||Error::bad("authorization code missing"))?})).await?;
    sqlx::query("UPDATE plugin_auth_flows SET status='authorized' WHERE id=$1")
        .bind(row.get::<Uuid, _>("id"))
        .execute(&s.pool)
        .await?;
    Ok(axum::response::Html(
        "<!doctype html><meta charset=utf-8><title>Wakeit</title><p>Authorization complete. You can close this page.</p>",
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Replay {
    id: Uuid,
    binding_id: Uuid,
    event_ids: Vec<Uuid>,
}
pub(crate) async fn replay(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(body): ApiJson<Replay>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "admin", true).await?;
    if body.event_ids.is_empty() || body.event_ids.len() > 1000 {
        return Err(Error::bad("select 1–1000 events to replay"));
    }
    let mut tx = s.pool.begin().await?;
    let b=sqlx::query("SELECT b.*,i.version,i.revision AS instance_revision FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.id=$1 AND b.workspace_id=$2 AND b.kind='event'").bind(body.binding_id).bind(workspace).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    let context = json!({"version":b.get::<String,_>("version"),"revision":b.get::<i64,_>("instance_revision"),"handler":b.get::<String,_>("handler"),"binding_revision":b.get::<i64,_>("revision")});
    let ids:Vec<Uuid>=sqlx::query_scalar("INSERT INTO plugin_deliveries(workspace_id,event_id,binding_id,instance_id,runner_id,replay_id,expires_at,context) SELECT $1,e.id,$2,$3,$4,$5,LEAST(e.expires_at,now()+make_interval(secs=>$6)),$7 FROM plugin_events e WHERE e.workspace_id=$1 AND e.id=ANY($8) AND e.expires_at>now() AND e.event_type=$9 ON CONFLICT DO NOTHING RETURNING id")
        .bind(workspace).bind(body.binding_id).bind(b.get::<Uuid,_>("instance_id")).bind(b.get::<Uuid,_>("runner_id")).bind(body.id).bind(b.get::<i32,_>("expires_seconds") as f64).bind(context).bind(body.event_ids).bind(b.get::<String,_>("event")).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":body.id,"state":"committed","deliveries":ids}),
    ))
}

pub(crate) async fn reprocess_result(
    State(s): State<App>,
    h: HeaderMap,
    Path(run): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "operator", true).await?;
    let runner:Uuid=sqlx::query_scalar("SELECT runner_id FROM runs WHERE workspace_id=$1 AND id=$2 AND state='failed' AND execution_snapshot->>'failure_phase'='postprocess'").bind(workspace).bind(run).fetch_optional(&s.pool).await?.ok_or_else(||Error::conflict("Run has no failed result processing"))?;
    let id = store::enqueue(
        &s.pool,
        workspace,
        runner,
        None,
        actor.id,
        &request_key(&h)?,
        "reprocess_result",
        json!({"action":"reprocess_result","params":{"run_id":run}}),
        Utc::now() + chrono::Duration::hours(1),
    )
    .await?;
    Ok(Json(
        json!({"id":id,"state":"pending","revision":1,"result":null,"error":null}),
    ))
}

pub(crate) async fn operation_action(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(body): ApiJson<Control>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "operator", true).await?;
    let mut tx = s.pool.begin().await?;
    let row=sqlx::query("SELECT actor_id,state,kind FROM plugin_operations WHERE id=$1 AND workspace_id=$2 FOR UPDATE").bind(id).bind(workspace).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    if actor.role != "admin" && row.get::<Option<Uuid>, _>("actor_id") != Some(actor.id) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "operation belongs to another actor".into(),
        ));
    }
    match body.action.as_str() {
        "cancel" => {
            if ["committed", "succeeded"].contains(&row.get::<String, _>("state").as_str()) {
                return Err(Error::conflict("operation already committed"));
            }
            sqlx::query("UPDATE plugin_operations SET state='cancelled',revision=revision+1,updated_at=now(),locked_until=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
            sqlx::query("UPDATE plugin_operations SET state='cancelled',revision=revision+1,updated_at=now(),locked_until=NULL WHERE id IN (SELECT id FROM plugin_hook_calls WHERE operation_id=$1) AND state IN ('pending','running','retry_wait')").bind(id).execute(&mut *tx).await?;
        }
        _ => return Err(Error::bad("unsupported operation action")),
    }
    tx.commit().await?;
    Ok(Json(json!({"id":id,"state":"cancelled"})))
}

pub(crate) async fn artifact_download(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response> {
    use axum::response::IntoResponse;
    use base64::Engine;
    let workspace = workspace_header(&h)?;
    user(&s, &h, "operator", false).await?;
    let artifact=sqlx::query("SELECT * FROM plugin_artifacts WHERE id=$1 AND workspace_id=$2 AND (expires_at IS NULL OR expires_at>now())").bind(id).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    let runner: Uuid = artifact.get("runner_id");
    let reference: Value = artifact.get("reference");
    let first=rpc(&s,workspace,runner,"file_read",json!({"path":reference["path"],"revision":reference["revision"],"offset":0,"operation":"read","limit":262144})).await?;
    let first_bytes = base64::engine::general_purpose::STANDARD
        .decode(
            first["data"]
                .as_str()
                .ok_or_else(|| Error::bad("file chunk missing"))?,
        )
        .map_err(|_| Error::bad("invalid file chunk"))?;
    let stream: std::pin::Pin<
        Box<
            dyn futures::Stream<Item = std::result::Result<axum::body::Bytes, std::io::Error>>
                + Send,
        >,
    > = Box::pin(async_stream::try_stream! {
        yield axum::body::Bytes::from(first_bytes);
        let mut next=first["next_offset"].as_u64().unwrap_or(0);let mut eof=first["eof"]==true;
        while !eof{
            let chunk=rpc(&s,workspace,runner,"file_read",json!({"path":reference["path"],"revision":reference["revision"],"offset":next,"operation":"read","limit":262144})).await.map_err(|_|std::io::Error::other("artifact source unavailable or changed"))?;
            let bytes=base64::engine::general_purpose::STANDARD.decode(chunk["data"].as_str().ok_or_else(||std::io::Error::other("file chunk missing"))?).map_err(|_|std::io::Error::other("invalid file chunk"))?;
            next=chunk["next_offset"].as_u64().ok_or_else(||std::io::Error::other("file cursor missing"))?;eof=chunk["eof"]==true;
            yield axum::body::Bytes::from(bytes);
        }
    });
    let name: String = artifact.get("name");
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"._-".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let mut response = axum::body::Body::from_stream::<_>(stream).into_response();
    response.headers_mut().insert(
        "content-type",
        axum::http::HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        "content-disposition",
        format!("attachment; filename*=UTF-8''{encoded}")
            .parse()
            .map_err(|_| Error::bad("invalid artifact name"))?,
    );
    Ok(response)
}
