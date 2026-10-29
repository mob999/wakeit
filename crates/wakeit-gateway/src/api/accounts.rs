use crate::{auth, store};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::api::access::user;
use crate::api::http::{ApiJson, Error, PageQuery, Path, Result, pagination};
use crate::auth::workspace::reauthorize;
use crate::state::App;

pub(crate) async fn users(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "admin", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH paged AS (SELECT id,username,instance_admin,active,password_hash IS NOT NULL AS has_password,created_at FROM users ORDER BY created_at,id LIMIT $1 OFFSET $2) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY created_at,id) FROM paged p),'[]'::jsonb),'total',(SELECT count(*) FROM users),'active_admins',(SELECT count(*) FROM users WHERE active AND instance_admin),'page',$3::bigint,'page_size',$4::bigint)"
    ).bind(q.page.map(|_|size)).bind(if q.page.is_some(){(page-1)*size}else{0}).bind(page).bind(size).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateUser {
    username: String,
    password: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PasswordBody {
    password: String,
}

pub(crate) async fn password_hash(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || auth::credentials::password(&password))
        .await?
        .map_err(|e| Error::bad(e.to_string()))
}

pub(crate) async fn create_user(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<CreateUser>,
) -> Result<Json<Value>> {
    if s.config.auth.mode != "local" {
        return Err(Error::bad("local account creation is disabled"));
    }
    let initial = user(&s, &h, "admin", true).await?;
    let name = b.username.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(Error::bad("invalid username or role"));
    }
    let hash = password_hash(b.password).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let actor = reauthorize(&mut tx, &initial, "admin").await?;
    let id = Uuid::new_v4();
    let row: Value=sqlx::query_scalar("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,$4) ON CONFLICT(username) DO NOTHING RETURNING jsonb_build_object('id',id,'username',username,'role',role,'active',active,'created_at',created_at)").bind(id).bind(name).bind(hash).bind("viewer").fetch_optional(&mut *tx).await?.ok_or_else(||Error::conflict("username unavailable"))?;
    store::audit(&mut tx, actor.id, None, "member.create", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(row))
}

pub(crate) async fn reset_password(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<PasswordBody>,
) -> Result<Json<Value>> {
    if s.config.auth.mode != "local" {
        return Err(Error::bad("password management is disabled"));
    }
    let initial = user(&s, &h, "admin", true).await?;
    let hash = password_hash(b.password).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let actor = reauthorize(&mut tx, &initial, "admin").await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 AND password_hash IS NOT NULL FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    sqlx::query("UPDATE users SET password_hash=$2 WHERE id=$1")
        .bind(id)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM credentials WHERE user_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    store::audit(&mut tx, actor.id, None, "member.password", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct MemberUpdate {
    role: Option<String>,
    active: Option<bool>,
}

pub(crate) async fn update_user(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<MemberUpdate>,
) -> Result<Json<Value>> {
    let initial = user(&s, &h, "admin", true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let u = reauthorize(&mut tx, &initial, "admin").await?;
    if b.role.is_some() {
        return Err(Error::bad("roles belong to workspace memberships"));
    }
    // Lock every workspace in the same order as member operations before checking
    // whether global deactivation would strand any organization without an Admin.
    sqlx::query("SELECT id FROM workspaces ORDER BY id FOR NO KEY UPDATE")
        .fetch_all(&mut *tx)
        .await?;
    let current = sqlx::query("SELECT active,instance_admin FROM users WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    let active = b.active.unwrap_or(current.get("active"));
    if !active && current.get::<bool, _>("active") {
        let stranded:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspace_members mine WHERE mine.user_id=$1 AND mine.active AND mine.role='admin' AND NOT EXISTS(SELECT 1 FROM workspace_members other JOIN users u ON u.id=other.user_id WHERE other.workspace_id=mine.workspace_id AND other.user_id<>$1 AND other.active AND other.role='admin' AND u.active))").bind(id).fetch_one(&mut *tx).await?;
        let last_instance: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS(SELECT 1 FROM users WHERE id<>$1 AND instance_admin AND active)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if stranded || (current.get::<bool, _>("instance_admin") && last_instance) {
            return Err(Error::conflict("cannot remove the last administrator"));
        }
    }
    sqlx::query("UPDATE users SET active=$2 WHERE id=$1")
        .bind(id)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    if !active {
        sqlx::query("DELETE FROM credentials WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE device_grants SET state='denied' WHERE user_id=$1 AND state IN ('pending','approved')").bind(id).execute(&mut *tx).await?;
    }
    store::audit(&mut tx, u.id, None, "member.update", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
