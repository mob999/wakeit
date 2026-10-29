use crate::config::TaskConfig;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::Row;

use crate::api::access::user;
use crate::api::http::{Error, Path, Result};
use crate::state::App;

pub(crate) async fn ready(State(s): State<App>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&s.pool).await?;
    s.js.get_stream("WAKEIT_WORK")
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Json(json!({"status":"ready"})))
}

pub(crate) async fn schema(Path(kind): Path<String>) -> Result<Json<Value>> {
    Ok(Json(match kind.as_str() {
        "agent" => serde_json::to_value(schemars::schema_for!(crate::config::AgentConfig))?,
        "task" => serde_json::to_value(schemars::schema_for!(TaskConfig))?,
        "template" => {
            serde_json::to_value(schemars::schema_for!(crate::task_templates::Definition))?
        }
        _ => return Err(Error::missing()),
    }))
}

pub(crate) async fn openapi() -> Json<Value> {
    Json(crate::api::openapi::document())
}

pub(crate) async fn metrics(State(s): State<App>, h: HeaderMap) -> Result<Response> {
    user(&s, &h, "admin", false).await?;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox WHERE published_at IS NULL")
        .fetch_one(&s.pool)
        .await?;
    let offline:i64=sqlx::query_scalar("SELECT count(*) FROM runners WHERE NOT revoked AND (last_seen IS NULL OR last_seen<now()-interval '60 seconds')").fetch_one(&s.pool).await?;
    let rows = sqlx::query("SELECT state,count(*) AS count FROM runs GROUP BY state")
        .fetch_all(&s.pool)
        .await?;
    let delay:f64=sqlx::query_scalar("SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(next_at)),0)::float8 FROM tasks WHERE enabled AND archived_at IS NULL AND next_at<now()").fetch_one(&s.pool).await?;
    let mut body = format!(
        "wakeit_outbox_pending {pending}\nwakeit_runners_offline {offline}\nwakeit_scheduler_delay_seconds {delay}\n"
    );
    for r in rows {
        body.push_str(&format!(
            "wakeit_runs{{state=\"{}\"}} {}\n",
            r.get::<String, _>("state"),
            r.get::<i64, _>("count")
        ));
    }
    Ok(([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body).into_response())
}

pub(crate) async fn meta(State(s): State<App>) -> Json<Value> {
    let version = env!("CARGO_PKG_VERSION");
    Json(
        json!({"version":version,"public_url":s.config.public_url,"auth":{"mode":s.config.auth.mode,"allow_registration":s.config.auth.allow_registration}}),
    )
}
