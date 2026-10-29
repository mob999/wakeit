use crate::{protocol, store};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::api::access::{user, workspace_header};
use crate::api::http::{ApiJson, Error, Path, Result, pagination, request_key, search_pattern};
use crate::auth::workspace::reauthorize;
use crate::state::App;

pub(crate) async fn verify_run(s: &App, workspace: Uuid, id: Uuid) -> Result<()> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$1 AND id=$2)")
            .bind(workspace)
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if !exists {
        return Err(Error::missing());
    }
    Ok(())
}

pub(crate) async fn retry(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    let task: String = sqlx::query_scalar(
        "SELECT task_id FROM runs WHERE workspace_id=$2 AND id=$1 AND parent_run_id IS NULL",
    )
    .bind(id)
    .bind(workspace)
    .fetch_optional(&s.pool)
    .await?
    .ok_or_else(Error::missing)?;
    let id = store::runs::trigger(
        &s.pool,
        workspace,
        &u.credential,
        &task,
        u.id,
        Some(id),
        &request_key(&h)?,
    )
    .await
    .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"id":id})))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ListRuns {
    task: Option<String>,
    state: Option<String>,
    before: Option<DateTime<Utc>>,
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    runner: Option<String>,
    agent: Option<String>,
}

pub(crate) async fn runs(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<ListRuns>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH filtered AS (SELECT r.*,EXISTS(SELECT 1 FROM tasks t WHERE t.workspace_id=r.workspace_id AND t.id=r.task_id AND t.archived_at IS NOT NULL) AS task_archived FROM runs r WHERE r.parent_run_id IS NULL AND r.workspace_id=$11 AND ($1::text IS NULL OR task_id=$1) AND ($2::text IS NULL OR state=$2) AND ($3::timestamptz IS NULL OR created_at<$3) AND ($4::text IS NULL OR concat_ws(' ',id::text,task_id,config->>'name',config->>'prompt',config->>'cwd',config->>'runner',config->>'agent',config->'dag') ILIKE $4) AND (($5::text IS NULL AND $6::text IS NULL) OR ((config->>'runner'=$5 OR $5::text IS NULL) AND (config->>'agent'=$6 OR $6::text IS NULL)) OR EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(config->'dag'->'nodes','[]'::jsonb)) n WHERE ($5::text IS NULL OR n->>'runner'=$5 OR EXISTS(SELECT 1 FROM runners x WHERE x.workspace_id=r.workspace_id AND x.name=$5 AND x.id::text=n->>'runner')) AND ($6::text IS NULL OR n->>'agent'=$6)))), paged AS (SELECT to_jsonb(r.*) AS item FROM filtered r ORDER BY created_at DESC,id DESC LIMIT $7 OFFSET $8) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(item) FROM paged),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$9::bigint,'page_size',$10::bigint)"
    ).bind(q.task).bind(q.state).bind(q.before).bind(search_pattern(q.q)?).bind(q.runner).bind(q.agent)
    .bind(if q.page.is_some() {size} else {100}).bind(if q.page.is_some() {(page-1)*size} else {0})
    .bind(page).bind(size).bind(workspace).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}

pub(crate) async fn run(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let mut value:Value=sqlx::query_scalar("SELECT to_jsonb(r.*)||jsonb_build_object('task_archived',t.archived_at IS NOT NULL) FROM runs r JOIN tasks t ON t.id=r.task_id AND t.workspace_id=r.workspace_id WHERE r.workspace_id=$2 AND r.id=$1").bind(id).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    if value["config"].get("dag").is_some() {
        value["nodes"] = json!(crate::store::dag::summaries(&s.pool, workspace, id).await?);
    }
    Ok(Json(value))
}

pub(crate) async fn cancel(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    verify_run(&s, workspace, id).await?;
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "operator").await?;
    sqlx::query(
        "SELECT t.id FROM tasks t JOIN runs r ON r.task_id=t.id AND r.workspace_id=t.workspace_id WHERE r.workspace_id=$2 AND r.id=$1 FOR UPDATE OF t",
    )
    .bind(id)
    .bind(workspace).fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    let row =
        sqlx::query("SELECT state,runner_id,parent_run_id,config FROM runs WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if row.get::<Option<Uuid>, _>("parent_run_id").is_some() {
        return Err(Error::bad(
            "node executions cannot be cancelled independently",
        ));
    }
    let state: String = row.get("state");
    if !protocol::terminal(&state) {
        sqlx::query("UPDATE runs SET cancel_requested=true,stop_cause=CASE WHEN config ? 'dag' THEN 'graph_cancel' ELSE stop_cause END,state=CASE WHEN state='queued' THEN 'cancelled' ELSE state END,finished_at=CASE WHEN state='queued' THEN now() ELSE finished_at END WHERE id=$1").bind(id).execute(&mut *tx).await?;
        if row.get::<Option<Uuid>, _>("runner_id").is_none() {
            crate::store::dag::cancel_nodes(&mut tx, workspace, id, "graph_cancel").await?;
        } else {
            store::outbox::enqueue(
                &mut tx,
                workspace,
                protocol::control_subject(row.get("runner_id")),
                format!("cancel:{id}"),
                json!({"version":1,"run_id":id,"action":"cancel"}),
            )
            .await?;
        }
    }
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        "run.cancel",
        &id.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Resolution {
    state: String,
    reason: String,
}

pub(crate) async fn resolve(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<Resolution>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    if !matches!(
        b.state.as_str(),
        "failed" | "cancelled" | "succeeded" | "interrupted"
    ) || b.reason.trim().is_empty()
    {
        return Err(Error::bad("final state and verification reason required"));
    }
    verify_run(&s, workspace, id).await?;
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "operator").await?;
    crate::store::dag::lock_parent(&mut tx, id).await?;
    let graph: bool = sqlx::query_scalar("SELECT config ? 'dag' FROM runs WHERE id=$1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if graph {
        return Err(Error::bad("reconcile unknown graph nodes individually"));
    }

    let n = sqlx::query(
        "UPDATE runs SET state=$2,reason=$3,finished_at=now() WHERE id=$1 AND state='unknown'",
    )
    .bind(id)
    .bind(&b.state)
    .bind(format!("manually verified: {}", b.reason))
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(Error::conflict("only unknown runs can be resolved"));
    }
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        "run.resolve",
        &id.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
