//! Workspace-authorized browser terminals. Database rows contain lifecycle metadata only.
use crate::{
    api::{
        self,
        http::{ApiJson, Error, Path, Result},
    },
    auth,
    auth::workspace::User,
    state::App,
    terminal_protocol::{self as wire, Frame},
};
use axum::{
    Json,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::store::terminals::{apply_state, refresh, request, stop};
use crate::transport::query::terminal as rpc;
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Create {
    pub request_id: Uuid,
    #[serde(default)]
    pub node_id: Option<String>,
}

fn id_json(row: &PgRow) -> Value {
    json!({"id":row.get::<Uuid,_>("id"),"state":row.get::<String,_>("state"),"generation":row.get::<i64,_>("generation"),"reason":row.get::<Option<String>,_>("reason"),"exit_code":row.get::<Option<i32>,_>("exit_code")})
}

async fn actor(s: &App, h: &HeaderMap, write: bool) -> Result<User> {
    api::access::user(s, h, "operator", write).await
}

async fn owned(s: &App, actor: &User, id: Uuid) -> Result<PgRow> {
    sqlx::query("SELECT t.* FROM terminals t JOIN runners r ON r.id=t.runner_id AND r.workspace_id=t.workspace_id WHERE t.id=$1 AND t.workspace_id=$2 AND t.user_id=$3 AND t.credential=$4 AND NOT r.revoked")
        .bind(id).bind(actor.workspace()?).bind(actor.id).bind(&actor.credential).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)
}

