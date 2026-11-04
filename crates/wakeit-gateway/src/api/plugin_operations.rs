//! Accepted business mutations remain queryable while remote policy hooks run.
use super::{
    access::{user, workspace_header},
    http::Error,
};
use crate::state::App;
use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) async fn boundary(State(s): State<App>, request: Request, next: Next) -> Response {
    let method = request.method().as_str();
    let path = request.uri().path();
    let relative = path
        .split("/workspaces/")
        .nth(1)
        .and_then(|s| s.split_once('/'))
        .map(|(_, p)| format!("/{p}"))
        .unwrap_or_else(|| path.to_owned());
    let stages = if method == "PUT" && relative.starts_with("/tasks/") {
        vec!["task.save.prepare", "task.activation.prepare"]
    } else if method == "PUT" && relative.starts_with("/task-templates/") {
        vec!["template.save.prepare"]
    } else if relative.starts_with("/task-templates/") && relative.ends_with("/instantiate") {
        vec!["template.compile.prepare", "template.compile.result"]
    } else if relative.starts_with("/runs/")
        && relative.contains("/nodes/")
        && relative.ends_with("/retry")
    {
        vec!["dag.recovery.prepare"]
    } else if method == "PUT" && relative == "/settings" {
        vec!["workspace.settings.prepare"]
    } else {
        vec![]
    };
    if stages.is_empty() {
        return next.run(request).await;
    }
    let headers = request.headers().clone();
    let result:super::http::Result<Option<(Uuid,Uuid,String)>>=async{
        let workspace=workspace_header(&headers)?;
        let actor=user(&s,&headers,if relative=="/settings"{"admin"}else{"operator"},true).await?;
        if actor.kind=="operation"{return Ok(None);}
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_bindings b JOIN plugin_instances i ON i.id=b.instance_id WHERE b.workspace_id=$1 AND b.enabled AND i.enabled AND b.kind='hook' AND b.event=ANY($2))").bind(workspace).bind(stages).fetch_one(&s.pool).await?;
        if !exists{return Ok(None);}
        let key=super::plugins::hook_key(&headers);Ok(Some((workspace,actor.id,key)))
    }.await;
    let (workspace, actor, key) = match result {
        Ok(Some(value)) => value,
        Ok(None) => return next.run(request).await,
        Err(e) => return e.into_response(),
    };
    let method = method.to_owned();
    let path = relative;
    let bytes = match axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024).await {
        Ok(v) => v,
        Err(_) => return Error::bad("request body exceeds 2 MiB").into_response(),
    };
    let body: Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return Error::bad("invalid JSON request").into_response(),
    };
    let request = json!({"method":method,"path":path,"body":body,"idempotency_key":key});
    let key = format!("business:{actor}:{key}");
    let id = Uuid::new_v4();
    let result:Result<(Uuid,Value),sqlx::Error>=sqlx::query_as("INSERT INTO plugin_operations(id,workspace_id,kind,request_key,actor_id,request,expires_at) VALUES($1,$2,'business',$3,$4,$5,now()+interval '15 minutes') ON CONFLICT(workspace_id,request_key) DO UPDATE SET request_key=excluded.request_key RETURNING id,request").bind(id).bind(workspace).bind(key).bind(actor).bind(&request).fetch_one(&s.pool).await;
    match result{
        Ok((id,existing)) if existing==request=>(StatusCode::ACCEPTED,Json(json!({"id":id,"state":"pending","revision":1,"result":null,"error":null,"operation_url":format!("/plugins/operations/{id}")}))).into_response(),
        Ok(_)=>Error::conflict("request key reused with a different business operation").into_response(),
        Err(e)=>Error::from(e).into_response()
    }
}
