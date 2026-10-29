use crate::store;
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use wakeit_core::config::TaskConfig;

use crate::api::access::{user, workspace_header};
use crate::api::http::{ApiJson, Error, Path, Result, pagination, request_key, search_pattern};
use crate::state::App;

#[derive(Default, Deserialize)]
pub(crate) struct ListTasks {
    #[serde(default)]
    archived: bool,
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    runner: Option<String>,
    agent: Option<String>,
    enabled: Option<bool>,
}

pub(crate) async fn tasks(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<ListTasks>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(
        &s,
        &h,
        if q.archived { "operator" } else { "viewer" },
        false,
    )
    .await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH filtered AS (SELECT t.* FROM tasks t WHERE workspace_id=$10 AND (archived_at IS NOT NULL)=$9 AND ($1::text IS NULL OR concat_ws(' ',config->>'name',id,config->>'prompt',config->>'cwd',config->>'runner',config->>'agent',config->'dag') ILIKE $1) AND (($2::text IS NULL AND $3::text IS NULL) OR ((config->>'runner'=$2 OR $2::text IS NULL) AND (config->>'agent'=$3 OR $3::text IS NULL)) OR EXISTS(SELECT 1 FROM jsonb_array_elements(COALESCE(config->'dag'->'nodes','[]'::jsonb)) n WHERE ($2::text IS NULL OR n->>'runner'=$2 OR EXISTS(SELECT 1 FROM runners x WHERE x.workspace_id=t.workspace_id AND x.name=$2 AND x.id::text=n->>'runner')) AND ($3::text IS NULL OR n->>'agent'=$3))) AND ($4::boolean IS NULL OR enabled=$4)), paged AS (SELECT to_jsonb(t.*) || jsonb_build_object('last_run',(SELECT jsonb_build_object('id',id,'state',state,'created_at',created_at) FROM runs WHERE parent_run_id IS NULL AND workspace_id=t.workspace_id AND task_id=t.id ORDER BY created_at DESC,id DESC LIMIT 1)) AS item FROM filtered t ORDER BY updated_at DESC,id DESC LIMIT $5 OFFSET $6) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(item) FROM paged),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$7::bigint,'page_size',$8::bigint)"
    ).bind(search_pattern(q.q)?).bind(q.runner).bind(q.agent).bind(q.enabled)
    .bind(q.page.map(|_| size)).bind(if q.page.is_some() { (page-1)*size } else {0})
    .bind(page).bind(size).bind(q.archived).bind(workspace).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}

pub(crate) async fn task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT to_jsonb(t.*) FROM tasks t WHERE workspace_id=$2 AND id=$1")
            .bind(id)
            .bind(workspace)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(Error::missing)?,
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Apply {
    config: TaskConfig,
    expected_revision: Option<i64>,
    #[serde(default)]
    editor_layout: Option<crate::editor_layout::EditorLayout>,
}

pub(crate) async fn validate_task(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Apply>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    if let Some(layout) = &b.editor_layout {
        layout
            .validate(&b.config)
            .map_err(|e| Error::bad(e.to_string()))?;
    }
    Ok(Json(
        json!({"errors":crate::store::dag::validation(&s.pool,u.workspace()?,&b.config).await?}),
    ))
}

pub(crate) async fn apply(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<Apply>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    if b.config.id != id {
        return Err(Error::bad("task id does not match URL"));
    }
    b.config.validate().map_err(|e| Error::bad(e.to_string()))?;
    Ok(Json(
        store::tasks::apply_task(
            &s.pool,
            workspace,
            &u.credential,
            &b.config,
            b.expected_revision,
            u.id,
            b.editor_layout.as_ref(),
        )
        .await
        .map_err(|e| Error::bad(e.to_string()))?
        .ok_or_else(|| Error::conflict("task changed; reload before saving"))?,
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ArchiveBody {
    expected_revision: i64,
}

pub(crate) async fn archive_task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<ArchiveBody>,
) -> Result<Json<Value>> {
    archive_change(s, h, id, b, true).await
}

pub(crate) async fn restore_task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<ArchiveBody>,
) -> Result<Json<Value>> {
    archive_change(s, h, id, b, false).await
}

pub(crate) async fn archive_change(
    s: App,
    h: HeaderMap,
    id: String,
    b: ArchiveBody,
    archived: bool,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    Ok(Json(
        store::tasks::set_archived(
            &s.pool,
            workspace,
            &u.credential,
            &id,
            b.expected_revision,
            archived,
            u.id,
        )
        .await
        .map_err(|e| Error::conflict(e.to_string()))?
        .ok_or_else(Error::missing)?,
    ))
}

pub(crate) async fn trigger(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    let id = store::runs::trigger(
        &s.pool,
        workspace,
        &u.credential,
        &id,
        u.id,
        None,
        &request_key(&h)?,
    )
    .await
    .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"id":id})))
}
