//! Self-hosted GitLab is an identity provider only. GitLab tokens never become Wakeit credentials.
use crate::auth::credentials::{gitlab_oauth_credentials, gitlab_origin};
use crate::{
    api::http::{ApiJson, Error, Result},
    auth,
    config::GatewayConfig,
    state::App,
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const COOKIE: &str = "wakeit_gitlab";

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/auth/gitlab/start", get(start))
        .route("/api/v1/auth/gitlab/callback", get(callback))
        .route("/api/v1/auth/gitlab/token", post(token_login))
        .route_layer(axum::middleware::from_fn(
            |request: axum::extract::Request, next: axum::middleware::Next| async move {
                let mut response = next.run(request).await;
                no_cache(&mut response);
                response
            },
        ))
}

fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|v| v.trim().strip_prefix("wakeit_gitlab="))
}

fn cookie_header(config: &GatewayConfig, value: &str, max_age: u32) -> String {
    format!(
        "{COOKIE}={value}; HttpOnly; SameSite=Lax; Path=/api/v1/auth/gitlab; Max-Age={max_age}{}",
        if config.public_url.starts_with("https:") {
            "; Secure"
        } else {
            ""
        }
    )
}

fn session_cookie_header(config: &GatewayConfig, token: &str) -> String {
    format!(
        "{}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{}",
        config.auth.session_cookie,
        if config.public_url.starts_with("https:") {
            "; Secure"
        } else {
            ""
        }
    )
}

fn no_cache(response: &mut Response) {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
}

fn safe_return(value: &str) -> bool {
    value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value.len() <= 2048
        && !value.starts_with("/api/")
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn check_same_origin(config: &GatewayConfig, h: &HeaderMap) -> Result<()> {
    if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let allowed = reqwest::Url::parse(&config.public_url)?
            .origin()
            .ascii_serialization();
        if origin != allowed {
            return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
        }
    }
    if h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "start login from Wakeit".into(),
        ));
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Wakeit GitLab Login")
        .build()?)
}

#[derive(Default, Deserialize)]
struct Start {
    return_to: Option<String>,
}

async fn start(State(s): State<App>, h: HeaderMap, Query(q): Query<Start>) -> Result<Response> {
    begin(&s.pool, &s.config, h, q).await
}

async fn begin(pool: &PgPool, config: &GatewayConfig, h: HeaderMap, q: Start) -> Result<Response> {
    if config.auth.mode != "gitlab" {
        return Err(Error::missing());
    }
    let origin = gitlab_origin(config)?;
    if config
        .auth
        .gitlab
        .as_ref()
        .and_then(|g| g.client_id_env.as_ref())
        .is_none()
    {
        return Err(Error(
            StatusCode::CONFLICT,
            "GitLab OAuth is not configured".into(),
        ));
    }
    let (client_id, _) = gitlab_oauth_credentials(config)?;
    let return_to = q.return_to.as_deref().unwrap_or("/");
    if !safe_return(return_to) {
        return Err(Error::bad("invalid login return path"));
    }
    check_same_origin(config, &h)?;
    let browser = cookie(&h)
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .unwrap_or_else(auth::credentials::secret);
    let state = auth::credentials::secret();
    let verifier = auth::credentials::secret();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743281)")
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_auth_limits WHERE key='gitlab:global' AND created_at>now()-interval '1 minute'",
    )
    .fetch_one(&mut *tx)
    .await?;
    let browser_key = format!("gitlab:browser:{}", auth::credentials::hash(&browser));
    let own: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_auth_limits WHERE key=$1 AND created_at>now()-interval '1 minute'",
    )
    .bind(&browser_key)
    .fetch_one(&mut *tx)
    .await?;
    if count >= 600 || own >= 10 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts; try again later".into(),
        ));
    }
    sqlx::query("INSERT INTO device_auth_limits(key) VALUES('gitlab:global'),($1)")
        .bind(&browser_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM gitlab_login_transactions WHERE expires_at<=now()")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO gitlab_login_transactions(state_hash,origin,browser_hash,verifier,return_to,expires_at) VALUES($1,$2,$3,$4,$5,now()+interval '10 minutes')",
    )
    .bind(auth::credentials::hash(&state))
    .bind(&origin)
    .bind(auth::credentials::hash(&browser))
    .bind(&verifier)
    .bind(return_to)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut url = reqwest::Url::parse(&format!("{origin}/oauth/authorize"))?;
    let public = config.public_url.trim_end_matches('/');
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id.as_str()),
        (
            "redirect_uri",
            &format!("{public}/api/v1/auth/gitlab/callback"),
        ),
        ("response_type", "code"),
        ("state", &state),
        ("code_challenge", &challenge(&verifier)),
        ("code_challenge_method", "S256"),
        ("scope", "read_user"),
    ]);
    let mut response = Redirect::to(url.as_str()).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie_header(config, &browser, 600).parse().unwrap(),
    );
    no_cache(&mut response);
    Ok(response)
}

