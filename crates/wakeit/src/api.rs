use crate::{
    auth, bus,
    config::{GatewayConfig, Schedule, TaskConfig},
    nats_auth, protocol, schedule, store,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::{convert::Infallible, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
pub struct App {
    pub pool: sqlx::PgPool,
    pub js: async_nats::jetstream::Context,
    pub client: async_nats::Client,
    pub config: Arc<GatewayConfig>,
    pub authority: nats_auth::Authority,
    pub shutdown: tokio_util::sync::CancellationToken,
}
pub struct Error(pub StatusCode, pub String);
impl Error {
    pub(crate) fn bad(s: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, s.into())
    }
    pub(crate) fn conflict(s: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, s.into())
    }
    fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "not found".into())
    }
}
impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        let e = e.into();
        tracing::error!(error=%e,"request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error".into(),
        )
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
type Result<T> = std::result::Result<T, Error>;
struct ApiJson<T>(T);
impl<S, T> axum::extract::FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: serde::de::DeserializeOwned,
{
    type Rejection = Error;
    async fn from_request(
        request: axum::extract::Request,
        state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(request, state)
            .await
            .map_err(|e| Error(e.status(), e.body_text()))?;
        Ok(Self(value))
    }
}
#[derive(Clone)]
pub struct User {
    pub id: Uuid,
    pub role: String,
    pub username: String,
    pub credential: String,
}
pub(crate) fn cookie(h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| s.trim().strip_prefix("wakeit_session=").map(str::to_owned))
}
pub(crate) async fn user(s: &App, h: &HeaderMap, needed: &str, write: bool) -> Result<User> {
    let bearer = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);
    if bearer.is_none() && write {
        if h.get("x-wakeit-request").and_then(|v| v.to_str().ok()) != Some("1") {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "missing request header".into(),
            ));
        }
        if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            let allowed = reqwest::Url::parse(&s.config.public_url)?
                .origin()
                .ascii_serialization();
            if origin != allowed {
                return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
            }
        }
    }
    let token = bearer
        .or_else(|| cookie(h))
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let hash = auth::hash(&token);
    let row=sqlx::query("SELECT u.id,u.username,u.role FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND u.active AND (c.expires_at IS NULL OR c.expires_at>now())").bind(&hash).fetch_optional(&s.pool).await?.ok_or_else(||Error(StatusCode::UNAUTHORIZED,"session expired or token invalid".into()))?;
    let role: String = row.get("role");
    if auth::role(&role) < auth::role(needed) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "insufficient permissions".into(),
        ));
    }
    Ok(User {
        id: row.get("id"),
        role,
        username: row.get("username"),
        credential: hash,
    })
}
// Recheck permissions on the transaction's connection after acquiring its locks.
// Acquiring another pool connection here can deadlock under concurrent member updates.
pub(crate) async fn reauthorize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    credential: &str,
    needed: &str,
) -> Result<User> {
    let row=sqlx::query("SELECT u.id,u.username,u.role FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND u.active AND (c.expires_at IS NULL OR c.expires_at>now())").bind(credential).fetch_optional(&mut **tx).await?.ok_or_else(||Error(StatusCode::UNAUTHORIZED,"session expired or token invalid".into()))?;
    let role: String = row.get("role");
    if auth::role(&role) < auth::role(needed) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "insufficient permissions".into(),
        ));
    }
    Ok(User {
        id: row.get("id"),
        username: row.get("username"),
        role,
        credential: credential.into(),
    })
}
async fn device(s: &App, h: &HeaderMap) -> Result<Uuid> {
    let token = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "device token required".into()))?;
    sqlx::query_scalar("SELECT id FROM runners WHERE token_hash=$1 AND NOT revoked")
        .bind(auth::hash(token))
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(|| {
            Error(
                StatusCode::UNAUTHORIZED,
                "device revoked or token invalid".into(),
            )
        })
}
pub fn router(app: App) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/metrics", get(metrics))
        .merge(crate::device_auth::routes())
        .route("/api/v1/meta", get(meta))
        .route("/api/v1/runners/{id}/directories", get(directories))
        .route(
            "/api/v1/runners/{id}/agents/{agent}/models/query",
            post(query_models),
        )
        .route(
            "/api/v1/runners/{id}/agents/{agent}/sessions/inspect",
            post(inspect_session),
        )
        .route(
            "/api/v1/runners/{id}/agents/{agent}/sessions/query",
            post(query_sessions),
        )
        .route("/api/v1/session", post(login).get(me).delete(logout))
        .route("/api/v1/invites/accept", post(accept_invite))
        .route("/api/v1/invites", post(invite))
        .route("/api/v1/users", get(users).post(create_user))
        .route("/api/v1/users/{id}", put(update_user))
        .route("/api/v1/users/{id}/password", post(reset_password))
        .route("/api/v1/tokens", get(tokens).post(create_token))
        .route("/api/v1/tokens/{id}", delete(delete_token))
        .route("/api/v1/tasks", get(tasks))
        .route("/api/v1/tasks/{id}", get(task).put(apply))
        .route("/api/v1/tasks/{id}/runs", post(trigger))
        .route("/api/v1/tasks/{id}/archive", post(archive_task))
        .route("/api/v1/tasks/{id}/restore", post(restore_task))
        .route("/api/v1/schedule/preview", post(preview))
        .route("/api/v1/moments/days", get(crate::moments::days))
        .route("/api/v1/moments", get(crate::moments::entries))
        .route("/api/v1/runs", get(runs))
        .route("/api/v1/runs/{id}", get(run))
        .route("/api/v1/runs/{id}/cancel", post(cancel))
        .route("/api/v1/runs/{id}/retry", post(retry))
        .route("/api/v1/runs/{id}/resolve", post(resolve))
        .route("/api/v1/runs/{id}/events", get(events))
        .route("/api/v1/runs/{id}/stream", get(stream))
        .route("/api/v1/runners", get(runners))
        .route("/api/v1/runners/enrollment", post(enrollment))
        .route("/api/v1/runners/{id}/revoke", post(revoke))
        .route("/api/v1/agents", get(agents))
        .route("/api/v1/agents/templates/{id}", get(template))
        .route("/api/v1/device/enroll", post(enroll))
        .route("/api/v1/device/hello", post(hello))
        .route("/api/v1/device/heartbeat", post(heartbeat))
        .route("/api/v1/device/claim", post(claim))
        .route("/api/v1/settings", get(settings).put(update_settings))
        .route("/api/v1/timezones", get(timezones))
        .route("/api/v1/audit", get(audits))
        .route("/api/v1/schema/{kind}", get(schema))
        .route("/api/v1/openapi.json", get(openapi))
        .fallback(static_file)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(app)
}
async fn ready(State(s): State<App>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&s.pool).await?;
    s.js.get_stream("WAKEIT_WORK")
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Json(json!({"status":"ready"})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Login {
    username: String,
    password: String,
}
async fn login(State(s): State<App>, h: HeaderMap, ApiJson(b): ApiJson<Login>) -> Result<Response> {
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
    let encoded: String = row.get("password_hash");
    let checked_hash = encoded.clone();
    let valid = tokio::task::spawn_blocking(move || auth::verify(&b.password, &encoded)).await?;
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
    let token = auth::secret();
    sqlx::query("INSERT INTO credentials(hash,user_id,kind,expires_at) VALUES($1,$2,'session',now()+interval '7 days')").bind(auth::hash(&token)).bind(row.get::<Uuid,_>("id")).execute(&mut *tx).await?;
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
                "wakeit_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{secure}"
            ),
        )],
        Json(json!({"ok":true,"token":token})),
    )
        .into_response())
}
async fn me(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    Ok(Json(json!({"id":u.id,"username":u.username,"role":u.role})))
}
async fn logout(State(s): State<App>, h: HeaderMap) -> Result<Response> {
    let u = user(&s, &h, "viewer", true).await?;
    sqlx::query("DELETE FROM credentials WHERE hash=$1")
        .bind(u.credential)
        .execute(&s.pool)
        .await?;
    Ok((
        [(
            header::SET_COOKIE,
            "wakeit_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        Json(json!({"ok":true})),
    )
        .into_response())
}
#[derive(Default, Deserialize)]
struct PageQuery {
    page: Option<i64>,
    page_size: Option<i64>,
}
async fn users(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "admin", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH paged AS (SELECT id,username,role,active,created_at FROM users ORDER BY created_at,id LIMIT $1 OFFSET $2) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY created_at,id) FROM paged p),'[]'::jsonb),'total',(SELECT count(*) FROM users),'active_admins',(SELECT count(*) FROM users WHERE active AND role='admin'),'page',$3::bigint,'page_size',$4::bigint)"
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
    role: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct PasswordBody {
    password: String,
}
async fn password_hash(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || auth::password(&password))
        .await?
        .map_err(|e| Error::bad(e.to_string()))
}
async fn create_user(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<CreateUser>,
) -> Result<Json<Value>> {
    let initial = user(&s, &h, "admin", true).await?;
    let name = b.username.trim();
    if name.is_empty() || name.len() > 100 || auth::role(&b.role) == 0 {
        return Err(Error::bad("invalid username or role"));
    }
    let hash = password_hash(b.password).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let actor = reauthorize(&mut tx, &initial.credential, "admin").await?;
    let id = Uuid::new_v4();
    let row: Value=sqlx::query_scalar("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,$4) ON CONFLICT(username) DO NOTHING RETURNING jsonb_build_object('id',id,'username',username,'role',role,'active',active,'created_at',created_at)").bind(id).bind(name).bind(hash).bind(b.role).fetch_optional(&mut *tx).await?.ok_or_else(||Error::conflict("username unavailable"))?;
    store::audit(&mut tx, actor.id, "member.create", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(row))
}
async fn reset_password(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<PasswordBody>,
) -> Result<Json<Value>> {
    let initial = user(&s, &h, "admin", true).await?;
    let hash = password_hash(b.password).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let actor = reauthorize(&mut tx, &initial.credential, "admin").await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
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
    store::audit(&mut tx, actor.id, "member.password", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RoleBody {
    role: String,
}
fn true_value() -> bool {
    true
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct MemberUpdate {
    role: Option<String>,
    active: Option<bool>,
}
async fn update_user(
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
    let u = reauthorize(&mut tx, &initial.credential, "admin").await?;
    let current = sqlx::query("SELECT role,active FROM users WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(Error::missing)?;
    let role = b.role.unwrap_or_else(|| current.get("role"));
    let active = b.active.unwrap_or_else(|| current.get("active"));
    if auth::role(&role) == 0 {
        return Err(Error::bad("invalid role"));
    }
    if current.get::<String, _>("role") == "admin"
        && current.get::<bool, _>("active")
        && (role != "admin" || !active)
    {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM users WHERE role='admin' AND active AND id<>$1",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if count == 0 {
            return Err(Error::conflict("cannot remove the last administrator"));
        }
    }
    sqlx::query("UPDATE users SET role=$2,active=$3 WHERE id=$1")
        .bind(id)
        .bind(role)
        .bind(active)
        .execute(&mut *tx)
        .await?;
    if !active {
        sqlx::query("DELETE FROM credentials WHERE user_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    store::audit(&mut tx, u.id, "member.update", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
async fn invite(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<RoleBody>,
) -> Result<Json<Value>> {
    user(&s, &h, "admin", true).await?;
    if auth::role(&b.role) == 0 {
        return Err(Error::bad("invalid role"));
    }
    let token = auth::secret();
    sqlx::query("INSERT INTO invites(hash,role,expires_at) VALUES($1,$2,now()+interval '2 days')")
        .bind(auth::hash(&token))
        .bind(b.role)
        .execute(&s.pool)
        .await?;
    Ok(Json(
        json!({"token":token,"url":format!("{}/invite#{}",s.config.public_url.trim_end_matches('/'),token)}),
    ))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Accept {
    token: String,
    username: String,
    password: String,
}
async fn accept_invite(State(s): State<App>, ApiJson(b): ApiJson<Accept>) -> Result<Json<Value>> {
    if b.username.trim().is_empty() || b.username.len() > 100 {
        return Err(Error::bad("invalid username"));
    }
    let pw = tokio::task::spawn_blocking(move || auth::password(&b.password))
        .await?
        .map_err(|e| Error::bad(e.to_string()))?;
    let mut tx = s.pool.begin().await?;
    let role:Option<String>=sqlx::query_scalar("UPDATE invites SET used_at=now() WHERE hash=$1 AND used_at IS NULL AND expires_at>now() RETURNING role").bind(auth::hash(&b.token)).fetch_optional(&mut *tx).await?;
    let role = role.ok_or_else(|| Error::bad("invitation invalid or expired"))?;
    sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,$4)")
        .bind(Uuid::new_v4())
        .bind(b.username)
        .bind(pw)
        .bind(role)
        .execute(&mut *tx)
        .await
        .map_err(|_| Error::conflict("username unavailable"))?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct TokenBody {
    name: String,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}
async fn create_token(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<TokenBody>,
) -> Result<Json<Value>> {
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
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(u.id)
        .fetch_one(&mut *tx)
        .await?;
    reauthorize(&mut tx, &u.credential, "viewer").await?;
    let t = auth::secret();
    sqlx::query(
        "INSERT INTO credentials(hash,user_id,kind,name,expires_at) VALUES($1,$2,'api',$3,$4)",
    )
    .bind(auth::hash(&t))
    .bind(u.id)
    .bind(name)
    .bind(b.expires_at)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"token":t})))
}
async fn tokens(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value=sqlx::query_scalar(
        "WITH filtered AS (SELECT hash AS id,name,created_at,expires_at FROM credentials WHERE user_id=$1 AND kind='api'), paged AS (SELECT * FROM filtered ORDER BY created_at DESC,id DESC LIMIT $2 OFFSET $3) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(p) ORDER BY created_at DESC,id DESC) FROM paged p),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$4::bigint,'page_size',$5::bigint)"
    ).bind(u.id).bind(q.page.map(|_|size)).bind(if q.page.is_some(){(page-1)*size}else{0}).bind(page).bind(size).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}
async fn delete_token(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", true).await?;
    sqlx::query("DELETE FROM credentials WHERE hash=$1 AND user_id=$2 AND kind='api'")
        .bind(id)
        .bind(u.id)
        .execute(&s.pool)
        .await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Default, Deserialize)]
struct ListTasks {
    #[serde(default)]
    archived: bool,
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    runner: Option<String>,
    agent: Option<String>,
    enabled: Option<bool>,
}
fn pagination(page: Option<i64>, size: Option<i64>) -> Result<(i64, i64)> {
    let page = page.unwrap_or(1);
    let size = size.unwrap_or(20);
    if !(1..=1_000_000).contains(&page) || !(1..=100).contains(&size) {
        return Err(Error::bad("invalid page or page_size"));
    }
    Ok((page, size))
}
fn search_pattern(q: Option<String>) -> Result<Option<String>> {
    let Some(q) = q else { return Ok(None) };
    if q.len() > 640 {
        return Err(Error::bad("search query too long"));
    }
    let escaped = q
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    Ok(Some(format!(
        "%{}%",
        escaped.split_whitespace().collect::<Vec<_>>().join("%")
    )))
}
async fn tasks(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<ListTasks>,
) -> Result<Json<Value>> {
    user(
        &s,
        &h,
        if q.archived { "operator" } else { "viewer" },
        false,
    )
    .await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH filtered AS (SELECT t.* FROM tasks t WHERE (archived_at IS NOT NULL)=$9 AND ($1::text IS NULL OR concat_ws(' ',config->>'name',id,config->>'prompt',config->>'cwd',config->>'runner',config->>'agent') ILIKE $1) AND ($2::text IS NULL OR config->>'runner'=$2) AND ($3::text IS NULL OR config->>'agent'=$3) AND ($4::boolean IS NULL OR enabled=$4)), paged AS (SELECT to_jsonb(t.*) || jsonb_build_object('last_run',(SELECT jsonb_build_object('id',id,'state',state,'created_at',created_at) FROM runs WHERE task_id=t.id ORDER BY created_at DESC,id DESC LIMIT 1)) AS item FROM filtered t ORDER BY updated_at DESC,id DESC LIMIT $5 OFFSET $6) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(item) FROM paged),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$7::bigint,'page_size',$8::bigint)"
    ).bind(search_pattern(q.q)?).bind(q.runner).bind(q.agent).bind(q.enabled)
    .bind(q.page.map(|_| size)).bind(if q.page.is_some() { (page-1)*size } else {0})
    .bind(page).bind(size).bind(q.archived).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}