pub(crate) async fn create(
    State(s): State<App>,
    h: HeaderMap,
    Path(source): Path<Uuid>,
    ApiJson(body): ApiJson<Create>,
) -> Result<Json<Value>> {
    let who = actor(&s, &h, true).await?;
    let workspace = who.workspace()?;
    let source = if let Some(node) = body.node_id.as_deref() {
        sqlx::query_scalar("SELECT execution_id FROM dag_run_nodes WHERE workspace_id=$1 AND parent_run_id=$2 AND node_id=$3")
            .bind(workspace).bind(source).bind(node).fetch_optional(&s.pool).await?.flatten().ok_or_else(Error::missing)?
    } else {
        source
    };
    let run=sqlx::query("SELECT r.*,n.runtime_id AS current_runtime,n.last_seen AS runner_last_seen,n.revoked,n.info FROM runs r JOIN runners n ON n.id=r.runner_id AND n.workspace_id=r.workspace_id WHERE r.id=$1 AND r.workspace_id=$2")
        .bind(source).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    if run.get::<bool, _>("revoked") {
        return Err(Error::missing());
    }
    if !crate::protocol::terminal(&run.get::<String, _>("state")) {
        return Err(Error::conflict("session is not ready for continuation"));
    }
    let session: crate::sessions::RunSession = serde_json::from_value(
        run.get::<Option<Value>, _>("session")
            .ok_or_else(|| Error::bad("session is unavailable"))?,
    )?;
    if !session.ready
        || session
            .command
            .as_deref()
            .is_none_or(|c| c.trim().is_empty())
    {
        return Err(Error::bad("session is unavailable"));
    }
    let runtime: Uuid = run
        .get::<Option<Uuid>, _>("current_runtime")
        .ok_or_else(|| Error(StatusCode::SERVICE_UNAVAILABLE, "Runner is offline".into()))?;
    if run
        .get::<Option<DateTime<Utc>>, _>("runner_last_seen")
        .is_none_or(|t| Utc::now() - t > chrono::Duration::seconds(60))
    {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Runner is offline".into(),
        ));
    }
    if run.get::<Value, _>("info")["web_terminal"] != 1 {
        return Err(Error::conflict("Runner does not support web terminals"));
    }
    let runner: Uuid = run.get("runner_id");
    if runner != session.runner_id {
        return Err(Error::conflict("session identity changed"));
    }
    // Refresh stale occupancy before entering the common session-claim transaction.
    if let Some(previous)=sqlx::query("SELECT * FROM terminals WHERE runner_id=$1 AND agent=$2 AND session_id=$3 AND state<>'closed'").bind(runner).bind(&session.agent).bind(&session.id).fetch_optional(&s.pool).await? {
        refresh(&s,&previous).await?;
    }
    let mut tx = s.pool.begin().await?;
    auth::workspace::reauthorize(&mut tx, &who, "operator").await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!(
            "session:{}:{}:{}",
            runner, session.agent, session.id
        ))
        .execute(&mut *tx)
        .await?;
    if let Some(previous) = sqlx::query(
        "SELECT * FROM terminals WHERE workspace_id=$1 AND user_id=$2 AND request_id=$3",
    )
    .bind(workspace)
    .bind(who.id)
    .bind(body.request_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        if previous.get::<Uuid, _>("run_id") != source
            || previous.get::<String, _>("credential") != who.credential
        {
            return Err(Error::conflict("terminal request identity changed"));
        }
        return Ok(Json(id_json(&previous)));
    }
    if let Some(previous)=sqlx::query("SELECT * FROM terminals WHERE runner_id=$1 AND agent=$2 AND session_id=$3 AND state<>'closed' FOR UPDATE").bind(runner).bind(&session.agent).bind(&session.id).fetch_optional(&mut *tx).await?{
        if previous.get::<Uuid,_>("user_id")==who.id && previous.get::<String,_>("credential")==who.credential && previous.get::<Uuid,_>("run_id")==source && !previous.get::<bool,_>("attached") && previous.get::<DateTime<Utc>,_>("lease_until")>Utc::now() && previous.get::<String,_>("state")!="unknown" {
            return Ok(Json(id_json(&previous)))
        }
        return Err(Error::conflict("session is already in use"))
    }
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE runner_id=$1 AND config->>'agent'=$2 AND COALESCE(session->>'id',config->'session'->>'id')=$3 AND state IN ('starting','running','unknown'))").bind(runner).bind(&session.agent).bind(&session.id).fetch_one(&mut *tx).await?;
    if busy {
        return Err(Error::conflict("session is already in use"));
    }
    let terminal = Uuid::new_v4();
    let row=sqlx::query("INSERT INTO terminals(id,workspace_id,run_id,runner_id,runtime_id,user_id,credential,request_id,agent,session_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING *")
        .bind(terminal).bind(workspace).bind(source).bind(runner).bind(runtime).bind(who.id).bind(&who.credential).bind(body.request_id).bind(&session.agent).bind(&session.id).fetch_one(&mut *tx).await?;
    crate::store::audit(
        &mut tx,
        who.id,
        Some(workspace),
        "terminal.open",
        &terminal.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(id_json(&row)))
}

pub(crate) async fn get(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let who = actor(&s, &h, false).await?;
    let row = owned(&s, &who, id).await?;
    refresh(&s, &row).await?;
    Ok(Json(id_json(&owned(&s, &who, id).await?)))
}

pub(crate) async fn attach(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let who = actor(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    auth::workspace::reauthorize(&mut tx, &who, "operator").await?;
    let row=sqlx::query("SELECT * FROM terminals WHERE id=$1 AND workspace_id=$2 AND user_id=$3 AND credential=$4 FOR UPDATE").bind(id).bind(who.workspace()?).bind(who.id).bind(&who.credential).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    if row.get::<bool, _>("attached")
        || ["closed", "closing", "unknown"].contains(&row.get::<String, _>("state").as_str())
    {
        return Err(Error::conflict(
            "terminal is unavailable or already attached",
        ));
    }
    let ready: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1 AND NOT revoked AND runtime_id=$2)",
    )
    .bind(row.get::<Uuid, _>("runner_id"))
    .bind(row.get::<Uuid, _>("runtime_id"))
    .fetch_one(&mut *tx)
    .await?;
    if !ready {
        return Err(Error::conflict("Runner runtime changed"));
    }
    let ticket = auth::credentials::secret();
    sqlx::query("UPDATE terminals SET ticket_hash=$2,ticket_expires=now()+interval '30 seconds' WHERE id=$1").bind(id).bind(auth::credentials::hash(&ticket)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ticket":ticket,"expires_in":30})))
}