#[derive(Deserialize)]
struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Token {
    access_token: Option<String>,
}

#[derive(Deserialize)]
struct GitLabUser {
    id: u64,
    username: String,
    #[serde(default)]
    state: Option<String>,
}

fn validate_user(user: GitLabUser) -> Result<GitLabUser> {
    let username = user.username.trim();
    if user.id == 0 || username.is_empty() || username.len() > 100 {
        return Err(Error::bad("invalid GitLab identity"));
    }
    if user.state.as_deref().is_some_and(|s| s != "active") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "GitLab account is not active".into(),
        ));
    }
    Ok(GitLabUser {
        id: user.id,
        username: username.to_owned(),
        state: user.state,
    })
}

async fn oauth_identity(
    client: &reqwest::Client,
    config: &GatewayConfig,
    origin: &str,
    code: &str,
    verifier: &str,
) -> Result<GitLabUser> {
    let (id, secret) = gitlab_oauth_credentials(config)?;
    let redirect = format!(
        "{}/api/v1/auth/gitlab/callback",
        config.public_url.trim_end_matches('/')
    );
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", id.as_str()),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect.as_str()),
    ];
    if let Some(secret) = secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let response = client
        .post(format!("{origin}/oauth/token"))
        .header(header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_GATEWAY,
                "GitLab login is unavailable; try again".into(),
            )
        })?;
    if !response.status().is_success() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "GitLab login is unavailable; try again".into(),
        ));
    }
    let token: Token = response
        .json()
        .await
        .map_err(|_| Error::bad("invalid GitLab login response"))?;
    let token = token
        .access_token
        .filter(|v| !v.is_empty() && v.len() <= 4096)
        .ok_or_else(|| Error::bad("GitLab authorization could not be exchanged; try again"))?;
    let response = client
        .get(format!("{origin}/api/v4/user"))
        .bearer_auth(&token)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_GATEWAY,
                "GitLab identity is unavailable; try again".into(),
            )
        })?;
    if !response.status().is_success() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "GitLab identity is unavailable; try again".into(),
        ));
    }
    let user: GitLabUser = response
        .json()
        .await
        .map_err(|_| Error::bad("invalid GitLab identity"))?;
    validate_user(user)
}

async fn pat_identity(client: &reqwest::Client, origin: &str, token: &str) -> Result<GitLabUser> {
    let response = client
        .get(format!("{origin}/api/v4/user"))
        .header("PRIVATE-TOKEN", token)
        .header(header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_GATEWAY,
                "GitLab identity is unavailable; try again".into(),
            )
        })?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "invalid GitLab token".into(),
        ));
    }
    if !response.status().is_success() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "GitLab identity is unavailable; try again".into(),
        ));
    }
    let user: GitLabUser = response
        .json()
        .await
        .map_err(|_| Error::bad("invalid GitLab identity"))?;
    validate_user(user)
}

