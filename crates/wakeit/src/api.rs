use crate::{
    auth, bus,
    config::{GatewayConfig, Schedule, TaskConfig},
    nats_auth, protocol, schedule, store,
};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
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
    pub(crate) fn missing() -> Self {
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
pub(crate) type Result<T> = std::result::Result<T, Error>;
pub(crate) struct ApiJson<T>(pub T);
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
    pub workspace_id: Option<Uuid>,
    pub instance_admin: bool,
    pub kind: String,
    pub demo: bool,
}
impl User {
    pub(crate) fn workspace(&self) -> Result<Uuid> {
        self.workspace_id
            .ok_or_else(|| Error::bad("workspace is required"))
    }
}
// Nested routes contain workspace plus the resource parameters. Keep resource extraction
// independent of the workspace, whose canonical value is set by server middleware.
struct Path<T>(T);
impl<S: Send + Sync, T: serde::de::DeserializeOwned> axum::extract::FromRequestParts<S>
    for Path<T>
{
    type Rejection = Error;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &S) -> Result<Self> {
        let AxumPath(mut params) =
            AxumPath::<std::collections::HashMap<String, String>>::from_request_parts(parts, state)
                .await
                .map_err(|e| Error::bad(e.to_string()))?;
        params.remove("workspace");
        let value = if params.len() == 1 {
            json!(params.into_values().next().unwrap())
        } else {
            json!([
                params.remove("id"),
                params.remove("agent").or_else(|| params.remove("node"))
            ])
        };
        Ok(Self(
            serde_json::from_value(value).map_err(|e| Error::bad(e.to_string()))?,
        ))
    }
}
pub(crate) fn cookie(config: &GatewayConfig, h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|s| {
            s.trim()
                .strip_prefix(&format!("{}=", config.auth.session_cookie))
                .map(str::to_owned)
        })
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
        .or_else(|| cookie(&s.config, h))
        .ok_or_else(|| Error(StatusCode::UNAUTHORIZED, "authentication required".into()))?;
    let hash = auth::hash(&token);
    let identity = crate::workspaces::identity(&s.pool, &hash)
        .await?
        .ok_or_else(|| {
            Error(
                StatusCode::UNAUTHORIZED,
                "session expired or token invalid".into(),
            )
        })?;
    let workspace = h
        .get("x-wakeit-workspace")
        .and_then(|v| v.to_str().ok())
        .map(str::parse::<Uuid>)
        .transpose()
        .map_err(|_| Error::bad("invalid workspace"))?;
    let mut role = "viewer".to_string();
    let mut demo = false;
    if let Some(workspace) = workspace {
        let context = crate::workspaces::authorize(&s.pool, &identity, workspace)
            .await?
            .ok_or_else(Error::missing)?;
        role = context.role;
        demo = context.demo;
        if demo && (write || h.get("x-wakeit-device-query").is_some()) {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "demo workspace is read only".into(),
            ));
        }
        if auth::role(&role) < auth::role(needed) {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "insufficient permissions".into(),
            ));
        }
    } else if needed != "viewer" {
        if !identity.instance_admin || identity.kind != "session" {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "instance administrator required".into(),
            ));
        }
        role = "admin".into();
    }
    Ok(User {
        id: identity.id,
        role,
        username: identity.username,
        credential: hash,
        workspace_id: workspace.or(identity.workspace_id),
        instance_admin: identity.instance_admin,
        kind: identity.kind,
        demo,
    })
}
pub(crate) async fn reauthorize(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    actor: &User,
    needed: &str,
) -> Result<User> {
    let mut current = actor.clone();
    if let Some(workspace) = actor.workspace_id {
        crate::workspaces::check_member(tx, &actor.credential, workspace, needed)
            .await
            .map_err(|_| Error(StatusCode::FORBIDDEN, "workspace permission changed".into()))?;
        current.role = sqlx::query_scalar(
            "SELECT role FROM workspace_members WHERE workspace_id=$1 AND user_id=$2",
        )
        .bind(workspace)
        .bind(actor.id)
        .fetch_one(&mut **tx)
        .await?;
    } else {
        let allowed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM credentials c JOIN users u ON u.id=c.user_id WHERE c.hash=$1 AND c.kind='session' AND u.active AND u.instance_admin AND (c.expires_at IS NULL OR c.expires_at>now()))").bind(&actor.credential).fetch_one(&mut **tx).await?;
        if !allowed {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "instance administrator required".into(),
            ));
        }
    }
    Ok(current)
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
fn workspace_header(h: &HeaderMap) -> Result<Uuid> {
    h.get("x-wakeit-workspace")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| Error::bad("workspace is required"))
}
async fn verify_run(s: &App, workspace: Uuid, id: Uuid) -> Result<()> {
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
async fn workspace_scope(
    AxumPath(params): AxumPath<std::collections::HashMap<String, String>>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(workspace) = params.get("workspace").and_then(|v| v.parse::<Uuid>().ok()) else {
        return Error::bad("invalid workspace").into_response();
    };
    request
        .headers_mut()
        .insert("x-wakeit-workspace", workspace.to_string().parse().unwrap());
    next.run(request).await
}
async fn clear_scope(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    request.headers_mut().remove("x-wakeit-workspace");
    request.headers_mut().remove("x-wakeit-device-query");
    next.run(request).await
}
async fn legacy_business() -> Response {
    (StatusCode::UPGRADE_REQUIRED,Json(json!({"error":"workspace API required; update Wakeit CLI","code":"workspace_required"}))).into_response()
}
pub fn router(app: App) -> Router {
    let business = Router::new()
        .route("/runners/{id}/directories", get(directories))
        .route(
            "/runners/{id}/agents/{agent}/models/query",
            post(query_models),
        )
        .route(
            "/runners/{id}/agents/{agent}/sessions/inspect",
            post(inspect_session),
        )
        .route(
            "/runners/{id}/agents/{agent}/sessions/query",
            post(query_sessions),
        )
        .route("/invites", post(invite))
        .route("/tokens", get(tokens).post(create_token))
        .route("/tokens/{id}", delete(delete_token))
        .route("/tasks", get(tasks))
        .route("/tasks/validate", post(validate_task))
        .route("/tasks/{id}", get(task).put(apply))
        .route("/tasks/{id}/runs", post(trigger))
        .route("/tasks/{id}/archive", post(archive_task))
        .route("/tasks/{id}/restore", post(restore_task))
        .route("/schedule/preview", post(preview))
        .route("/moments/days", get(crate::moments::days))
        .route("/moments", get(crate::moments::entries))
        .route("/runs", get(runs))
        .route("/runs/{id}", get(run))
        .route("/runs/{id}/nodes/{node}", get(run_node))
        .route("/runs/{id}/nodes/{node}/events", get(node_events))
        .route("/runs/{id}/nodes/{node}/resolve", post(resolve_node))
        .route("/runs/{id}/cancel", post(cancel))
        .route("/runs/{id}/retry", post(retry))
        .route("/runs/{id}/resolve", post(resolve))
        .route("/runs/{id}/events", get(events))
        .route("/runs/{id}/stream", get(stream))
        .route("/runners", get(runners))
        .route("/runners/enrollment", post(enrollment))
        .route("/runners/{id}/revoke", post(revoke))
        .route("/agents", get(agents))
        .route("/agents/templates/{id}", get(template))
        .route("/settings", get(settings).put(update_settings))
        .route("/timezones", get(timezones))
        .route("/audit", get(audits))
        .route_layer(axum::middleware::from_fn(workspace_scope));
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/metrics", get(metrics))
        .merge(crate::device_auth::routes())
        .merge(crate::workspace_api::routes())
        .merge(crate::changelog::routes())
        .merge(crate::github_auth::routes())
        .route("/api/v1/meta", get(meta))
        .route("/api/v1/session", post(login).get(me).delete(logout))
        .route("/api/v1/instance/users", get(users).post(create_user))
        .route("/api/v1/instance/users/{id}", put(update_user))
        .route("/api/v1/instance/users/{id}/password", post(reset_password))
        .route("/api/v1/device/enroll", post(enroll))
        .route("/api/v1/device/hello", post(hello))
        .route("/api/v1/device/heartbeat", post(heartbeat))
        .route("/api/v1/device/claim", post(claim))
        .route("/api/v1/schema/{kind}", get(schema))
        .route("/api/v1/openapi.json", get(openapi))
        .nest("/api/v1/workspaces/{workspace}", business)
        .route("/api/v1/{*legacy}", axum::routing::any(legacy_business))
        .fallback(static_file)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| tracing::info_span!("http.request", method=%request.method(), path=request.uri().path())))
        .layer(axum::middleware::from_fn(clear_scope))
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
                "{}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{secure}",
                s.config.auth.session_cookie
            ),
        )],
        Json(json!({"ok":true,"token":token})),
    )
        .into_response())
}
async fn me(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    Ok(Json(
        json!({"id":u.id,"username":u.username,"role":u.role,"instance_admin":u.instance_admin,"kind":u.kind,"workspace_id":u.workspace_id}),
    ))
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
            format!(
                "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
                s.config.auth.session_cookie
            ),
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
async fn reset_password(
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
    let u = reauthorize(&mut tx, &initial, "admin").await?;
    if b.role.is_some() {
        return Err(Error::bad("roles belong to workspace memberships"));
    }
    // Lock every workspace in the same order as member operations before checking
    // whether global deactivation would strand any organization without an Admin.
    sqlx::query("SELECT id FROM workspaces ORDER BY id FOR UPDATE")
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
async fn invite(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<RoleBody>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "admin", true).await?;
    if auth::role(&b.role) == 0 {
        return Err(Error::bad("invalid role"));
    }
    let mut tx = s.pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &actor, "admin").await?;
    let token = auth::secret();
    sqlx::query("INSERT INTO invites(hash,role,expires_at,workspace_id,created_by) VALUES($1,$2,now()+interval '2 days',$3,$4)")
        .bind(auth::hash(&token))
        .bind(b.role)
        .bind(workspace).bind(actor.id).execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"token":token,"url":format!("{}/invite#{}",s.config.public_url.trim_end_matches('/'),token)}),
    ))
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct Accept {
    pub token: String,
    pub username: Option<String>,
    pub password: Option<String>,
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
    crate::workspaces::lock(&mut tx, workspace).await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
        .bind(u.id)
        .fetch_one(&mut *tx)
        .await?;
    reauthorize(&mut tx, &u, "viewer").await?;
    let t = auth::secret();
    sqlx::query(
        "INSERT INTO credentials(hash,user_id,kind,name,expires_at,workspace_id) VALUES($1,$2,'api',$3,$4,$5)",
    )
    .bind(auth::hash(&t))
    .bind(u.id)
    .bind(name)
    .bind(b.expires_at)
    .bind(workspace).execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"token":t})))
}
async fn tokens(
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
async fn delete_token(
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
async fn task(State(s): State<App>, h: HeaderMap, Path(id): Path<String>) -> Result<Json<Value>> {
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
async fn validate_task(
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
        json!({"errors":crate::dag_store::validation(&s.pool,u.workspace()?,&b.config).await?}),
    ))
}
async fn node_execution(s: &App, workspace: Uuid, id: Uuid, node: &str) -> Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT execution_id FROM dag_run_nodes WHERE workspace_id=$1 AND parent_run_id=$2 AND node_id=$3")
        .bind(workspace).bind(id).bind(node).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)
}
async fn run_node(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    let workspace = u.workspace()?;
    let execution = node_execution(&s, workspace, id, &node).await?;
    let summary = crate::dag_store::summaries(&s.pool, workspace, id)
        .await?
        .into_iter()
        .find(|n| n["id"] == node)
        .ok_or_else(Error::missing)?;
    let execution = if let Some(id) = execution {
        run(State(s), h, Path(id)).await?.0
    } else {
        Value::Null
    };
    Ok(Json(json!({"node":summary,"execution":execution})))
}
async fn node_events(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    Query(q): Query<Cursor>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "viewer", false).await?;
    let workspace = u.workspace()?;
    if let Some(execution) = node_execution(&s, workspace, id, &node).await? {
        Ok(Json(json!(
            event_rows(&s, execution, workspace, q.after, q.kind.as_deref()).await?
        )))
    } else {
        Ok(Json(json!([])))
    }
}
async fn resolve_node(
    State(s): State<App>,
    h: HeaderMap,
    Path((id, node)): Path<(Uuid, String)>,
    ApiJson(b): ApiJson<Resolution>,
) -> Result<Json<Value>> {
    let u = user(&s, &h, "operator", true).await?;
    let execution = node_execution(&s, u.workspace()?, id, &node)
        .await?
        .ok_or_else(Error::missing)?;
    resolve(State(s), h, Path(execution), ApiJson(b)).await
}
async fn apply(
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
        store::apply_task(
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
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    Ok(Json(
        store::set_archived(
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
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    let id = store::trigger(
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
async fn retry(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
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
    let id = store::trigger(
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
async fn run(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    let mut value:Value=sqlx::query_scalar("SELECT to_jsonb(r.*)||jsonb_build_object('task_archived',t.archived_at IS NOT NULL) FROM runs r JOIN tasks t ON t.id=r.task_id AND t.workspace_id=r.workspace_id WHERE r.workspace_id=$2 AND r.id=$1").bind(id).bind(workspace).fetch_optional(&s.pool).await?.ok_or_else(Error::missing)?;
    if value["config"].get("dag").is_some() {
        value["nodes"] = json!(crate::dag_store::summaries(&s.pool, workspace, id).await?);
    }
    Ok(Json(value))
}
async fn cancel(State(s): State<App>, h: HeaderMap, Path(id): Path<Uuid>) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "operator", true).await?;
    verify_run(&s, workspace, id).await?;
    let mut tx = s.pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
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
        sqlx::query("UPDATE runs SET cancel_requested=true,state=CASE WHEN state='queued' THEN 'cancelled' ELSE state END,finished_at=CASE WHEN state='queued' THEN now() ELSE finished_at END WHERE id=$1").bind(id).execute(&mut *tx).await?;
        if row.get::<Option<Uuid>, _>("runner_id").is_none() {
            crate::dag_store::cancel_nodes(&mut tx, workspace, id).await?;
        } else {
            store::enqueue(
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
async fn resolve(
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
    crate::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "operator").await?;
    crate::dag_store::lock_parent(&mut tx, id).await?;
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
#[derive(Deserialize, schemars::JsonSchema)]
struct Cursor {
    #[serde(default)]
    after: i64,
    kind: Option<String>,
}
async fn event_rows(
    s: &App,
    id: Uuid,
    workspace: Uuid,
    after: i64,
    kind: Option<&str>,
) -> std::result::Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(e.*) || CASE WHEN r.parent_run_id IS NOT NULL THEN jsonb_build_object('run_id',$1,'node_id',r.node_id,'execution_id',r.id,'original_seq',e.seq,'kind',CASE WHEN e.kind='state' THEN 'node_state' ELSE e.kind END) ELSE CASE WHEN e.kind='node' THEN jsonb_build_object('node_id',e.data->'node_id') ELSE '{}'::jsonb END END FROM events e JOIN runs r ON r.id=e.run_id AND r.workspace_id=e.workspace_id WHERE e.workspace_id=$4 AND (e.run_id=$1 OR r.parent_run_id=$1) AND e.id>$2 AND ($3::text IS NULL OR e.kind=$3 OR ($3='trace' AND e.kind='trace_status')) ORDER BY e.id LIMIT 500",
    )
    .bind(id)
    .bind(after)
    .bind(kind)
    .bind(workspace).fetch_all(&s.pool)
    .await
}
async fn events(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    verify_run(&s, workspace, id).await?;
    Ok(Json(json!(
        event_rows(&s, id, workspace, q.after, q.kind.as_deref()).await?
    )))
}
async fn stream(
    State(s): State<App>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Query(q): Query<Cursor>,
) -> Result<impl IntoResponse> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    verify_run(&s, workspace, id).await?;
    let mut cursor = h
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(q.after);
    let output = async_stream::stream! {let mut tick=tokio::time::interval(Duration::from_secs(1));loop{tick.tick().await;
        if s.shutdown.is_cancelled() || user(&s,&h,"viewer",false).await.is_err(){break}
        match event_rows(&s,id,workspace,cursor,None).await {Ok(rows)=>for row in rows {cursor=row["id"].as_i64().unwrap_or(cursor);yield Ok::<_,Infallible>(Event::default().id(cursor.to_string()).event("run_event").data(row.to_string()));},Err(e)=>{tracing::warn!(error=%e,"SSE read failed");break}}
    }};
    Ok(Sse::new(output).keep_alive(KeepAlive::default()))
}
async fn preview(
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
async fn runners(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT jsonb_build_object('id',id,'name',name,'revoked',revoked,'last_seen',last_seen,'online',NOT revoked AND last_seen>now()-interval '60 seconds','agents',agents,'directory_roots',directory_roots,'info',info) FROM runners WHERE workspace_id=$1 ORDER BY name").bind(workspace).fetch_all(&s.pool).await?)))
}
async fn agents(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(json!(sqlx::query_scalar::<_,Value>("SELECT a || jsonb_build_object('runner_id',r.id,'runner',r.name) FROM runners r CROSS JOIN LATERAL jsonb_array_elements(r.agents) a WHERE r.workspace_id=$1 AND NOT r.revoked").bind(workspace).fetch_all(&s.pool).await?)))
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
    let workspace = workspace_header(&h)?;
    let actor = user(&s, &h, "operator", true).await?;
    let mut tx = s.pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &actor, "operator").await?;
    let t = auth::secret();
    sqlx::query("INSERT INTO enrollments(hash,expires_at,workspace_id,created_by) VALUES($1,now()+interval '30 minutes',$2,$3)")
        .bind(auth::hash(&t))
        .bind(workspace).bind(actor.id).execute(&mut *tx)
        .await?;
    tx.commit().await?;
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
    let enrollment_workspace: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM enrollments WHERE hash=$1")
            .bind(auth::hash(&b.token))
            .fetch_optional(&mut *tx)
            .await?;
    let enrollment_workspace = enrollment_workspace.ok_or_else(|| {
        Error(
            StatusCode::UNAUTHORIZED,
            "enrollment token expired or already used".into(),
        )
    })?;
    crate::workspaces::lock(&mut tx, enrollment_workspace).await?;
    let workspace:Option<Uuid>=sqlx::query_scalar("UPDATE enrollments SET used_at=now() WHERE hash=$1 AND used_at IS NULL AND expires_at>now() AND EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=enrollments.workspace_id AND m.user_id=enrollments.created_by AND m.active AND u.active AND m.role IN ('admin','operator')) RETURNING workspace_id").bind(auth::hash(&b.token)).fetch_optional(&mut *tx).await?;
    if workspace.is_none() {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "enrollment token expired or already used".into(),
        ));
    }
    let workspace = workspace.unwrap();
    let token = auth::secret();
    sqlx::query(
        "INSERT INTO runners(id,name,token_hash,public_key,workspace_id) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(b.name)
    .bind(auth::hash(&token))
    .bind(b.public_key)
    .bind(workspace)
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
    let workspace = workspace_header(&h)?;
    let u = user(&s, &h, "admin", true).await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners WHERE workspace_id=$1 AND id=$2)")
            .bind(workspace)
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
    if !exists {
        return Err(Error::missing());
    }
    let mut tx = s.pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    reauthorize(&mut tx, &u, "admin").await?;
    // Serialize revocation with scheduling/claiming on the affected tasks.
    sqlx::query("SELECT id FROM tasks WHERE runner_id=$1 ORDER BY id FOR UPDATE")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    sqlx::query("UPDATE runners SET revoked=true WHERE workspace_id=$2 AND id=$1")
        .bind(id)
        .bind(workspace)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runs SET state='unknown',reason='runner revoked' WHERE runner_id=$1 AND state IN ('running','starting')").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE runs SET state='cancelled',reason='runner revoked before start',finished_at=now() WHERE runner_id=$1 AND state='queued'").bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE tasks SET enabled=false,config=jsonb_set(config,'{enabled}','false'),revision=revision+1 WHERE runner_id=$1 AND enabled").bind(id).execute(&mut *tx).await?;
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        "runner.revoke",
        &id.to_string(),
    )
    .await?;
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
    let task_row = sqlx::query("SELECT t.id,t.archived_at FROM tasks t JOIN runs r ON r.task_id=t.id AND r.workspace_id=t.workspace_id WHERE r.id=$1 AND r.runner_id=$2 FOR UPDATE OF t").bind(b.run_id).bind(runner).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    let graph_parent = crate::dag_store::lock_parent(&mut tx, b.run_id).await?;
    let parent = if let Some(id) = graph_parent {
        Some(
            sqlx::query(
                "SELECT state,started_at,cancel_requested,finished_at FROM runs WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?,
        )
    } else {
        None
    };
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
    if (task_row
        .get::<Option<DateTime<Utc>>, _>("archived_at")
        .is_some()
        && parent
            .as_ref()
            .is_none_or(|p| p.get::<Option<DateTime<Utc>>, _>("started_at").is_none()))
        || parent.as_ref().is_some_and(|p| {
            p.get::<bool, _>("cancel_requested")
                || p.get::<Option<DateTime<Utc>>, _>("finished_at").is_some()
        })
        || state != "queued"
        || row.get::<bool, _>("cancel_requested")
        || row.get::<DateTime<Utc>, _>("deadline")
            < sqlx::query_scalar::<_, DateTime<Utc>>("SELECT now()")
                .fetch_one(&mut *tx)
                .await?
    {
        return Ok(Json(json!({"execute":false,"state":state})));
    }
    let workspace: Uuid = row.get("workspace_id");
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
    if graph_parent.is_none() && task.execution.overlap == crate::config::Overlap::Skip {
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$3 AND task_id=$1 AND id<>$2 AND state IN ('starting','running','unknown'))").bind(&task.id).bind(b.run_id).bind(workspace).fetch_one(&mut *tx).await?;
        if busy {
            return Err(Error::conflict("task has an active run"));
        }
    }
    let mut snapshot: Value = row
        .get::<Option<Value>, _>("execution_snapshot")
        .unwrap_or(json!({}));
    let session_support:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners n,LATERAL jsonb_array_elements(n.agents) a WHERE n.id=$1 AND a->>'id'=$2 AND a->>'session_supported'='true')").bind(runner).bind(&task.agent).fetch_one(&mut *tx).await?;
    if task.session.is_none() && session_support && snapshot["session_title"].is_null() {
        let timezone: String =
            sqlx::query_scalar("SELECT data->>'timezone' FROM settings WHERE workspace_id=$1")
                .bind(workspace)
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
    if let Some(parent) = graph_parent {
        sqlx::query(
            "UPDATE runs SET state='running',started_at=COALESCE(started_at,now()) WHERE id=$1",
        )
        .bind(parent)
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
    let workspace = workspace_header(&h)?;
    user(&s, &h, "viewer", false).await?;
    Ok(Json(
        sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&s.pool)
            .await?,
    ))
}
async fn update_settings(
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
    crate::workspaces::lock(&mut tx, workspace).await?;
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
async fn audits(State(s): State<App>, h: HeaderMap) -> Result<Json<Value>> {
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
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", false).await?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
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
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    if q.q.as_ref().is_some_and(|v| v.len() > 1024)
        || q.cursor.as_ref().is_some_and(|v| v.len() > 2048)
    {
        return Err(Error::bad("invalid session query"));
    }
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
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
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    crate::sessions::validate_id(&q.id).map_err(|e| Error::bad(e.to_string()))?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
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
    let workspace = workspace_header(&h)?;
    let _actor = user(&s, &h, "operator", true).await?;
    Ok(Json(
        device_query(
            &s,
            &h,
            workspace,
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
    headers: &HeaderMap,
    workspace: Uuid,
    id: Uuid,
    action: &str,
    payload: Value,
    timeout: u64,
) -> Result<Value> {
    use futures::StreamExt;
    let row = sqlx::query(
        "SELECT runtime_id,last_seen FROM runners WHERE workspace_id=$2 AND id=$1 AND NOT revoked",
    )
    .bind(id)
    .bind(workspace)
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
            user(s,headers,"operator",false).await?;
            if let Some(error)=response.error {return Err(Error(StatusCode::BAD_REQUEST,error.message));}
            return response.data.ok_or_else(||Error::bad("empty device response"));
        }
        Err(Error(StatusCode::SERVICE_UNAVAILABLE,"Runner 离线".into()))
    }).await;
    let _ = sub.unsubscribe().await;
    result.map_err(|_| Error(StatusCode::GATEWAY_TIMEOUT, "Runner 查询超时".into()))?
}

async fn meta(State(s): State<App>) -> Json<Value> {
    let version = env!("CARGO_PKG_VERSION");
    let base_url = format!("https://github.com/mob999/wakeit/releases/download/v{version}");
    Json(
        json!({"version":version,"public_url":s.config.public_url,"auth":{"mode":s.config.auth.mode,"allow_registration":s.config.auth.mode=="local" && s.config.auth.allow_registration},"install":{"version":version,"release":format!("https://github.com/mob999/wakeit/releases/tag/v{version}"),"manifest":format!("{base_url}/wakeit-release.json"),"checksums":format!("{base_url}/SHA256SUMS"),"base_url":base_url}}),
    )
}

#[cfg(test)]
mod workspace_cookie_tests {
    use super::*;

    #[test]
    fn configured_session_cookie_isolated_from_other_services() {
        let mut config: GatewayConfig = serde_json::from_value(json!({
            "version": 1,
            "public_url": "http://127.0.0.1:58620",
            "database_url_env": "UNUSED_TEST_DATABASE",
            "nats_url": "nats://127.0.0.1:1",
            "nats_public_url": "ws://127.0.0.1:2",
            "nats_auth_dir": "unused"
        }))
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "wakeit_session=original; wakeit_workspace_preview=preview"
                .parse()
                .unwrap(),
        );
        assert_eq!(cookie(&config, &headers).as_deref(), Some("original"));
        config.auth.session_cookie = "wakeit_workspace_preview".into();
        assert_eq!(cookie(&config, &headers).as_deref(), Some("preview"));
        headers.insert(
            header::COOKIE,
            "wakeit_session=original; another_wakeit_workspace_preview=wrong"
                .parse()
                .unwrap(),
        );
        assert!(cookie(&config, &headers).is_none());
    }
}