async fn task(State(s): State<App>, h: HeaderMap, Path(id): Path<String>) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT to_jsonb(t.*) FROM tasks t WHERE id=$1")
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(Error::missing)?,
    ))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Apply {
    config: TaskConfig,
    expected_revision: Option<i64>,
}
async fn apply(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<Apply>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    if b.config.id != id {
        return Err(Error::bad("task id does not match URL"));
    }
    b.config.validate().map_err(|e| Error::bad(e.to_string()))?;
    Ok(Json(
        store::apply_task(&s.pool, &b.config, b.expected_revision, u.id)
            .await
            .map_err(|e| Error::bad(e.to_string()))?
            .ok_or_else(|| Error::conflict("task changed; reload before saving"))?,
    ))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ArchiveBody {
    expected_revision: i64,
}
async fn archive_task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<ArchiveBody>,
) -> Result<Json<Value>> {
    archive_change(s, h, id, b, true).await
}
async fn restore_task(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
    ApiJson(b): ApiJson<ArchiveBody>,
) -> Result<Json<Value>> {
    archive_change(s, h, id, b, false).await
}
async fn archive_change(
    s: App,
    h: HeaderMap,
    id: String,
    b: ArchiveBody,
    archived: bool,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    Ok(Json(
        store::set_archived(&s.pool, &id, b.expected_revision, archived, u.id)
            .await
            .map_err(|e| Error::conflict(e.to_string()))?
            .ok_or_else(Error::missing)?,
    ))
}
fn request_key(h: &HeaderMap) -> Result<String> {
    let k = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Error::bad("Idempotency-Key required"))?;
    if k.len() > 160 || k.is_empty() {
        return Err(Error::bad("invalid Idempotency-Key"));
    }
    Ok(k.into())
}
async fn trigger(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    let id = store::trigger(&s.pool, &id, u.id, None, &request_key(&h)?)
        .await
        .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"id":id})))
}
async fn retry(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    let task: String = sqlx::query_scalar("SELECT task_id FROM runs WHERE id=$1")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(Error::missing)?;
    let id = store::trigger(&s.pool, &task, u.id, Some(id), &request_key(&h)?)
        .await
        .map_err(|e| Error::conflict(e.to_string()))?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize, schemars::JsonSchema)]
