use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, time::Duration};
use uuid::Uuid;

use crate::api::access::{user, workspace_header};
use crate::api::http::{Path, Result};
use crate::api::runs::verify_run;
use crate::state::App;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Cursor {
    #[serde(default)]
    pub(crate) after: i64,
    pub(crate) kind: Option<String>,
}

pub(crate) async fn event_rows(
    s: &App,
    id: Uuid,
    workspace: Uuid,
    after: i64,
    kind: Option<&str>,
) -> std::result::Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(e.*) || CASE WHEN r.parent_run_id IS NOT NULL THEN jsonb_build_object('run_id',$1,'node_id',r.node_id,'execution_id',r.id,'original_seq',e.seq,'kind',CASE WHEN e.kind='state' THEN 'node_state' ELSE e.kind END) ELSE CASE WHEN e.kind='node' THEN jsonb_build_object('node_id',e.data->'node_id') ELSE '{}'::jsonb END END FROM events e JOIN runs r ON r.id=e.run_id AND r.workspace_id=e.workspace_id WHERE e.workspace_id=$4 AND (e.run_id=$1 OR r.parent_run_id=$1) AND e.id>$2 AND ($3::text IS NULL OR e.kind=$3 OR ($3='trace' AND e.kind='trace_status')) ORDER BY e.id LIMIT 500",
    )
    .bind(id)
    .bind(after)
    .bind(kind)
    .bind(workspace).fetch_all(&s.pool)
    .await
}

pub(crate) async fn events(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    verify_run(&s, workspace, id).await?;
    Ok(Json(json!(
        event_rows(&s, id, workspace, q.after, q.kind.as_deref()).await?
    )))
}

pub(crate) async fn stream(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<impl IntoResponse> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    verify_run(&s, workspace, id).await?;
    let mut cursor = h
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(q.after);
    let output = async_stream::stream! {let mut tick=tokio::time::interval(Duration::from_secs(1));loop{tick.tick().await;
        if s.shutdown.is_cancelled() || user(&s,&h,"viewer",false).await.is_err(){break}
        match event_rows(&s,id,workspace,cursor,None).await {Ok(rows)=>for row in rows {cursor=row["id"].as_i64().unwrap_or(cursor);yield Ok::<_,Infallible>(Event::default().id(cursor.to_string()).event("run_event").data(row.to_string()));},Err(e)=>{tracing::warn!(error=%e,"SSE read failed");break}}
    }};
    Ok(Sse::new(output).keep_alive(KeepAlive::default()))
}