async fn sign_in(pool: &PgPool, origin: &str, subject: &GitLabUser) -> Result<String> {
    let mut tx = pool.begin().await?;
    let subject_id = format!("{origin}:{}", subject.id);
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,3))")
        .bind(&subject_id)
        .execute(&mut *tx)
        .await?;
    let existing = sqlx::query(
        "SELECT u.id,u.active FROM user_identities i JOIN users u ON u.id=i.user_id WHERE i.provider='gitlab' AND i.subject=$1 FOR UPDATE OF u",
    )
    .bind(&subject_id)
    .fetch_optional(&mut *tx)
    .await?;
    let user = if let Some(row) = existing {
        if !row.get::<bool, _>("active") {
            return Err(Error(StatusCode::FORBIDDEN, "account is disabled".into()));
        }
        row.get::<Uuid, _>("id")
    } else {
        let user = Uuid::new_v4();
        // A username collision never grants access to an existing account.
        let name = format!("@{}", subject.username);
        let inserted = sqlx::query(
            "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,NULL,'viewer') ON CONFLICT(username) DO NOTHING",
        )
        .bind(user)
        .bind(&name)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            sqlx::query(
                "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,NULL,'viewer')",
            )
            .bind(user)
            .bind(format!("gitlab-{}-{}", subject.id, user.simple()))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO user_identities(provider,subject,user_id) VALUES('gitlab',$1,$2)")
            .bind(&subject_id)
            .bind(user)
            .execute(&mut *tx)
            .await?;
        user
    };
    let token = auth::credentials::secret();
    sqlx::query(
        "INSERT INTO credentials(hash,user_id,kind,expires_at) VALUES($1,$2,'session',now()+interval '7 days')",
    )
    .bind(auth::credentials::hash(&token))
    .bind(user)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO audit(user_id,action,target) VALUES($1,'identity.gitlab.login',$1::text)",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(token)
}

async fn callback(
    State(s): State<App>,
    h: HeaderMap,
    Query(q): Query<Callback>,
) -> Result<Response> {
    complete(&s.pool, &s.config, h, q).await
}

async fn complete(
    pool: &PgPool,
    config: &GatewayConfig,
    h: HeaderMap,
    q: Callback,
) -> Result<Response> {
    if config.auth.mode != "gitlab" {
        return Err(Error::missing());
    }
    let origin = gitlab_origin(config)?;
    if config
        .auth
        .gitlab
        .as_ref()
        .and_then(|g| g.client_id_env.as_ref())
        .is_none()
    {
        return Err(Error(
            StatusCode::CONFLICT,
            "GitLab OAuth is not configured".into(),
        ));
    }
    let state = q
        .state
        .filter(|v| v.len() == 64)
        .ok_or_else(|| Error::bad("invalid or expired GitLab login"))?;
    let browser = cookie(&h)
        .filter(|v| v.len() == 64)
        .ok_or_else(|| Error::bad("GitLab login browser does not match; try again"))?;
    // Consume before network I/O, including denial. A callback is never replayable.
    let row = sqlx::query(
        "DELETE FROM gitlab_login_transactions WHERE state_hash=$1 AND browser_hash=$2 AND origin=$3 AND expires_at>now() RETURNING verifier,return_to",
    )
    .bind(auth::credentials::hash(&state))
    .bind(auth::credentials::hash(browser))
    .bind(&origin)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| Error::bad("invalid or expired GitLab login"))?;
    let mut response = if q.error.is_some() {
        Redirect::to("/?auth_error=gitlab_denied").into_response()
    } else {
        let code = q
            .code
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .ok_or_else(|| Error::bad("GitLab authorization code is missing"))?;
        let client = http_client()?;
        let identity = oauth_identity(
            &client,
            config,
            &origin,
            &code,
            &row.get::<String, _>("verifier"),
        )
        .await?;
        let token = sign_in(pool, &origin, &identity).await?;
        let target: String = row.get("return_to");
        // Finish in a same-origin document before navigating so Strict session cookies attach.
        let nonce = auth::credentials::secret();
        let target = serde_json::to_string(&target)?.replace('<', "\\u003c");
        let mut response = Html(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Wakeit</title></head><body><script nonce=\"{nonce}\">location.replace({target})</script><noscript><a href=\"/\">Continue to Wakeit</a></noscript></body></html>"
        ))
        .into_response();
        response.headers_mut().insert(
            header::CONTENT_SECURITY_POLICY,
            format!(
                "default-src 'none'; script-src 'nonce-{nonce}'; frame-ancestors 'none'; base-uri 'none'"
            )
            .parse()
            .unwrap(),
        );
        response.headers_mut().append(
            header::SET_COOKIE,
            session_cookie_header(config, &token).parse().unwrap(),
        );
        response
    };
    response.headers_mut().append(
        header::SET_COOKIE,
        cookie_header(config, "", 0).parse().unwrap(),
    );
    no_cache(&mut response);
    Ok(response)
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct GitLabTokenLogin {
    token: String,
}