struct ListRuns {
    task: Option<String>,
    state: Option<String>,
    before: Option<DateTime<Utc>>,
    page: Option<i64>,
    page_size: Option<i64>,
    q: Option<String>,
    runner: Option<String>,
    agent: Option<String>,
}
async fn runs(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<ListRuns>,
) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    let (page, size) = pagination(q.page, q.page_size)?;
    let result: Value = sqlx::query_scalar(
        "WITH filtered AS (SELECT r.*,EXISTS(SELECT 1 FROM tasks t WHERE t.id=r.task_id AND t.archived_at IS NOT NULL) AS task_archived FROM runs r WHERE ($1::text IS NULL OR task_id=$1) AND ($2::text IS NULL OR state=$2) AND ($3::timestamptz IS NULL OR created_at<$3) AND ($4::text IS NULL OR concat_ws(' ',id::text,task_id,config->>'name',config->>'prompt',config->>'cwd',config->>'runner',config->>'agent') ILIKE $4) AND ($5::text IS NULL OR config->>'runner'=$5) AND ($6::text IS NULL OR config->>'agent'=$6)), paged AS (SELECT to_jsonb(r.*) AS item FROM filtered r ORDER BY created_at DESC,id DESC LIMIT $7 OFFSET $8) SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(item) FROM paged),'[]'::jsonb),'total',(SELECT count(*) FROM filtered),'page',$9::bigint,'page_size',$10::bigint)"
    ).bind(q.task).bind(q.state).bind(q.before).bind(search_pattern(q.q)?).bind(q.runner).bind(q.agent)
    .bind(if q.page.is_some() {size} else {100}).bind(if q.page.is_some() {(page-1)*size} else {0})
    .bind(page).bind(size).fetch_one(&s.pool).await?;
    Ok(Json(if q.page.is_some() {
        result
    } else {
        result["items"].clone()
    }))
}
async fn run(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT to_jsonb(r.*)||jsonb_build_object('task_archived',t.archived_at IS NOT NULL) FROM runs r JOIN tasks t ON t.id=r.task_id WHERE r.id=$1")
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(Error::missing)?,
    ))
}
async fn cancel(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "SELECT t.id FROM tasks t JOIN runs r ON r.task_id=t.id WHERE r.id=$1 FOR UPDATE OF t",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(Error::missing)?;
    let row = sqlx::query("SELECT state,runner_id FROM runs WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let state: String = row.get("state");
    if !protocol::terminal(&state) {
        sqlx::query("UPDATE runs SET cancel_requested=true,state=CASE WHEN state='queued' THEN 'cancelled' ELSE state END,finished_at=CASE WHEN state='queued' THEN now() ELSE finished_at END WHERE id=$1").bind(id).execute(&mut *tx).await?;
        store::enqueue(
            &mut tx,
            protocol::control_subject(row.get("runner_id")),
            format!("cancel:{id}"),
            json!({"version":1,"run_id":id,"action":"cancel"}),
        )
        .await?;
    }
    store::audit(&mut tx, u.id, "run.cancel", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Resolution {
    state: String,
    reason: String,
}
async fn resolve(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    ApiJson(b): ApiJson<Resolution>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    if !matches!(
        b.state.as_str(),
        "failed" | "cancelled" | "succeeded" | "interrupted"
    ) || b.reason.trim().is_empty()
    {
        return Err(Error::bad("final state and verification reason required"));
    }
    let mut tx = s.pool.begin().await?;
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
    store::audit(&mut tx, u.id, "run.resolve", &id.to_string()).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, schemars::JsonSchema)]
struct Cursor {
    #[serde(default)]
    after: i64,
    kind: Option<String>,
}
async fn event_rows(
    s: &App,
    id: Uuid,
    after: i64,
    kind: Option<&str>,
) -> std::result::Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(e.*) FROM events e WHERE run_id=$1 AND id>$2 AND ($3::text IS NULL OR kind=$3 OR ($3='trace' AND kind='trace_status')) ORDER BY id LIMIT 500",
    )
    .bind(id)
    .bind(after)
    .bind(kind)
    .fetch_all(&s.pool)
    .await
}
async fn events(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(
        event_rows(&s, id, q.after, q.kind.as_deref()).await?
    )))
}
async fn stream(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<impl IntoResponse> {
    user(&s, &h, "viewer", false).await?;
    let mut cursor = h
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(q.after);
    let output = async_stream::stream! {let mut tick=tokio::time::interval(Duration::from_secs(1));loop{tick.tick().await;
        if s.shutdown.is_cancelled() || user(&s,&h,"viewer",false).await.is_err(){break}
        match event_rows(&s,id,cursor,None).await {Ok(rows)=>for row in rows {cursor=row["id"].as_i64().unwrap_or(cursor);yield Ok::<_,Infallible>(Event::default().id(cursor.to_string()).event("run_event").data(row.to_string()));},Err(e)=>{tracing::warn!(error=%e,"SSE read failed");break}}
    }};
    Ok(Sse::new(output).keep_alive(KeepAlive::default()))
}
async fn preview(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(mut b): ApiJson<Schedule>,
) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    if b.timezone.is_empty() {
        let settings: Value = sqlx::query_scalar("SELECT data FROM settings")
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
async fn runners(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'name',name,'revoked',revoked,'last_seen',last_seen,'online',NOT revoked AND last_seen>now()-interval '60 seconds','agents',agents,'directory_roots',directory_roots,'info',info) FROM runners ORDER BY name").fetch_all(&s.pool).await?)))
}
async fn agents(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT a || jsonb_build_object('runner_id',r.id,'runner',r.name) FROM runners r CROSS JOIN LATERAL jsonb_array_elements(r.agents) a WHERE NOT r.revoked").fetch_all(&s.pool).await?)))
}
async fn template(State(s): State<App>, h: HeaderMap, Path(id): Path<String>) -> Result<Response> {
    user(&s, &h, "viewer", false).await?;
    Ok((
        [(header::CONTENT_TYPE, "application/yaml")],
        crate::config::builtin(&id).map_err(|_| Error::missing())?,
    )
        .into_response())
}
async fn enrollment(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    user(&s, &h, "operator", true).await?;
    let t = auth::secret();
    sqlx::query("INSERT INTO enrollments(hash,expires_at) VALUES($1,now()+interval '30 minutes')")
        .bind(auth::hash(&t))
        .execute(&s.pool)
        .await?;
    Ok(Json(json!({"token":t,"gateway":s.config.public_url})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Enroll {
    token: String,
    name: String,
    public_key: String,
}
async fn enroll(State(s): State<App>, ApiJson(b): ApiJson<Enroll>) -> Result<Json<Value>> {
    crate::config::validate_id(&b.name).map_err(|e| Error::bad(e.to_string()))?;
    let id = Uuid::new_v4();
    let jwt = s
        .authority
        .runner_jwt(id, &b.public_key)
        .map_err(|e| Error::bad(e.to_string()))?;
    let mut tx = s.pool.begin().await?;
    let n=sqlx::query("UPDATE enrollments SET used_at=now() WHERE hash=$1 AND used_at IS NULL AND expires_at>now()").bind(auth::hash(&b.token)).execute(&mut *tx).await?.rows_affected();
    if n == 0 {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "enrollment token expired or already used".into(),
        ));
    }
    let token = auth::secret();
    sqlx::query("INSERT INTO runners(id,name,token_hash,public_key) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(b.name)
        .bind(auth::hash(&token))
        .bind(b.public_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| Error::conflict("runner name or identity already registered"))?;
    bus::provision(&s.js, id, 2).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"token":token,"jwt":jwt,"nats_url":s.config.nats_public_url}),
    ))
}
async fn revoke(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    let u = user(&s, &h, "admin", true).await?;
    let mut tx = s.pool.begin().await?;
    // Serialize revocation with scheduling/claiming on the affected tasks.
    sqlx::query("SELECT id FROM tasks WHERE runner_id=$1 ORDER BY id FOR UPDATE")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    sqlx::query("UPDATE runners SET revoked=true WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runs SET state='unknown',reason='runner revoked' WHERE runner_id=$1 AND state IN ('running','starting')").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE runs SET state='cancelled',reason='runner revoked before start',finished_at=now() WHERE runner_id=$1 AND state='queued'").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE tasks SET enabled=false,config=jsonb_set(config,'{enabled}','false'),revision=revision+1 WHERE runner_id=$1 AND enabled").bind(id).execute(&mut *tx).await?;
    store::audit(&mut tx, u.id, "runner.revoke", &id.to_string()).await?;
    tx.commit().await?;
    let pending = s
        .authority
        .sync_revocations(&s.config.nats_url, &s.pool)
        .await
        .is_err();
    Ok(Json(json!({"ok":true,"broker_revocation_pending":pending})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Hello {
    runtime_id: Uuid,
    agents: Vec<protocol::AgentInfo>,
    #[serde(default)]
    directory_roots: Vec<String>,
    info: Value,
    concurrency: usize,
}
async fn hello(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Hello>,
) -> Result<Json<Value>> {
    let id = device(&s, &h).await?;
    if b.concurrency == 0 || b.concurrency > 128 || b.agents.len() > 100 {
        return Err(Error::bad("invalid runner capacity"));
    }
    let n=sqlx::query("UPDATE runners SET runtime_id=$2,last_seen=now(),agents=$3,info=$4,directory_roots=$5 WHERE id=$1 AND (runtime_id IS NULL OR runtime_id=$2 OR last_seen<now()-interval '60 seconds')").bind(id).bind(b.runtime_id).bind(json!(b.agents)).bind(b.info).bind(json!(b.directory_roots)).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(Error::conflict("another runner process is still active"));
    }
    bus::provision(&s.js, id, b.concurrency).await?;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Heartbeat {
    runtime_id: Uuid,
    #[serde(default)]
    claims: Vec<ClaimPair>,
    info: Value,
}
#[derive(Deserialize, schemars::JsonSchema)]
struct ClaimPair {
    run_id: Uuid,
    claim_id: Uuid,
    #[serde(default = "true_value")]
    monitoring: bool,
}
async fn heartbeat(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Heartbeat>,
) -> Result<Json<Value>> {
    let id = device(&s, &h).await?;
    let n = sqlx::query("UPDATE runners SET last_seen=now(),info=$3 WHERE id=$1 AND runtime_id=$2")
        .bind(id)
        .bind(b.runtime_id)
        .bind(b.info)
        .execute(&s.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(Error::conflict(
            "runner process no longer owns device lease",
        ));
    }
    let mut closed = Vec::new();
    for c in b.claims {
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM runs WHERE id=$1 AND claim_id=$2 AND runner_id=$3",
        )
        .bind(c.run_id)
        .bind(c.claim_id)
        .bind(id)
        .fetch_optional(&s.pool)
        .await?;
        if state.as_deref().is_some_and(protocol::terminal) {
            closed.push(c.run_id);
        } else if c.monitoring {
            sqlx::query("UPDATE runs SET last_seen=now() WHERE id=$1 AND claim_id=$2 AND runner_id=$3 AND state IN ('running','starting','unknown')").bind(c.run_id).bind(c.claim_id).bind(id).execute(&s.pool).await?;
        }
    }
    Ok(Json(json!({"ok":true,"closed":closed})))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Claim {
    run_id: Uuid,
    claim_id: Uuid,
    runtime_id: Uuid,
    agent_digest: String,
}
async fn claim(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Claim>,
) -> Result<Json<Value>> {
    let runner = device(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let task_row = sqlx::query("SELECT t.id,t.archived_at FROM tasks t JOIN runs r ON r.task_id=t.id WHERE r.id=$1 AND r.runner_id=$2 FOR UPDATE OF t").bind(b.run_id).bind(runner).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    let row = sqlx::query("SELECT * FROM runs WHERE id=$1 FOR UPDATE")
        .bind(b.run_id)
        .fetch_one(&mut *tx)
        .await?;
    let owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1 AND runtime_id=$2 AND NOT revoked)",
    )
    .bind(runner)
    .bind(b.runtime_id)
    .fetch_one(&mut *tx)
    .await?;
    if !owner {
        return Err(Error::conflict("runner lease expired"));
    }
    let state: String = row.get("state");
    if row.get::<Option<Uuid>, _>("claim_id") == Some(b.claim_id)
        && row.get::<Option<Uuid>, _>("runtime_id") == Some(b.runtime_id)
        && matches!(state.as_str(), "starting" | "running")
    {
        return Ok(Json(
            json!({"execute":true,"cancel_requested":row.get::<bool,_>("cancel_requested"),"session_title":row.get::<Option<Value>,_>("execution_snapshot").and_then(|v|v.get("session_title").cloned())}),
        ));
    }
    if task_row
        .get::<Option<DateTime<Utc>>, _>("archived_at")
        .is_some()
        || state != "queued"
        || row.get::<bool, _>("cancel_requested")
        || row.get::<DateTime<Utc>, _>("deadline")
            < sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
                .fetch_one(&mut *tx)
                .await?
    {
        return Ok(Json(json!({"execute":false,"state":state})));
    }
    let task: TaskConfig = serde_json::from_value(row.get("config"))?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    if task.execution.misfire == crate::config::Misfire::Skip
        && row.get::<String, _>("source") == "schedule"
        && now - row.get::<DateTime<Utc>, _>("scheduled_at") > chrono::Duration::seconds(5)
    {
        sqlx::query(
            "UPDATE runs SET state='skipped',reason='misfire',finished_at=now() WHERE id=$1",
        )
        .bind(b.run_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({"execute":false,"state":"skipped"})));
    }
    if let Some(session) = &task.session {
        let key = format!("session:{}:{}:{}", runner, task.agent, session.id);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE runner_id=$1 AND config->>'agent'=$2 AND COALESCE(session->>'id',config->'session'->>'id')=$3 AND id<>$4 AND state IN ('starting','running','unknown'))")
            .bind(runner).bind(&task.agent).bind(&session.id).bind(b.run_id).fetch_one(&mut *tx).await?;
        if busy {
            sqlx::query("UPDATE runs SET state='skipped',reason='会话正在使用',finished_at=now() WHERE id=$1").bind(b.run_id).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(Json(json!({"execute":false,"state":"skipped"})));
        }
    }
    if task.execution.overlap == crate::config::Overlap::Skip {
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE task_id=$1 AND id<>$2 AND state IN ('starting','running','unknown'))").bind(&task.id).bind(b.run_id).fetch_one(&mut *tx).await?;
        if busy {
            return Err(Error::conflict("task has an active run"));
        }
    }
    let mut snapshot: Value = row
        .get::<Option<Value>, _>("execution_snapshot")
        .unwrap_or(json!({}));
    let session_support:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners n,LATERAL jsonb_array_elements(n.agents) a WHERE n.id=$1 AND a->>'id'=$2 AND a->>'session_supported'='true')").bind(runner).bind(&task.agent).fetch_one(&mut *tx).await?;
    if task.session.is_none() && session_support && snapshot["session_title"].is_null() {
        let timezone: String = sqlx::query_scalar("SELECT data->>'timezone' FROM settings")
            .fetch_one(&mut *tx)
            .await?;
        snapshot["session_title"] = json!(crate::config::generated_session_title(
            &task.name, now, &timezone
        )?);
        sqlx::query("UPDATE runs SET execution_snapshot=$2 WHERE id=$1")
            .bind(b.run_id)
            .bind(&snapshot)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE runs SET state='starting',claim_id=$2,runtime_id=$3,agent_digest=$4,started_at=now(),last_seen=now() WHERE id=$1").bind(b.run_id).bind(b.claim_id).bind(b.runtime_id).bind(b.agent_digest).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"execute":true,"cancel_requested":false,"session_title":snapshot.get("session_title")}),
    ))
}
async fn timezones(State(s): State<App>, h: HeaderMap) -> Result<Json<Vec<String>>> {
    user(&s, &h, "viewer", false).await?;
    let mut names: Vec<String> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(ToString::to_string)
        .collect();
    names.sort();
    names.dedup();
    Ok(Json(names))
}
async fn settings(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT data FROM settings")
            .fetch_one(&s.pool)
            .await?,
    ))
}
async fn update_settings(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Value>,
) -> Result<Json<Value>> {
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
    sqlx::query("UPDATE settings SET data=$1")
        .bind(&b)
        .execute(&mut *tx)
        .await?;
    store::audit(&mut tx, u.id, "settings.update", "workspace").await?;
    tx.commit().await?;
    Ok(Json(b))
}
async fn audits(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    user(&s, &h, "admin", false).await?;
    Ok(Json(json!(
        sqlx::query_scalar::<_, Value>(
            "SELECT to_jsonb(a.*) FROM audit a ORDER BY id DESC LIMIT 100"
        )
        .fetch_all(&s.pool)
        .await?
    )))
}
async fn schema(Path(kind): Path<String>) -> Result<Json<Value>> {
    Ok(Json(match kind.as_str() {
        "agent" => serde_json::to_value(schemars::schema_for!(crate::config::AgentConfig))?,
        "task" => serde_json::to_value(schemars::schema_for!(TaskConfig))?,
        _ => return Err(Error::missing()),
    }))
}
async fn openapi() -> Json<Value> {
    Json(crate::openapi::document())
}
async fn metrics(State(s): State<App>, h: HeaderMap) -> Result<Response> {
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
#[derive(rust_embed::RustEmbed)]
#[folder = "../../web/dist/"]
struct Assets;
async fn static_file(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("health/") {
        return Error::missing().into_response();
    }
    let file = Assets::get(if path.is_empty() { "index.html" } else { path })
        .or_else(|| Assets::get("index.html"));
    match file {
        Some(file) => {
            let mime = if path.contains('.') {
                mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string()
            } else {
                "text/html; charset=utf-8".into()
            };
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
                    (header::CACHE_CONTROL, "no-cache".into()),
                ],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Build web assets with npm run build in web/, then rebuild wakeit.",
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct DirectoryQuery {
    path: Option<String>,
    cursor: Option<String>,
}
async fn directories(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<DirectoryQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "operator", false).await?;
    Ok(Json(
        device_query(
            &s,
            id,
            "list_directories",
            json!({"path":q.path,"cursor":q.cursor}),
            5,
        )
        .await?,
    ))
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelsQuery {
    cwd: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionQuery {
    cwd: String,
    id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionsQuery {
    cwd: String,
    cursor: Option<String>,
    q: Option<String>,
}
async fn query_sessions(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<SessionsQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "operator", true).await?;
    if q.q.as_ref().is_some_and(|v| v.len() > 1024)
        || q.cursor.as_ref().is_some_and(|v| v.len() > 2048)
    {
        return Err(Error::bad("invalid session query"));
    }
    Ok(Json(
        device_query(
            &s,
            id,
            "list_sessions",
            json!({"cwd":q.cwd,"agent":agent,"q":q.q,"cursor":q.cursor}),
            30,
        )
        .await?,
    ))
}
async fn inspect_session(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<SessionQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "operator", true).await?;
    crate::sessions::validate_id(&q.id).map_err(|e| Error::bad(e.to_string()))?;
    Ok(Json(
        device_query(
            &s,
            id,
            "inspect_session",
            json!({"cwd":q.cwd,"id":q.id,"agent":agent}),
            30,
        )
        .await?,
    ))
}
async fn query_models(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, agent)): Path<(Uuid, String)>,
    ApiJson(q): ApiJson<ModelsQuery>,
) -> Result<Json<Value>> {
    user(&s, &h, "operator", true).await?;
    Ok(Json(
        device_query(
            &s,
            id,
            "list_models",
            json!({"cwd":q.cwd,"agent":agent}),
            30,
        )
        .await?,
    ))
}
async fn device_query(
    s: &App,
    id: Uuid,
    action: &str,
    payload: Value,
    timeout: u64,
) -> Result<Value> {
    use futures::StreamExt;
    let row = sqlx::query("SELECT runtime_id,last_seen FROM runners WHERE id=$1 AND NOT revoked")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or_else(Error::missing)?;
    let runtime = row
        .get::<Option<Uuid>, _>("runtime_id")
        .ok_or_else(|| Error(StatusCode::SERVICE_UNAVAILABLE, "Runner 离线".into()))?;
    if row
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_seen")
        .is_none_or(|t| chrono::Utc::now() - t > chrono::Duration::seconds(60))
    {
        return Err(Error(StatusCode::SERVICE_UNAVAILABLE, "Runner 离线".into()));
    }
    let request = protocol::QueryRequest {
        version: 1,
        id: Uuid::new_v4(),
        runtime_id: runtime,
        action: action.into(),
        payload,
    };
    let reply = format!("{}.{}", protocol::reply_prefix(id), request.id.simple());
    let mut sub = s
        .client
        .subscribe(reply.clone())
        .await
        .map_err(anyhow::Error::from)?;
    s.client
        .publish_with_reply(
            protocol::query_subject(id),
            reply,
            serde_json::to_vec(&request)?.into(),
        )
        .await
        .map_err(anyhow::Error::from)?;
    s.client.flush().await.map_err(anyhow::Error::from)?;
    let result=tokio::time::timeout(Duration::from_secs(timeout),async {
        while let Some(msg)=sub.next().await {
            if msg.status==Some(async_nats::StatusCode::NO_RESPONDERS) {return Err(Error(StatusCode::SERVICE_UNAVAILABLE,"Runner 离线".into()));}
            let response:protocol::QueryReply=serde_json::from_slice(&msg.payload)?;
            if response.version!=1 || response.id!=request.id || response.runtime_id!=runtime {continue;}
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1 AND runtime_id=$2 AND NOT revoked)").bind(id).bind(runtime).fetch_one(&s.pool).await?;
            if !valid {return Err(Error::conflict("Runner 身份已改变"));}
            if let Some(error)=response.error {return Err(Error(StatusCode::BAD_REQUEST,error.message));}
            return response.data.ok_or_else(||Error::bad("empty device response"));
        }
        Err(Error(StatusCode::SERVICE_UNAVAILABLE,"Runner 离线".into()))
    }).await;
    let _ = sub.unsubscribe().await;
    result.map_err(|_| Error(StatusCode::GATEWAY_TIMEOUT, "Runner 查询超时".into()))?
}

async fn meta(State(s): State<App>) -> Json<Value> {
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"public_url":s.config.public_url,"install":{"version":"0.1.0","release":"https://github.com/mob999/wakeit/releases/tag/v0.1.0","manifest":"https://github.com/mob999/wakeit/releases/download/v0.1.0/wakeit-release.json","checksums":"https://github.com/mob999/wakeit/releases/download/v0.1.0/SHA256SUMS","base_url":"https://github.com/mob999/wakeit/releases/download/v0.1.0"}}),
    )
}
