use crate::{config::Schedule, schedule, store};
use axum::{Json, extract::State, http::HeaderMap};
use chrono::Utc;
use serde_json::{Value, json};

use crate::api::access::{user, workspace_header};
use crate::api::http::{ApiJson, Error, Result};
use crate::auth::workspace::reauthorize;
use crate::state::App;

pub(crate) async fn preview(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(mut b): ApiJson<Schedule>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    if b.timezone.is_empty() {
        let settings: Value = sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&s.pool)
            .await?;
        b.timezone = settings["timezone"]
            .as_str()
            .unwrap_or("Asia/Shanghai")
            .into();
    }
    Ok(Json(json!(
        schedule::preview(&b, Utc::now()).map_err(|e| Error::bad(e.to_string()))?
    )))
}

pub(crate) async fn timezones(State(s): State<App>, h: HeaderMap) -> Result<Json<Vec<String>>> {
    user(&s, &h, "viewer", false).await?;
    let mut names: Vec<String> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    names.sort();
    names.dedup();
    Ok(Json(names))
}

pub(crate) async fn settings(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&s.pool)
            .await?,
    ))
}

pub(crate) async fn update_settings(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "admin", true).await?;
    let _: chrono_tz::Tz = b["timezone"]
        .as_str()
        .unwrap_or("")
        .parse()
        .map_err(|_| Error::bad("invalid timezone"))?;
    for k in ["log_retention_days", "run_retention_days"] {
        if !b[k].as_i64().is_some_and(|v| (1..=3650).contains(&v)) {
            return Err(Error::bad("retention must be 1–3650 days"));
        }
    }
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "admin").await?;
    sqlx::query("UPDATE settings SET data=$1 WHERE workspace_id=$2")
        .bind(&b)
        .bind(workspace)
        .execute(&mut *tx)
        .await?;
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        "settings.update",
        "workspace",
    )
    .await?;
    tx.commit().await?;
    Ok(Json(b))
}

pub(crate) async fn audits(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "admin", false).await?;
    Ok(Json(json!(
        sqlx::query_scalar::<_, Value>(
            "SELECT to_jsonb(a.*) FROM audit a WHERE workspace_id=$1 ORDER BY id DESC LIMIT 100"
        )
        .bind(workspace)
        .fetch_all(&s.pool)
        .await?
    )))
}
