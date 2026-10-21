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
    /// An original result link to a Markdown file containing this image.
    #[schemars(length(min = 1, max = 8192))]
    pub document: Option<String>,
    /// Thumbnail for inline display, or a bounded original-image chunk.
    pub image: Option<crate::image_preview::Mode>,
    #[serde(default)]
    pub offset: u64,
    #[schemars(length(max = 64))]
    pub revision: Option<String>,
}
pub(crate) async fn preview(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<Preview>,
) -> Result<Response> {
    if b.revision
        .as_ref()
        .is_some_and(|r| r.len() != 64 || !r.bytes().all(|c| c.is_ascii_hexdigit()))
        || (b.image.is_none() && (b.offset != 0 || b.revision.is_some()))
    {
        return Err(Error::bad("invalid image request"));
    }
    let u = user(&s, &h, "operator", true).await?;
    let workspace = u.workspace()?;
    let row=sqlx::query("SELECT r.runner_id,r.result,r.execution_snapshot FROM runs r JOIN tasks t ON t.workspace_id=r.workspace_id AND t.id=r.task_id WHERE r.workspace_id=$1 AND r.id=$2 AND NOT r.config ? 'dag'").bind(workspace).bind(id).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    let result: Option<Value> = row.get("result");
    let report = result
        .as_ref()
        .and_then(result_files::report_text)
        .unwrap_or("");
    let target = b.document.as_deref().unwrap_or(&b.href);
    let anchor = result_files::linked(report, target);
    let image = b.document.is_none() && result_files::image_linked(report, target);
    if b.href.len() > 8192
        || target.len() > 8192
        || !result_files::local_link(&b.href)
        || !result_files::local_link(target)
        || !(anchor || image)
    {
        return Err(Error::bad("link is not present in this execution result"));
    }
    let runner: Uuid = row.get("runner_id");
    let snapshot: Option<Value> = row.get("execution_snapshot");
    let cwd = snapshot.as_ref().and_then(|s| s["cwd"].as_str());
    let document = if let Some(href) = &b.document {
        let value = crate::api::device_query(
            &s,
            &h,
            workspace,
            runner,
            "read_result_file",
            json!({"href":href,"cwd":cwd}),
            10,
        )
        .await?;
        if value["format"] != "markdown"
            || value["truncated"] != false
            || !value["content"]
                .as_str()
                .is_some_and(|text| result_files::image_linked(text, &b.href))
        {
            return Err(Error::bad(
                "image is not present in the complete Markdown document",
            ));
        }
        Some(value)
    } else {
        None
    };
    let cwd = if let Some(value) = &document {
        Some(
            value["path"]
                .as_str()
                .and_then(result_files::document_directory)
                .ok_or_else(|| Error::bad("Markdown document directory is unavailable"))?,
        )
    } else {
        cwd
    };
    let mut value = crate::api::device_query(
        &s,
        &h,
        workspace,
        runner,
        "read_result_file",
        json!({"href":b.href,"cwd":cwd,"image":b.image,"offset":b.offset,"revision":b.revision}),
        10,
    )
    .await?;
    if (document.is_some() || !anchor) && value["format"] != "image" {
        return Err(Error::bad("referenced file is not a supported image"));
    }
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