async fn token_login(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<GitLabTokenLogin>,
) -> Result<Response> {
    sign_in_with_token(&s.pool, &s.config, h, b).await
}

async fn sign_in_with_token(
    pool: &PgPool,
    config: &GatewayConfig,
    h: HeaderMap,
    b: GitLabTokenLogin,
) -> Result<Response> {
    if config.auth.mode != "gitlab" {
        return Err(Error::missing());
    }
    if !config.auth.gitlab.as_ref().is_some_and(|g| g.allow_token) {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "GitLab token login is disabled".into(),
        ));
    }
    if h.get("x-wakeit-request").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "missing request header".into(),
        ));
    }
    check_same_origin(config, &h)?;
    let token = b.token.trim();
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(Error::bad("invalid GitLab token"));
    }
    let origin = gitlab_origin(config)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743281)")
        .execute(&mut *tx)
        .await?;
    let global_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_auth_limits WHERE key='gitlab:token:global' AND created_at>now()-interval '1 minute'",
    )
    .fetch_one(&mut *tx)
    .await?;
    let token_key = format!("gitlab:token:{}", auth::credentials::hash(token));
    let own_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_auth_limits WHERE key=$1 AND created_at>now()-interval '1 minute'",
    )
    .bind(&token_key)
    .fetch_one(&mut *tx)
    .await?;
    if global_count >= 120 || own_count >= 10 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts; try again later".into(),
        ));
    }
    sqlx::query("INSERT INTO device_auth_limits(key) VALUES('gitlab:token:global'),($1)")
        .bind(&token_key)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let client = http_client()?;
    let identity = pat_identity(&client, &origin, token).await?;
    let session = sign_in(pool, &origin, &identity).await?;
    let mut response = (
        [(header::SET_COOKIE, session_cookie_header(config, &session))],
        Json(json!({"ok":true,"token":session})),
    )
        .into_response();
    no_cache(&mut response);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ok<T>(result: Result<T>) -> T {
        result.unwrap_or_else(|e| panic!("{}: {}", e.0, e.1))
    }

    fn fixture_config(gitlab_url: &str, oauth: bool, allow_token: bool) -> GatewayConfig {
        let mut gitlab = json!({"url": gitlab_url, "allow_token": allow_token});
        if oauth {
            gitlab["client_id_env"] = json!("WAKEIT_TEST_GITLAB_CLIENT_ID");
            gitlab["client_secret_env"] = json!("WAKEIT_TEST_GITLAB_CLIENT_SECRET");
        }
        serde_json::from_value(json!({
            "version": 1,
            "public_url": "http://127.0.0.1:58620",
            "database_url_env": "WAKEIT_TEST_DATABASE_URL",
            "nats_url": "nats://127.0.0.1:1",
            "nats_public_url": "ws://127.0.0.1:2",
            "nats_auth_dir": "unused",
            "auth": {
                "mode": "gitlab",
                "gitlab": gitlab,
                "demo_workspace": true
            }
        }))
        .unwrap()
    }

    fn browser_headers(response: &Response) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        h
    }

    fn csrf_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("x-wakeit-request", "1".parse().unwrap());
        h.insert(header::ORIGIN, "http://127.0.0.1:58620".parse().unwrap());
        h
    }

    fn login_state(response: &Response) -> String {
        reqwest::Url::parse(response.headers()[header::LOCATION].to_str().unwrap())
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned()
    }

    #[test]
    fn config_validation_and_origin_normalization() {
        let mut config = fixture_config("https://GITLAB.EXAMPLE.COM:443/", false, true);
        crate::auth::credentials::validate_github(&config).unwrap();
        assert_eq!(
            crate::auth::credentials::gitlab_origin(&config).unwrap(),
            "https://gitlab.example.com"
        );
        config.auth.gitlab.as_mut().unwrap().url = "http://gitlab.example.com".into();
        assert!(crate::auth::credentials::validate_github(&config).is_err());
        config.auth.gitlab.as_mut().unwrap().url = "https://gitlab.example.com/subpath".into();
        assert!(crate::auth::credentials::validate_github(&config).is_err());
        config.auth.gitlab.as_mut().unwrap().url = "https://gitlab.example.com".into();
        config.auth.gitlab.as_mut().unwrap().allow_token = false;
        assert!(crate::auth::credentials::validate_github(&config).is_err());
    }

    #[tokio::test]
    #[ignore = "requires a disposable migrated PostgreSQL database; scripts/github_auth_checks.py"]
    async fn database_gitlab_oauth_token_and_origin_isolation() {
        let subject = rand::random::<u32>() as u64 + 1;
        let login = format!("gl-user-{subject}");
        let provider_login = login.clone();
        let pat_login = login.clone();
        let app = Router::new()
            .route(
                "/oauth/token",
                post(
                    |axum::Form(form): axum::Form<std::collections::HashMap<String, String>>| async move {
                        assert_eq!(form["grant_type"], "authorization_code");
                        assert_eq!(form["client_id"], "fixture-gitlab-client");
                        assert_eq!(form["client_secret"], "fixture-gitlab-secret");
                        assert_eq!(form["code"], "fixture-gitlab-code");
                        assert!(!form["code_verifier"].is_empty());
                        axum::Json(json!({"access_token": "fixture-gitlab-oauth-token"}))
                    },
                ),
            )
            .route(
                "/api/v4/user",
                get(move |h: HeaderMap| {
                    let oauth_user = provider_login.clone();
                    let pat_user = pat_login.clone();
                    async move {
                        if h.get(header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            == Some("Bearer fixture-gitlab-oauth-token")
                        {
                            return (
                                StatusCode::OK,
                                axum::Json(
                                    json!({"id": subject, "username": oauth_user, "state": "active"}),
                                ),
                            );
                        }
                        match h.get("private-token").and_then(|v| v.to_str().ok()) {
                            Some("glpat-valid-fixture") => (
                                StatusCode::OK,
                                axum::Json(
                                    json!({"id": subject, "username": pat_user, "state": "active"}),
                                ),
                            ),
                            Some("glpat-blocked-fixture") => (
                                StatusCode::OK,
                                axum::Json(
                                    json!({"id": subject + 1, "username": "blocked-user", "state": "blocked"}),
                                ),
                            ),
                            _ => (
                                StatusCode::UNAUTHORIZED,
                                axum::Json(json!({"message": "401 Unauthorized"})),
                            ),
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_origin = format!("http://{}", listener.local_addr().unwrap());
        let mock = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let config = fixture_config(&mock_origin, true, true);
        crate::auth::credentials::validate_github(&config).unwrap();
        let pool = crate::store::pool(&config.database_url_env).await.unwrap();

        // Pre-create a local account with the same @username to verify non-merging.
        let local = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,username,password_hash,role,instance_admin) VALUES($1,$2,NULL,'viewer',true)")
            .bind(local)
            .bind(format!("@{login}"))
            .execute(&pool)
            .await
            .unwrap();

        let initial = ok(begin(
            &pool,
            &config,
            HeaderMap::new(),
            Start {
                return_to: Some("/invite#fixture-gitlab".into()),
            },
        )
        .await);
        let auth_url =
            reqwest::Url::parse(initial.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let params: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
        assert_eq!(params["response_type"], "code");
        assert_eq!(params["scope"], "read_user");
        assert_eq!(params["code_challenge_method"], "S256");

        let state = login_state(&initial);
        let headers = browser_headers(&initial);
        let response = ok(complete(
            &pool,
            &config,
            headers.clone(),
            Callback {
                state: Some(state.clone()),
                code: Some("fixture-gitlab-code".into()),
                error: None,
            },
        )
        .await);
        let cookies = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            cookies
                .iter()
                .any(|v| v.starts_with("wakeit_session=") && v.contains("SameSite=Strict"))
        );
        assert!(
            cookies
                .iter()
                .any(|v| v.starts_with("wakeit_gitlab=;") && v.contains("Max-Age=0"))
        );

        // Callback cannot be replayed.
        assert!(
            complete(
                &pool,
                &config,
                headers,
                Callback {
                    state: Some(state),
                    code: Some("fixture-gitlab-code".into()),
                    error: None,
                },
            )
            .await
            .is_err()
        );

        let external: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM user_identities WHERE provider='gitlab' AND subject=$1",
        )
        .bind(format!("{mock_origin}:{subject}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_ne!(external, local);

        // Personal Access Token login maps to the exact same GitLab identity without storing the PAT.
        assert!(
            sign_in_with_token(
                &pool,
                &config,
                HeaderMap::new(),
                GitLabTokenLogin {
                    token: "glpat-valid-fixture".into(),
                },
            )
            .await
            .is_err()
        );
        let pat_resp = ok(sign_in_with_token(
            &pool,
            &config,
            csrf_headers(),
            GitLabTokenLogin {
                token: "glpat-valid-fixture".into(),
            },
        )
        .await);
        let pat_body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(pat_resp.into_body(), 4096)
                .await
                .unwrap(),
        )
        .unwrap();
        let pat_session = pat_body["token"].as_str().unwrap();
        assert_ne!(pat_session, "glpat-valid-fixture");
        let resolved =
            crate::auth::workspace::identity(&pool, &auth::credentials::hash(pat_session))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(resolved.id, external);

        // Invalid and blocked GitLab tokens are rejected.
        assert_eq!(
            sign_in_with_token(
                &pool,
                &config,
                csrf_headers(),
                GitLabTokenLogin {
                    token: "glpat-wrong".into(),
                },
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            sign_in_with_token(
                &pool,
                &config,
                csrf_headers(),
                GitLabTokenLogin {
                    token: "glpat-blocked-fixture".into(),
                },
            )
            .await
            .err()
            .unwrap()
            .0,
            StatusCode::FORBIDDEN
        );

        // Same numeric GitLab user ID on a different GitLab origin must NOT share the account.
        let other_session = ok(sign_in(
            &pool,
            "https://other-gitlab.example.com",
            &GitLabUser {
                id: subject,
                username: login.clone(),
                state: Some("active".into()),
            },
        )
        .await);
        let other_identity =
            crate::auth::workspace::identity(&pool, &auth::credentials::hash(&other_session))
                .await
                .unwrap()
                .unwrap();
        assert_ne!(other_identity.id, external);

        // Demo workspace grants read-only viewer access to GitLab-authenticated sessions.
        crate::store::demo::seed(&pool).await.unwrap();
        let demo: Uuid = sqlx::query_scalar("SELECT id FROM workspaces WHERE is_demo")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            crate::auth::workspace::authorize(&pool, &resolved, demo)
                .await
                .unwrap()
                .unwrap()
                .demo
        );

        mock.abort();
    }
}