pub(crate) async fn close(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let who = actor(&s, &h, true).await?;
    let row = owned(&s, &who, id).await?;
    stop(&s, &row).await?;
    Ok(Json(id_json(&owned(&s, &who, id).await?)))
}

pub(crate) async fn stream(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    upgrade: WebSocketUpgrade,
) -> Result<Response> {
    // WebSocket cannot send the normal CSRF marker header; only a ticket minted by a
    // same-origin POST can authorize it. Cookies alone never authorize terminal IO.
    let origin = h
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error(StatusCode::FORBIDDEN, "origin required".into()))?;
    let allowed = reqwest::Url::parse(&s.config.public_url)?
        .origin()
        .ascii_serialization();
    if origin != allowed {
        return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
    }
    let who = actor(&s, &h, false).await?;
    owned(&s, &who, id).await?;
    Ok(upgrade
        .max_message_size(64 * 1024)
        .on_upgrade(move |socket| async move {
            let _ = socket_loop(s, h, who, id, socket).await;
        })
        .into_response())
}
use axum::response::IntoResponse;

async fn socket_loop(
    s: App,
    h: HeaderMap,
    who: User,
    id: Uuid,
    mut socket: WebSocket,
) -> Result<()> {
    let first = tokio::time::timeout(Duration::from_secs(5), socket.recv())
        .await
        .map_err(|_| Error::bad("terminal handshake timed out"))?
        .ok_or_else(|| Error::bad("terminal connection closed"))?
        .map_err(anyhow::Error::from)?;
    let Message::Text(text) = first else {
        return Err(Error::bad("terminal handshake required"));
    };
    let init: Value = serde_json::from_str(&text)?;
    let _ = wire::dimensions(&init).map_err(|e| Error::bad(e.to_string()))?;
    let mut tx = s.pool.begin().await?;
    auth::workspace::reauthorize(&mut tx, &who, "operator").await?;
    let row=sqlx::query("UPDATE terminals SET attached=true,ticket_hash=NULL,generation=generation+1,lease_until=now()+interval '30 seconds',state=CASE WHEN state='created' THEN 'starting' ELSE state END WHERE id=$1 AND workspace_id=$2 AND user_id=$3 AND credential=$4 AND NOT attached AND state IN ('created','starting','running') AND ticket_hash=$5 AND ticket_expires>now() AND lease_until>now() RETURNING *")
        .bind(id).bind(who.workspace()?).bind(who.id).bind(&who.credential).bind(auth::credentials::hash(init["ticket"].as_str().unwrap_or_default())).fetch_optional(&mut *tx).await?.ok_or_else(||Error::conflict("terminal ticket expired or already used"))?;
    tx.commit().await?;
    let generation: i64 = row.get("generation");
    let mut output = s
        .client
        .subscribe(wire::output_subject(row.get("runner_id"), id))
        .await
        .map_err(anyhow::Error::from)?;
    s.client.flush().await.map_err(anyhow::Error::from)?;
    let mut data = init.clone();
    data["run_id"] = json!(row.get::<Uuid, _>("run_id"));
    data["session_id"] = json!(row.get::<String, _>("session_id"));
    data["agent"] = json!(row.get::<String, _>("agent"));
    data.as_object_mut().unwrap().remove("ticket");
    let attach = rpc(&s, request(&row, "attach", data)).await;
    let result:Result<()>=async {
        let snapshot=attach?;
        let mut after=snapshot["seq"].as_u64().unwrap_or(0);
        socket.send(Message::Text(json!({"kind":"snapshot","data":snapshot}).to_string().into())).await.map_err(anyhow::Error::from)?;
        if let Some(state)=snapshot.get("state"){apply_state(&s.pool,id,generation,state).await?;}
        let mut heartbeat=tokio::time::interval(Duration::from_secs(5));
        let mut last=Instant::now();
        loop {
            tokio::select!{
                _=s.shutdown.cancelled()=>break,
                item=socket.recv()=>{
                    let Some(item)=item else{break};let item=item.map_err(anyhow::Error::from)?;
                    let Message::Text(text)=item else{if matches!(item,Message::Close(_)){break}else{continue}};
                    let message:Value=serde_json::from_str(&text)?;
                    api::access::user(&s,&h,"operator",false).await?;
                    let active=owned(&s,&who,id).await?;
                    if active.get::<i64,_>("generation")!=generation || !active.get::<bool,_>("attached") || !["starting","running"].contains(&active.get::<String,_>("state").as_str()){break}
                    let action=message["kind"].as_str().unwrap_or("");
                    if action=="heartbeat"{last=Instant::now();continue}
                    if !["input","resize","snapshot"].contains(&action){return Err(Error::bad("unsupported terminal message"))}
                    let value=rpc(&s,request(&row,action,message["data"].clone())).await?;
                    if action=="snapshot"{after=value["seq"].as_u64().unwrap_or(after);socket.send(Message::Text(json!({"kind":"snapshot","data":value}).to_string().into())).await.map_err(anyhow::Error::from)?;}
                    if action=="input"{socket.send(Message::Text(json!({"kind":"ack","data":value}).to_string().into())).await.map_err(anyhow::Error::from)?;}
                },
                frame=output.next()=>{
                    let Some(frame)=frame else{break};
                    let frame:Frame=serde_json::from_slice(&frame.payload)?;
                    if frame.workspace_id!=who.workspace()? || frame.terminal_id!=id || frame.runtime_id!=row.get::<Uuid,_>("runtime_id") || frame.generation!=generation || frame.version!=1{continue}
                    if frame.kind=="state"{apply_state(&s.pool,id,generation,&frame.data).await?;}
                    if frame.kind=="output"{
                        if frame.seq<=after{continue}
                        if frame.seq!=after+1{let value=rpc(&s,request(&row,"snapshot",json!({}))).await?;after=value["seq"].as_u64().unwrap_or(frame.seq);socket.send(Message::Text(json!({"kind":"snapshot","data":value}).to_string().into())).await.map_err(anyhow::Error::from)?;continue}
                        after=frame.seq;
                    }
                    tokio::time::timeout(Duration::from_secs(5),socket.send(Message::Text(serde_json::to_string(&frame)?.into()))).await.map_err(|_|Error::bad("terminal viewer is too slow"))?.map_err(anyhow::Error::from)?;
                    if frame.kind=="state" && ["closed","unknown"].contains(&frame.data["state"].as_str().unwrap_or("")){break}
                },
                _=heartbeat.tick()=>{
                    if last.elapsed()>Duration::from_secs(15){break}
                    api::access::user(&s,&h,"operator",false).await?;
                    let active=owned(&s,&who,id).await?;
                    if active.get::<i64,_>("generation")!=generation || !active.get::<bool,_>("attached") || !["starting","running"].contains(&active.get::<String,_>("state").as_str()){break}
                    let updated=sqlx::query("UPDATE terminals SET lease_until=now()+interval '30 seconds' WHERE id=$1 AND generation=$2 AND attached AND state IN ('starting','running')").bind(id).bind(generation).execute(&s.pool).await?;
                    if updated.rows_affected()==0{break}
                    rpc(&s,request(&row,"lease",json!({}))).await?;
                }
            }
        }
        Ok(())
    }.await;
    if let Err(error) = &result {
        let _ = socket
            .send(Message::Text(
                json!({"kind":"error","data":{"message":error.1}})
                    .to_string()
                    .into(),
            ))
            .await;
    }
    let permission = api::access::user(&s, &h, "operator", false).await;
    if permission.is_err() || owned(&s, &who, id).await.is_err() {
        let _ = stop(&s, &row).await;
    } else {
        let _ = rpc(&s, request(&row, "detach", json!({}))).await;
    }
    sqlx::query("UPDATE terminals SET attached=false,lease_until=now()+interval '30 seconds' WHERE id=$1 AND generation=$2").bind(id).bind(generation).execute(&s.pool).await?;
    let _ = refresh(&s, &row).await;
    let _ = output.unsubscribe().await;
    let _ = socket.close().await;
    result
}
