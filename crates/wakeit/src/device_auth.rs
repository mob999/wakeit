//! Browser-confirmed device grants. All state and limits are shared across Gateways.
use crate::{
    api::{App, Error, User, reauthorize, user},
    auth, store,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub os: String,
    #[serde(default)]
    pub register_runner: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
}
pub fn validate_url(value: &str) -> anyhow::Result<String> {
    let u = reqwest::Url::parse(value)?;
    anyhow::ensure!(
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path().trim_matches('/').is_empty(),
        "Gateway must be an origin without credentials, path, query or fragment"
    );
    let local = matches!(
        u.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    anyhow::ensure!(
        u.scheme() == "https" || (u.scheme() == "http" && local),
        "Gateway requires HTTPS (HTTP is allowed only on loopback)"
    );
    Ok(u.as_str().trim_end_matches('/').to_string())
}
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/oauth/device/code", post(create))
        .route("/api/v1/oauth/token", post(exchange))
        .route("/api/v1/device-authorizations/{code}", get(details))
        .route(
            "/api/v1/device-authorizations/{code}/approve",
            post(approve),
        )
        .route("/api/v1/device-authorizations/{code}/deny", post(deny))
        .route(
            "/api/v1/workspaces/{workspace}/runners/connect",
            post(connect_scoped),
        )
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        ))
        .layer(tower_http::set_header::SetResponseHeaderLayer::overriding(
            header::PRAGMA,
            header::HeaderValue::from_static("no-cache"),
        ))
}
async fn limit(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, key: &str, max: i64) -> Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,8678))")
        .bind(key)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM device_auth_limits WHERE created_at<now()-interval '10 minutes'")
        .execute(&mut **tx)
        .await?;
    let n:i64=sqlx::query_scalar("SELECT count(*) FROM device_auth_limits WHERE key=$1 AND created_at>now()-interval '1 minute'").bind(key).fetch_one(&mut **tx).await?;
    if n >= max {
        return Err(Error(StatusCode::TOO_MANY_REQUESTS, "slow_down".into()));
    }
    sqlx::query("INSERT INTO device_auth_limits(key) VALUES($1)")
        .bind(key)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn browser(s: &App, h: &HeaderMap, write: bool) -> Result<User> {
    if h.contains_key(header::AUTHORIZATION) || super::api::cookie(&s.config, h).is_none() {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "Web session required".into(),
        ));
    }
    let u = user(s, h, "viewer", write).await?;
    let session: bool = sqlx::query_scalar("SELECT kind='session' FROM credentials WHERE hash=$1")
        .bind(&u.credential)
        .fetch_one(&s.pool)
        .await?;
    if !session {
        return Err(Error(StatusCode::FORBIDDEN, "Web session required".into()));
    }
    Ok(u)
}
fn scoped(mut h: HeaderMap, workspace: Uuid) -> HeaderMap {
    h.insert("x-wakeit-workspace", workspace.to_string().parse().unwrap());
    h
}
async fn connect_scoped(
    State(s): State<App>,
    h: HeaderMap,
    Path(workspace): Path<Uuid>,
    body: Json<Connect>,
) -> Result<Json<Value>> {
    connect(State(s), scoped(h, workspace), body).await
}
async fn create(State(s): State<App>, Json(intent): Json<Intent>) -> Result<Json<Value>> {
    let workspace = intent
        .workspace_id
        .ok_or_else(|| Error::bad("workspace is required"))?;
    let valid: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspaces WHERE id=$1 AND NOT is_demo)")
            .bind(workspace)
            .fetch_one(&s.pool)
            .await?;
    if !valid {
        return Err(Error::bad("workspace is unavailable"));
    }
    if intent.name.is_empty()
        || intent.name.len() > 100
        || intent.os.len() > 100
        || intent.allowed_roots.len() > 32
        || intent
            .allowed_roots
            .iter()
            .any(|p| p.is_empty() || p.len() > 4096)
        || (intent.register_runner && intent.allowed_roots.is_empty())
    {
        return Err(Error::bad("invalid device details"));
    }
    if intent.register_runner {
        crate::config::validate_id(&intent.name).map_err(|e| Error::bad(e.to_string()))?;
    }
    let base = validate_url(&s.config.public_url).map_err(|e| Error::bad(e.to_string()))?;
    let mut tx = s.pool.begin().await?;
    limit(&mut tx, "device-create-global", 60).await?;
    // 40 bits of entropy; no ambiguous I/O/0/1. Only hashes are stored.
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let code: String = (0..8)
        .map(|_| alphabet[rand::thread_rng().gen_range(0..alphabet.len())] as char)
        .collect();
    let device = auth::secret();
    sqlx::query("DELETE FROM device_grants WHERE expires_at<now()-interval '1 day' AND (api_hash IS NULL OR NOT EXISTS (SELECT 1 FROM credentials c WHERE c.hash=device_grants.api_hash))")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO device_grants(device_hash,user_hash,intent,expires_at,workspace_id) VALUES($1,$2,$3,now()+interval '10 minutes',$4)").bind(auth::hash(&device)).bind(auth::hash(&code)).bind(json!(intent)).bind(workspace).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"device_code":device,"user_code":code,"verification_uri":format!("{base}/authorize/device"),"verification_uri_complete":format!("{base}/authorize/device?user_code={code}"),"expires_in":600,"interval":5}),
    ))
}
fn code_hash(code: &str) -> Result<String> {
    let c = code.replace(['-', ' '], "").to_ascii_uppercase();
    if c.len() != 8 || !c.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Error::bad("invalid verification code"));
    }
    Ok(auth::hash(&c))
}
async fn details(
    State(s): State<App>,
    h: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<Value>> {
    browser(&s, &h, false).await?;
    let mut tx = s.pool.begin().await?;
    limit(&mut tx, "device-lookup-global", 120).await?;
    let v:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('intent',intent,'state',CASE WHEN expires_at<=now() THEN 'expired' ELSE state END,'expires_at',expires_at,'gateway',$2::text) FROM device_grants WHERE user_hash=$1").bind(code_hash(&code)?).bind(&s.config.public_url).fetch_optional(&mut *tx).await?;
    let ws: Uuid = sqlx::query_scalar("SELECT workspace_id FROM device_grants WHERE user_hash=$1")
        .bind(code_hash(&code)?)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| Error(StatusCode::NOT_FOUND, "invalid verification code".into()))?;
    tx.commit().await?;
    let member = browser(&s, &scoped(h, ws), false).await?;
    let mut v = v;
    if let Some(value) = &mut v {
        value["workspace_id"] = json!(ws);
        value["role"] = json!(member.role);
        value["workspace"] =
            sqlx::query_scalar::<_, String>("SELECT name FROM workspaces WHERE id=$1")
                .bind(ws)
                .fetch_one(&s.pool)
                .await?
                .into();
    }
    Ok(Json(v.ok_or_else(|| {
        Error(StatusCode::NOT_FOUND, "invalid verification code".into())
    })?))
}
async fn decide(s: App, h: HeaderMap, code: String, allow: bool) -> Result<Json<Value>> {
    browser(&s, &h, true).await?;
    let workspace: Uuid =
        sqlx::query_scalar("SELECT workspace_id FROM device_grants WHERE user_hash=$1")
            .bind(code_hash(&code)?)
            .fetch_optional(&s.pool)
            .await?
            .ok_or_else(|| Error::bad("invalid verification code"))?;
    let initial = browser(&s, &scoped(h, workspace), true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    let u = reauthorize(&mut tx, &initial, "viewer").await?;
    let row=sqlx::query("SELECT intent,state,expires_at>now() AS valid,user_id FROM device_grants WHERE user_hash=$1 FOR UPDATE").bind(code_hash(&code)?).fetch_optional(&mut *tx).await?.ok_or_else(||Error::bad("invalid verification code"))?;
    let intent: Intent = serde_json::from_value(row.get("intent"))?;
    if allow && intent.register_runner && auth::role(&u.role) < 2 {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Operator permission is required to register a Runner".into(),
        ));
    }
    if !row.get::<bool, _>("valid") {
        return Err(Error::bad("expired_token"));
    }
    let state: String = row.get("state");
    let target = if allow { "approved" } else { "denied" };
    if state == target && row.get::<Option<Uuid>, _>("user_id") == Some(u.id) {
        return Ok(Json(json!({"state":state})));
    }
    if state != "pending" {
        return Err(Error::conflict("authorization already decided"));
    }
    sqlx::query(
        "UPDATE device_grants SET state=$2,user_id=$3,approval_credential=$4 WHERE user_hash=$1",
    )
    .bind(code_hash(&code)?)
    .bind(target)
    .bind(u.id)
    .bind(&u.credential)
    .execute(&mut *tx)
    .await?;
    store::audit(
        &mut tx,
        u.id,
        Some(workspace),
        if allow {
            "device.approve"
        } else {
            "device.deny"
        },
        &intent.name,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"state":target})))
}
async fn approve(
    State(s): State<App>,
    h: HeaderMap,
    Path(code): Path<String>,
) -> Result<Json<Value>> {
    decide(s, h, code, true).await
}
async fn deny(State(s): State<App>, h: HeaderMap, Path(code): Path<String>) -> Result<Json<Value>> {
    decide(s, h, code, false).await
}
#[derive(Deserialize, schemars::JsonSchema)]
pub struct TokenRequest {
    pub device_code: String,
    pub grant_type: String,
}
fn oauth_error(code: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error":code}))).into_response()
}
async fn exchange(State(s): State<App>, Json(b): Json<TokenRequest>) -> Result<Response> {
    if b.grant_type != "urn:ietf:params:oauth:grant-type:device_code" || b.device_code.len() != 64 {
        return Ok(oauth_error("invalid_grant"));
    }
    let workspace: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM device_grants WHERE device_hash=$1")
            .bind(auth::hash(&b.device_code))
            .fetch_optional(&s.pool)
            .await?;
    let Some(workspace) = workspace else {
        return Ok(oauth_error("invalid_grant"));
    };
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    let row=sqlx::query("SELECT *,expires_at>now() AS valid, last_poll IS NOT NULL AND last_poll>now()-make_interval(secs=>poll_interval) AS too_fast FROM device_grants WHERE device_hash=$1 FOR UPDATE").bind(auth::hash(&b.device_code)).fetch_optional(&mut *tx).await?;
    let Some(row) = row else {
        return Ok(oauth_error("invalid_grant"));
    };
    if !row.get::<bool, _>("valid") {
        return Ok(oauth_error("expired_token"));
    }
    let state: String = row.get("state");
    if state == "consumed" {
        return Ok(oauth_error("invalid_grant"));
    }
    if state == "denied" {
        return Ok(oauth_error("access_denied"));
    }
    if row.get::<bool, _>("too_fast") {
        sqlx::query("UPDATE device_grants SET last_poll=now(),poll_interval=LEAST(60,poll_interval+5) WHERE device_hash=$1").bind(auth::hash(&b.device_code)).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(oauth_error("slow_down"));
    }
    sqlx::query("UPDATE device_grants SET last_poll=now() WHERE device_hash=$1")
        .bind(auth::hash(&b.device_code))
        .execute(&mut *tx)
        .await?;
    if state == "pending" {
        tx.commit().await?;
        return Ok(oauth_error("authorization_pending"));
    }
    let intent: Intent = serde_json::from_value(row.get("intent"))?;
    let credential: String = row.get("approval_credential");
    let needed = if intent.register_runner {
        "operator"
    } else {
        "viewer"
    };
    let actor_id =
        match crate::workspaces::check_member(&mut tx, &credential, workspace, needed).await {
            Ok(id) => id,
            Err(_) => return Ok(oauth_error("access_denied")),
        };
    let current=sqlx::query("SELECT u.username,m.role FROM users u JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=$2 WHERE u.id=$1").bind(actor_id).bind(workspace).fetch_one(&mut *tx).await?;
    let actor = User {
        id: actor_id,
        username: current.get("username"),
        role: current.get("role"),
        credential,
        workspace_id: Some(workspace),
        instance_admin: false,
        kind: "session".into(),
        demo: false,
    };
    let token = auth::secret();
    let hash = auth::hash(&token);
    sqlx::query(
        "INSERT INTO credentials(hash,user_id,kind,name,workspace_id) VALUES($1,$2,'api',$3,$4)",
    )
    .bind(&hash)
    .bind(actor.id)
    .bind(format!("CLI · {}", intent.name))
    .bind(workspace)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE device_grants SET state='consumed',api_hash=$2 WHERE device_hash=$1")
        .bind(auth::hash(&b.device_code))
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    store::audit(
        &mut tx,
        actor.id,
        Some(workspace),
        "device.exchange",
        &intent.name,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"access_token":token,"token_type":"Bearer","workspace_id":workspace,"user":{"id":actor.id,"username":actor.username,"role":actor.role}})).into_response())
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct Connect {
    pub request_id: Uuid,
    pub public_key: String,
    pub signature: String,
    pub timestamp: i64,
    pub name: String,
    pub allowed_roots: Vec<String>,
}
pub fn proof(id: Uuid, name: &str, roots: &[String], timestamp: i64) -> String {
    serde_json::to_string(&json!(["wakeit-connect-v1", id, name, roots, timestamp])).unwrap()
}
async fn connect(
    State(s): State<App>,
    h: HeaderMap,
    Json(b): Json<Connect>,
) -> Result<Json<Value>> {
    let initial = user(&s, &h, "operator", true).await?;
    let workspace = initial.workspace()?;
    crate::config::validate_id(&b.name).map_err(|e| Error::bad(e.to_string()))?;
    if (chrono::Utc::now().timestamp() - b.timestamp).abs() > 60
        || b.allowed_roots.is_empty()
        || b.allowed_roots.len() > 32
    {
        return Err(Error::bad("invalid registration proof"));
    }
    let key = nkeys::KeyPair::from_public_key(&b.public_key)
        .map_err(|_| Error::bad("invalid public key"))?;
    key.verify(
        proof(b.request_id, &b.name, &b.allowed_roots, b.timestamp).as_bytes(),
        &hex::decode(&b.signature).map_err(|_| Error::bad("invalid signature"))?,
    )
    .map_err(|_| Error::bad("invalid signature"))?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    let actor = reauthorize(&mut tx, &initial, "operator").await?;
    let approved: Option<Value> =
        sqlx::query_scalar("SELECT intent FROM device_grants WHERE api_hash=$1")
            .bind(&actor.credential)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(intent) = approved {
        let i: Intent = serde_json::from_value(intent)?;
        if i.register_runner && (i.name != b.name || i.allowed_roots != b.allowed_roots) {
            return Err(Error::bad("registration differs from browser consent"));
        }
    }
    let old=sqlx::query("SELECT d.runner_id,d.public_key,d.user_id,d.workspace_id,r.revoked FROM device_registrations d JOIN runners r ON r.id=d.runner_id WHERE d.request_id=$1 FOR UPDATE OF r").bind(b.request_id).fetch_optional(&mut *tx).await?;
    let id = if let Some(row) = old {
        if row.get::<Uuid, _>("workspace_id") != workspace
            || row.get::<String, _>("public_key") != b.public_key
            || row.get::<Uuid, _>("user_id") != actor.id
            || row.get::<bool, _>("revoked")
        {
            return Err(Error(
                StatusCode::FORBIDDEN,
                "registration identity rejected".into(),
            ));
        }
        row.get::<Uuid, _>("runner_id")
    } else {
        Uuid::new_v4()
    };
    let token = auth::secret();
    if sqlx::query("SELECT 1 FROM device_registrations WHERE request_id=$1")
        .bind(b.request_id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
    {
        sqlx::query("UPDATE runners SET token_hash=$2 WHERE id=$1")
            .bind(id)
            .bind(auth::hash(&token))
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("INSERT INTO runners(id,name,public_key,token_hash,workspace_id) VALUES($1,$2,$3,$4,$5)")
            .bind(id)
            .bind(&b.name)
            .bind(&b.public_key)
            .bind(auth::hash(&token))
            .bind(workspace).execute(&mut *tx)
            .await
            .map_err(|_| {
                Error::conflict("Runner name or identity already registered; choose another name")
            })?;
        sqlx::query("INSERT INTO device_registrations(request_id,runner_id,public_key,user_id,workspace_id) VALUES($1,$2,$3,$4,$5)").bind(b.request_id).bind(id).bind(&b.public_key).bind(actor.id).bind(workspace).execute(&mut *tx).await?;
    }
    let jwt = s.authority.runner_jwt(id, &b.public_key)?;
    crate::bus::provision(&s.js, id, 2).await?;
    store::audit(
        &mut tx,
        actor.id,
        Some(workspace),
        "runner.connect",
        &id.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"token":token,"jwt":jwt,"nats_url":s.config.nats_public_url}),
    ))
}
