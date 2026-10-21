use crate::{
    api::{ApiJson, App, Error, Path, Result, user},
    result_files,
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct Preview {
    #[schemars(length(min = 1, max = 8192))]
    pub href: String,
}
pub(crate) async fn preview(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<Preview>,
) -> Result<Response> {
    let u = user(&s, &h, "operator", true).await?;
    let workspace = u.workspace()?;
    let row=sqlx::query("SELECT r.runner_id,r.result,r.execution_snapshot FROM runs r JOIN tasks t ON t.workspace_id=r.workspace_id AND t.id=r.task_id WHERE r.workspace_id=$1 AND r.id=$2 AND NOT r.config ? 'dag'").bind(workspace).bind(id).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    let result: Option<Value> = row.get("result");
    if b.href.len() > 8192
        || !result
            .as_ref()
            .and_then(result_files::report_text)
            .is_some_and(|text| result_files::linked(text, &b.href))
        || !result_files::local_link(&b.href)
    {
        return Err(Error::bad("link is not present in this execution result"));
    }
    let runner: Uuid = row.get("runner_id");
    let snapshot: Option<Value> = row.get("execution_snapshot");
    let cwd = snapshot.as_ref().and_then(|s| s["cwd"].as_str());
    let mut value = crate::api::device_query(
        &s,
        &h,
        workspace,
        runner,
        "read_result_file",
        json!({"href":b.href,"cwd":cwd}),
        10,
    )
    .await?;
    // A result can be removed by retention while the Runner is reading.
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE id=$1 AND workspace_id=$2)")
            .bind(id)
            .bind(workspace)
            .fetch_one(&s.pool)
            .await?;
    if !exists {
        return Err(Error::missing());
    }
    user(&s, &h, "operator", true).await?;
    value["runner_id"] = json!(runner);
    Ok((
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Json(value),
    )
        .into_response())
}
