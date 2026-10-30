use crate::{auth, protocol, store};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::time::Duration;
use uuid::Uuid;

use crate::api::access::{user, workspace_header};
use crate::api::http::{ApiJson, Error, Path, Result};
use crate::auth::workspace::reauthorize;
use crate::state::App;

pub(crate) async fn runners(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'name',name,'revoked',revoked,'last_seen',last_seen,'online',NOT revoked AND last_seen>now()-interval '60 seconds','agents',agents,'directory_roots',directory_roots,'info',info) FROM runners WHERE workspace_id=$1 ORDER BY name").bind(workspace).fetch_all(&s.pool).await?)))
}

pub(crate) async fn agents(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT a || jsonb_build_object('runner_id',r.id,'runner',r.name) FROM runners r CROSS JOIN LATERAL jsonb_array_elements(r.agents) a WHERE r.workspace_id=$1 AND NOT r.revoked").bind(workspace).fetch_all(&s.pool).await?)))
}

pub(crate) async fn enrollment(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "operator", true).await?;
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &actor, "operator").await?;
    let t = auth::credentials::secret();
    sqlx::query("INSERT INTO enrollments(hash,expires_at,workspace_id,created_by) VALUES($1,now()+interval '30 minutes',$2,$3)")
        .bind(auth::credentials::hash(&t))
        .bind(workspace).bind(actor.id).execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"token":t,"gateway":s.config.public_url})))
}

pub(crate) async fn revoke(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "admin", true).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners WHERE workspace_id=$1 AND id=$2)")
            .bind(workspace)
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if !exists {
        return Err(Error::missing());
    }
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "admin").await?;
    // Serialize revocation with scheduling/claiming on the affected tasks.
    sqlx::query("SELECT id FROM tasks WHERE runner_id=$1 ORDER BY id FOR UPDATE")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    sqlx::query("UPDATE runners SET revoked=true WHERE workspace_id=$2 AND id=$1")
        .bind(id)
        .bind(workspace)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runs SET state='unknown',reason='runner revoked' WHERE runner_id=$1 AND state IN ('running','starting')").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE runs SET state='cancelled',reason='runner revoked before start',finished_at=now() WHERE runner_id=$1 AND state='queued'").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE tasks SET enabled=false,config=jsonb_set(config,'{enabled}','false'),revision=revision+1 WHERE runner_id=$1 AND enabled").bind(id).execute(&mut *tx).await?;
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        "runner.revoke",
        &id.to_string(),
    )
    .await?;
    tx.commit().await?;
    let pending = s
        .authority
        .sync_revocations(&s.config.nats_url, &s.pool)
        .await
        .is_err();
    Ok(Json(json!({"ok":true,"broker_revocation_pending":pending})))
}

#[derive(Deserialize)]
pub(crate) struct DirectoryQuery {
    path: Option<String>,
    cursor: Option<String>,
}

pub(crate) async fn directories(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", false).await?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
            id,
            "list_directories",
            json!({"path":q.path,"cursor":q.cursor}),
            5,
        )
        .await?,
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelsQuery {
    cwd: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionQuery {
    cwd: String,
    id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionsQuery {
    cwd: String,
    cursor: Option<String>,
    q: Option<String>,
}

pub(crate) async fn query_sessions(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<SessionsQuery>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    if q.q.as_ref().is_some_and(|v| v.len() > 1024)
        || q.cursor.as_ref().is_some_and(|v| v.len() > 2048)
    {
        return Err(Error::bad("invalid session query"));
    }
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
            id,
            "list_sessions",
            json!({"cwd":q.cwd,"agent":agent,"q":q.q,"cursor":q.cursor}),
            30,
        )
        .await?,
    ))
}

pub(crate) async fn inspect_session(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<SessionQuery>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    crate::sessions::validate_id(&q.id).map_err(|e| Error::bad(e.to_string()))?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
            id,
            "inspect_session",
            json!({"cwd":q.cwd,"id":q.id,"agent":agent}),
            30,
        )
        .await?,
    ))
}

pub(crate) async fn query_models(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<ModelsQuery>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
            id,
            "list_models",
            json!({"cwd":q.cwd,"agent":agent}),
            30,
        )
        .await?,
    ))
}

pub(crate) async fn device_query(
    s: &App,
    headers: &HeaderMap,
    workspace: Uuid,
    id: Uuid,
    action: &str,
    payload: Value,
    timeout: u64,
) -> Result<Value> {
    use futures::StreamExt;
    let row = sqlx::query(
        "SELECT runtime_id,last_seen FROM runners WHERE workspace_id=$2 AND id=$1 AND NOT revoked",
    )
    .bind(id)
    .bind(workspace)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(Error::missing)?;
    let runtime = row
        .get::<Option<Uuid>, _>("runtime_id")
        .ok_or_else(|| Error(StatusCode::SERVICE_UNAVAILABLE, "Runner 离线".into()))?;
    if row
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_seen")
        .is_none_or(|t| chrono::Utc::now() - t > chrono::Duration::seconds(60))
    {
        return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "Runner 离线".into()));
    }
    let request = protocol::QueryRequest {
        version: 1,
        id: Uuid::new_v4(),
        runtime_id: runtime,
        action: action.into(),
        payload,
    };
    let mut sub = crate::transport::query::subscribe(&s.client, id, &request).await?;
    let result=tokio::time::timeout(Duration::from_secs(timeout),async {
        while let Some(msg)=sub.next().await {
            if msg.status==Some(async_nats::StatusCode::NO_RESPONDERS) {return Err(Error(StatusCode::SERVICE_UNAVAILABLE,"Runner 离线".into()));}
            let response:protocol::QueryReply=serde_json::from_slice(&msg.payload)?;
            if response.version!=1 || response.id!=request.id || response.runtime_id!=runtime {continue;}
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1 AND runtime_id=$2 AND NOT revoked)").bind(id).bind(runtime).fetch_one(&s.pool).await?;
            if !valid {return Err(Error::conflict("Runner 身份已改变"));}
            user(s,headers,"operator",false).await?;
            if let Some(error)=response.error {return Err(Error(StatusCode::BAD_REQUEST,error.message));}
            return response.data.ok_or_else(||Error::bad("empty device response"));
        }
        Err(Error(StatusCode::SERVICE_UNAVAILABLE,"Runner 离线".into()))
    }).await;
    let _ = sub.unsubscribe().await;
    result.map_err(|_| Error(StatusCode::GATEWAY_TIMEOUT, "Runner 查询超时".into()))?
}
