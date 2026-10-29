use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::access::user;
use crate::api::http::{ApiJson, Error, Path, Result, request_key};
use crate::api::{
    events::{Cursor, event_rows},
    runs::{Resolution, resolve, run},
};
use crate::state::App;

pub(crate) async fn node_execution(
    s: &App,
    workspace: Uuid,
    id: Uuid,
    node: &str,
) -> Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT execution_id FROM dag_run_nodes WHERE workspace_id=$1 AND parent_run_id=$2 AND node_id=$3")
        .bind(workspace).bind(id).bind(node).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)
}

#[derive(Default, Deserialize)]
pub(crate) struct NodeQuery {
    execution: Option<Uuid>,
}

pub(crate) async fn run_node(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    Query(q): Query<NodeQuery>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    let workspace = u.workspace()?;
    let execution = node_execution(&s, workspace, id, &node).await?;
    let summary = crate::store::dag::summaries(&s.pool, workspace, id)
        .await?
        .into_iter()
        .find(|n| n["id"] == node)
        .ok_or_else(Error::missing)?;
    let attempts:Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'state',state,'reason',reason,'scheduled_at',scheduled_at,'started_at',started_at,'finished_at',finished_at) FROM runs WHERE workspace_id=$1 AND parent_run_id=$2 AND node_id=$3 ORDER BY created_at,id").bind(workspace).bind(id).bind(&node).fetch_all(&s.pool).await?;
    let execution = q.execution.or(execution);
    if execution.is_some_and(|id| !attempts.iter().any(|a| a["id"] == id.to_string())) {
        return Err(Error::missing());
    }
    let retry = crate::store::dag_recovery::info(&s, workspace, id, &node).await?;
    let execution = if let Some(id) = execution {
        run(State(s), h, Path(id)).await?.0
    } else {
        Value::Null
    };
    Ok(Json(
        json!({"node":summary,"execution":execution,"attempts":attempts,"retry":retry}),
    ))
}

pub(crate) async fn node_events(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    Query(q): Query<Cursor>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    let workspace = u.workspace()?;
    if let Some(execution) = node_execution(&s, workspace, id, &node).await? {
        Ok(Json(json!(
            event_rows(&s, execution, workspace, q.after, q.kind.as_deref()).await?
        )))
    } else {
        Ok(Json(json!([])))
    }
}

pub(crate) async fn resolve_node(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    ApiJson(b): ApiJson<Resolution>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    let execution = node_execution(&s, u.workspace()?, id, &node)
        .await?
        .ok_or_else(Error::missing)?;
    resolve(State(s), h, Path(execution), ApiJson(b)).await
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryNode {
    // Missing is different from an explicit null (a node that failed before dispatch).
    #[serde(deserialize_with = "required_execution")]
    #[schemars(required)]
    pub execution_id: Option<Uuid>,
    #[serde(default)]
    pub resume_stopped: bool,
}
fn required_execution<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Uuid>, D::Error> {
    Option::<Uuid>::deserialize(d)
}

pub(crate) async fn retry_node(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    ApiJson(b): ApiJson<RetryNode>,
) -> Result<Json<Value>> {
    let actor = user(&s, &h, "operator", true).await?;
    let key = request_key(&h)?;
    crate::store::dag_recovery::retry(
        &s,
        &actor,
        &key,
        id,
        &node,
        b.execution_id,
        b.resume_stopped,
    )
    .await
    .map(Json)
    .map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_guard_is_required_but_nullable() {
        assert!(serde_json::from_value::<RetryNode>(json!({})).is_err());
        assert!(serde_json::from_value::<RetryNode>(json!({"execution_id":null})).is_ok());
    }
}
