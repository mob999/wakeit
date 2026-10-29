use crate::auth;
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::api::access::user;
use crate::api::http::{ApiJson, Error, Result};
use crate::state::App;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Login {
    username: String,
    password: String,
}

pub(crate) async fn login(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Login>,
) -> Result<Response> {
    if s.config.auth.mode != "local" {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "password login is disabled".into(),
        ));
    }
    if b.username.len() > 100 || b.password.len() > 1024 {
        return Err(Error::bad("invalid credentials"));
    }
    // Shared database limit applies across Gateway instances.
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,1))")
        .bind(&b.username)
        .execute(&mut *tx)
        .await?;
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM login_attempts WHERE username=$1 AND created_at>now()-interval '15 minutes'").bind(&b.username).fetch_one(&mut *tx).await?;
    if count >= 20 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts; try again later".into(),
        ));
    }
    sqlx::query("INSERT INTO login_attempts(username) VALUES($1)")
        .bind(&b.username)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let row = sqlx::query("SELECT id,password_hash FROM users WHERE username=$1 AND active")
        .bind(&b.username)
        .fetch_optional(&s.pool)
        .await?;
    let Some(row) = row else {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    };
    let Some(encoded) = row.get::<Option<String>, _>("password_hash") else {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    };
    let checked_hash = encoded.clone();
    let valid =
        tokio::task::spawn_blocking(move || auth::credentials::verify(&b.password, &encoded))
            .await?;
    if !valid {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    }
    // Prevent login CSRF as well as authenticated write CSRF.
    if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && origin
            != reqwest::Url::parse(&s.config.public_url)?
                .origin()
                .ascii_serialization()
    {
        return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
    }
    let mut tx = s.pool.begin().await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 AND active FOR SHARE")
            .bind(row.get::<Uuid, _>("id"))
            .fetch_optional(&mut *tx)
            .await?;
    if current.as_deref() != Some(checked_hash.as_str()) {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid credentials".into(),
        ));
    }
    let token = auth::credentials::secret();
    sqlx::query("INSERT INTO credentials(hash,user_id,kind,expires_at) VALUES($1,$2,'session',now()+interval '7 days')").bind(auth::credentials::hash(&token)).bind(row.get::<Uuid,_>("id")).execute(&mut *tx).await?;
    tx.commit().await?;
    let secure = if s.config.public_url.starts_with("https:") {
        "; Secure"
    } else {
        ""
    };
    Ok((
        [(
            header::SET_COOKIE,
            format!(
                "{}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{secure}",
                s.config.auth.session_cookie
            ),
        )],
        Json(json!({"ok":true,"token":token})),
    )
        .into_response())
}

pub(crate) async fn me(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    Ok(Json(
        json!({"id":u.id,"username":u.username,"role":u.role,"instance_admin":u.instance_admin,"kind":u.kind,"workspace_id":u.workspace_id}),
    ))
}

pub(crate) async fn logout(State(s): State<App>, h: HeaderMap) -> Result<Response> {
    let u = user(&s, &h, "viewer", true).await?;
    sqlx::query("DELETE FROM credentials WHERE hash=$1")
        .bind(u.credential)
        .execute(&s.pool)
        .await?;
    Ok((
        [(
            header::SET_COOKIE,
            format!(
                "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
                s.config.auth.session_cookie
            ),
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
