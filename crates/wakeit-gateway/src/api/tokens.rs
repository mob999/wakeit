use crate::auth;
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::access::{user, workspace_header};
use crate::api::http::{ApiJson, Error, PageQuery, Path, Result, pagination};
use crate::auth::workspace::reauthorize;
use crate::state::App;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TokenBody {
    name: String,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

pub(crate) async fn create_token(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<TokenBody>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "viewer", true).await?;
    let name = b.name.trim();
    if name.is_empty() {
        return Err(Error::bad("API key name required"));
    }
    if b.expires_at
        .is_some_and(|expires_at| expires_at <= Utc::now())
    {
        return Err(Error::bad("API key expiration must be in the future"));
    }
    let mut tx = s.pool.begin().await?;
    crate::store::workspaces::lock(&mut tx, workspace).await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(u.id)
        .fetch_one(&mut *tx)
        .await?;
    reauthorize(&mut tx, &u, "viewer").await?;
    let t = auth::credentials::secret();
    sqlx::query(
        "INSERT INTO credentials(hash,user_id,kind,name,expires_at,workspace_id) VALUES($1,$2,'api',$3,$4,$5)",
    )
    .bind(auth::credentials::hash(&t))
    .bind(u.id)
    .bind(name)
    .bind(b.expires_at)
    .bind(workspace).execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"token":t})))
}

pub(crate) async fn tokens(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "viewer", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value=sqlx::query_scalar(
        "WITH filtered AS (SELECT hash AS id,name,created_at,expires_at FROM credentials WHERE workspace_id=$6 AND user_id=$1 AND kind='api'), paged AS (SELECT * FROM filtered ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY created_at DESC,id DESC) FROM paged p),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$4::bigint,'page_size',$5::bigint)"
    ).bind(u.id).bind(q.page.map(|_|size)).bind(if q.page.is_some(){(page-1)*size}else{0}).bind(page).bind(size).bind(workspace).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}

pub(crate) async fn delete_token(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "viewer", true).await?;
    sqlx::query(
        "DELETE FROM credentials WHERE workspace_id=$3 AND hash=$1 AND user_id=$2 AND kind='api'",
    )
    .bind(id)
    .bind(u.id)
    .bind(workspace)
    .execute(&s.pool)
    .await?;
    Ok(Json(json!({"ok":true})))
}
