//! GitHub is an identity provider only. Its access token never becomes a Wakeit credential.
use crate::auth::credentials::github_credentials as credentials;
use crate::{
    api::http::{Error, Result},
    auth,
    config::GatewayConfig,
    state::App,
};
use axum::{
    Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const AUTHORIZE: &str = "https://github.com/login/oauth/authorize";
const TOKEN: &str = "https://github.com/login/oauth/access_token";
const USER: &str = "https://api.github.com/user";
const COOKIE: &str = "wakeit_github";
pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/v1/auth/github/start", get(start))
        .route("/api/v1/auth/github/callback", get(callback))
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
        .find_map(|v| v.trim().strip_prefix("wakeit_github="))
}
fn cookie_header(config: &GatewayConfig, value: &str, max_age: u32) -> String {
    format!(
        "{COOKIE}={value}; HttpOnly; SameSite=Lax; Path=/api/v1/auth/github; Max-Age={max_age}{}",
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
#[derive(Default, Deserialize)]
struct Start {
    return_to: Option<String>,
}
async fn start(State(s): State<App>, h: HeaderMap, Query(q): Query<Start>) -> Result<Response> {
    begin(&s.pool, &s.config, h, q).await
}
async fn begin(pool: &PgPool, config: &GatewayConfig, h: HeaderMap, q: Start) -> Result<Response> {
    if config.auth.mode != "github" {
        return Err(Error::missing());
    }
    let (client, _) = credentials(config)?;
    let return_to = q.return_to.as_deref().unwrap_or("/");
    if !safe_return(return_to) {
        return Err(Error::bad("invalid login return path"));
    }
    if let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok())
        && origin != config.public_url
    {
        return Err(Error(StatusCode::FORBIDDEN, "origin rejected".into()));
    }
    if h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "start login from Wakeit".into(),
        ));
    }
    let browser = cookie(&h)
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .unwrap_or_else(auth::credentials::secret);
    let state = auth::credentials::secret();
    let verifier = auth::credentials::secret();
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743279)")
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM device_auth_limits WHERE key='github:global' AND created_at>now()-interval '1 minute'",
    )
    .fetch_one(&mut *tx)
    .await?;
    let browser_key = format!("github:browser:{}", auth::credentials::hash(&browser));
    let own:i64=sqlx::query_scalar("SELECT count(*) FROM device_auth_limits WHERE key=$1 AND created_at>now()-interval '1 minute'").bind(&browser_key).fetch_one(&mut *tx).await?;
    if count >= 600 || own >= 10 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts; try again later".into(),
        ));
    }
    sqlx::query("INSERT INTO device_auth_limits(key) VALUES('github:global'),($1)")
        .bind(&browser_key)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM github_login_transactions WHERE expires_at<=now()")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO github_login_transactions(state_hash,browser_hash,verifier,return_to,expires_at) VALUES($1,$2,$3,$4,now()+interval '10 minutes')").bind(auth::credentials::hash(&state)).bind(auth::credentials::hash(&browser)).bind(&verifier).bind(return_to).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut url = reqwest::Url::parse(AUTHORIZE).unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", client.as_str()),
        (
            "redirect_uri",
            &format!("{}/api/v1/auth/github/callback", config.public_url),
        ),
        ("state", &state),
        ("code_challenge", &challenge(&verifier)),
        ("code_challenge_method", "S256"),
        ("scope", ""),
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
struct GitHubUser {
    id: u64,
    login: String,
}
async fn remote_identity(
    client: &reqwest::Client,
    config: &GatewayConfig,
    code: &str,
    verifier: &str,
    token_url: &str,
    user_url: &str,
) -> Result<GitHubUser> {
    let (id, secret) = credentials(config)?;
    let response = client
        .post(token_url)
        .header(header::ACCEPT, "application/json")
        .form(&[
            ("client_id", id.as_str()),
            ("client_secret", secret.as_str()),
            ("code", code),
            ("code_verifier", verifier),
            (
                "redirect_uri",
                &format!("{}/api/v1/auth/github/callback", config.public_url),
            ),
        ])
        .send()
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_GATEWAY,
                "GitHub login is unavailable; try again".into(),
            )
        })?;
    if !response.status().is_success() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "GitHub login is unavailable; try again".into(),
        ));
    }
    let token: Token = response
        .json()
        .await
        .map_err(|_| Error::bad("invalid GitHub login response"))?;
    let token = token
        .access_token
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::bad("GitHub authorization could not be exchanged; try again"))?;
    let response = client
        .get(user_url)
        .bearer_auth(&token)
        .header(header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|_| {
            Error(
                StatusCode::BAD_GATEWAY,
                "GitHub identity is unavailable; try again".into(),
            )
        })?;
    if !response.status().is_success() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "GitHub identity is unavailable; try again".into(),
        ));
    }
    let user: GitHubUser = response
        .json()
        .await
        .map_err(|_| Error::bad("invalid GitHub identity"))?;
    if user.id == 0 || user.login.is_empty() || user.login.len() > 100 {
        return Err(Error::bad("invalid GitHub identity"));
    }
    Ok(user)
}
async fn sign_in(pool: &PgPool, subject: &GitHubUser) -> Result<String> {
    let mut tx = pool.begin().await?;
    let subject_id = subject.id.to_string();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,2))")
        .bind(&subject_id)
        .execute(&mut *tx)
        .await?;
    let existing=sqlx::query("SELECT u.id,u.active FROM user_identities i JOIN users u ON u.id=i.user_id WHERE i.provider='github' AND i.subject=$1 FOR UPDATE OF u").bind(&subject_id).fetch_optional(&mut *tx).await?;
    let user = if let Some(row) = existing {
        if !row.get::<bool, _>("active") {
            return Err(Error(StatusCode::FORBIDDEN, "account is disabled".into()));
        }
        row.get::<Uuid, _>("id")
    } else {
        let user = Uuid::new_v4();
        // A username collision never grants access to an existing account.
        let name = format!("@{}", subject.login);
        let inserted=sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,NULL,'viewer') ON CONFLICT(username) DO NOTHING").bind(user).bind(&name).execute(&mut *tx).await?.rows_affected();
        if inserted == 0 {
            sqlx::query(
                "INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,NULL,'viewer')",
            )
            .bind(user)
            .bind(format!("github-{}-{}", subject.id, user.simple()))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("INSERT INTO user_identities(provider,subject,user_id) VALUES('github',$1,$2)")
            .bind(&subject_id)
            .bind(user)
            .execute(&mut *tx)
            .await?;
        user
    };
    let token = auth::credentials::secret();
    sqlx::query("INSERT INTO credentials(hash,user_id,kind,expires_at) VALUES($1,$2,'session',now()+interval '7 days')").bind(auth::credentials::hash(&token)).bind(user).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO audit(user_id,action,target) VALUES($1,'identity.github.login',$1::text)",
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
    complete(&s.pool, &s.config, h, q, TOKEN, USER).await
}
async fn complete(
    pool: &PgPool,
    config: &GatewayConfig,
    h: HeaderMap,
    q: Callback,
    token_url: &str,
    user_url: &str,
) -> Result<Response> {
    if config.auth.mode != "github" {
        return Err(Error::missing());
    }
    let state = q
        .state
        .filter(|v| v.len() == 64)
        .ok_or_else(|| Error::bad("invalid or expired GitHub login"))?;
    let browser = cookie(&h)
        .filter(|v| v.len() == 64)
        .ok_or_else(|| Error::bad("GitHub login browser does not match; try again"))?;
    // Consume before network I/O, including denial. A callback is never replayable.
    let row=sqlx::query("DELETE FROM github_login_transactions WHERE state_hash=$1 AND browser_hash=$2 AND expires_at>now() RETURNING verifier,return_to").bind(auth::credentials::hash(&state)).bind(auth::credentials::hash(browser)).fetch_optional(pool).await?.ok_or_else(||Error::bad("invalid or expired GitHub login"))?;
    let mut response = if q.error.is_some() {
        Redirect::to("/?auth_error=github_denied").into_response()
    } else {
        let code = q
            .code
            .filter(|v| !v.is_empty() && v.len() <= 1024)
            .ok_or_else(|| Error::bad("GitHub authorization code is missing"))?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Wakeit GitHub Login")
            .build()?;
        let identity = remote_identity(
            &client,
            config,
            &code,
            &row.get::<String, _>("verifier"),
            token_url,
            user_url,
        )
        .await?;
        let token = sign_in(pool, &identity).await?;
        let target: String = row.get("return_to");
        // Finish in a same-origin document before navigating. A redirect chain
        // from GitHub can otherwise omit the new Strict session cookie.
        let nonce = auth::credentials::secret();
        let target = serde_json::to_string(&target)?.replace('<', "\\u003c");
        let mut response=Html(format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>Wakeit</title></head><body><script nonce=\"{nonce}\">location.replace({target})</script><noscript><a href=\"/\">Continue to Wakeit</a></noscript></body></html>")).into_response();
        response.headers_mut().insert(header::CONTENT_SECURITY_POLICY,format!("default-src 'none'; script-src 'nonce-{nonce}'; frame-ancestors 'none'; base-uri 'none'").parse().unwrap());
        response.headers_mut().append(
            header::SET_COOKIE,
            format!(
                "{}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=604800{}",
                config.auth.session_cookie,
                if config.public_url.starts_with("https:") {
                    "; Secure"
                } else {
                    ""
                }
            )
            .parse()
            .unwrap(),
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
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn ok<T>(result: Result<T>) -> T {
        result.unwrap_or_else(|e| panic!("{}: {}", e.0, e.1))
    }
    fn fixture_config() -> GatewayConfig {
        serde_json::from_value(json!({"version":1,"public_url":"http://127.0.0.1:58620","database_url_env":"WAKEIT_TEST_DATABASE_URL","nats_url":"nats://127.0.0.1:1","nats_public_url":"ws://127.0.0.1:2","nats_auth_dir":"unused","auth":{"mode":"github","github":{"client_id_env":"WAKEIT_TEST_GITHUB_CLIENT_ID","client_secret_env":"WAKEIT_TEST_GITHUB_CLIENT_SECRET"},"demo_workspace":true}})).unwrap()
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
    fn login_state(response: &Response) -> String {
        reqwest::Url::parse(response.headers()[header::LOCATION].to_str().unwrap())
            .unwrap()
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned()
    }
    #[tokio::test]
    #[ignore = "requires a disposable migrated PostgreSQL database; scripts/github_auth_checks.py"]
    async fn database_oauth_identity_and_demo() {
        let config = fixture_config();
        crate::auth::credentials::validate_github(&config).unwrap();
        let pool = crate::store::pool(&config.database_url_env).await.unwrap();
        let initial = ok(begin(
            &pool,
            &config,
            HeaderMap::new(),
            Start {
                return_to: Some("/invite#fixture-token".into()),
            },
        )
        .await);
        assert_eq!(initial.headers()[header::CACHE_CONTROL], "no-store");
        assert!(
            initial.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Lax")
        );
        let auth_url =
            reqwest::Url::parse(initial.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        assert_eq!(auth_url.host_str(), Some("github.com"));
        let params: std::collections::HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
        assert_eq!(params["scope"], "");
        assert_eq!(params["code_challenge_method"], "S256");
        assert!(
            !initial.headers()[header::LOCATION]
                .to_str()
                .unwrap()
                .contains("fixture-token")
        );
        let state = login_state(&initial);
        let headers = browser_headers(&initial);
        let bad = Callback {
            state: Some(state.clone()),
            code: Some("fixture-code".into()),
            error: None,
        };
        assert!(
            complete(&pool, &config, HeaderMap::new(), bad, TOKEN, USER)
                .await
                .is_err()
        );
        let bad = Callback {
            state: Some(auth::credentials::secret()),
            code: Some("fixture-code".into()),
            error: None,
        };
        assert!(
            complete(&pool, &config, headers.clone(), bad, TOKEN, USER)
                .await
                .is_err()
        );
        let verifier: String = sqlx::query_scalar(
            "SELECT verifier FROM github_login_transactions WHERE state_hash=$1",
        )
        .bind(auth::credentials::hash(&state))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(params["code_challenge"], challenge(&verifier));
        let expected_verifier = verifier.clone();
        let subject = rand::random::<u32>() as u64 + 1;
        let login = format!("collision-{subject}");
        let provider_login = login.clone();
        let local = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,username,password_hash,role,instance_admin) VALUES($1,$2,NULL,'viewer',true)").bind(local).bind(format!("@{login}")).execute(&pool).await.unwrap();
        let app = Router::new()
            .route(
                "/token",
                axum::routing::post(
                    move |axum::Form(form): axum::Form<
                        std::collections::HashMap<String, String>,
                    >| {
                        let verifier = expected_verifier.clone();
                        async move {
                            assert_eq!(form["client_secret"], "fixture-client-secret");
                            assert_eq!(form["code_verifier"], verifier);
                            assert_eq!(form["code"], "fixture-code");
                            axum::Json(json!({"access_token":"fixture-github-token"}))
                        }
                    },
                ),
            )
            .route(
                "/user",
                get(move |h: HeaderMap| {
                    let login = provider_login.clone();
                    async move {
                        assert_eq!(h[header::AUTHORIZATION], "Bearer fixture-github-token");
                        axum::Json(json!({"id":subject,"login":login}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mock = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let callback = Callback {
            state: Some(state.clone()),
            code: Some("fixture-code".into()),
            error: None,
        };
        let response = ok(complete(
            &pool,
            &config,
            headers.clone(),
            callback,
            &format!("{url}/token"),
            &format!("{url}/user"),
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
                .any(|v| v.starts_with("wakeit_github=;") && v.contains("Max-Age=0"))
        );
        let body = axum::body::to_bytes(response.into_body(), 8192)
            .await
            .unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("location.replace(\"/invite#fixture-token\")")
        );
        let duplicate = Callback {
            state: Some(state),
            code: Some("fixture-code".into()),
            error: None,
        };
        assert!(
            complete(
                &pool,
                &config,
                headers,
                duplicate,
                &format!("{url}/token"),
                &format!("{url}/user")
            )
            .await
            .is_err()
        );
        let external: Uuid = sqlx::query_scalar(
            "SELECT user_id FROM user_identities WHERE provider='github' AND subject=$1",
        )
        .bind(subject.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_ne!(external, local);
        let elevated: bool = sqlx::query_scalar("SELECT instance_admin FROM users WHERE id=$1")
            .bind(external)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!elevated);
        let memberships: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workspace_members WHERE user_id=$1")
                .bind(external)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(memberships, 0);
        let token = ok(sign_in(
            &pool,
            &GitHubUser {
                id: subject,
                login: "renamed-login".into(),
            },
        )
        .await);
        let identity = crate::auth::workspace::identity(&pool, &auth::credentials::hash(&token))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(identity.id, external);
        crate::store::demo::seed(&pool).await.unwrap();
        crate::store::demo::seed(&pool).await.unwrap();
        let demo: Uuid = sqlx::query_scalar("SELECT id FROM workspaces WHERE is_demo")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            crate::auth::workspace::authorize(&pool, &identity, demo)
                .await
                .unwrap()
                .unwrap()
                .demo
        );
        let ordinary = crate::auth::workspace::Identity {
            id: local,
            username: "local".into(),
            instance_admin: true,
            kind: "session".into(),
            workspace_id: None,
            credential: String::new(),
        };
        assert!(
            crate::auth::workspace::authorize(&pool, &ordinary, demo)
                .await
                .unwrap()
                .is_none()
        );
        let mut tx = pool.begin().await.unwrap();
        assert!(
            crate::auth::workspace::check_member(
                &mut tx,
                &auth::credentials::hash(&token),
                demo,
                "viewer"
            )
            .await
            .is_err()
        );
        tx.rollback().await.unwrap();
        let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM runs WHERE workspace_id=$1")
            .bind(demo)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(runs, 3);
        sqlx::query("UPDATE users SET active=false WHERE id=$1")
            .bind(external)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            sign_in(&pool, &GitHubUser { id: subject, login })
                .await
                .is_err()
        );
        mock.abort();
    }
    #[tokio::test]
    #[ignore = "requires a disposable migrated PostgreSQL database; scripts/github_auth_checks.py"]
    async fn database_denial_expiry_replay_and_limits() {
        let config = fixture_config();
        let pool = crate::store::pool(&config.database_url_env).await.unwrap();
        let first = ok(begin(&pool, &config, HeaderMap::new(), Start::default()).await);
        let state = login_state(&first);
        let headers = browser_headers(&first);
        let denied = Callback {
            state: Some(state.clone()),
            code: None,
            error: Some("access_denied".into()),
        };
        let response = ok(complete(&pool, &config, headers.clone(), denied, TOKEN, USER).await);
        assert_eq!(
            response.headers()[header::LOCATION],
            "/?auth_error=github_denied"
        );
        let duplicate = Callback {
            state: Some(state),
            code: None,
            error: Some("access_denied".into()),
        };
        assert!(
            complete(&pool, &config, headers.clone(), duplicate, TOKEN, USER)
                .await
                .is_err()
        );
        let expired = ok(begin(&pool, &config, headers.clone(), Start::default()).await);
        let expired = login_state(&expired);
        sqlx::query("UPDATE github_login_transactions SET expires_at=now()-interval '1 second' WHERE state_hash=$1").bind(auth::credentials::hash(&expired)).execute(&pool).await.unwrap();
        assert!(
            complete(
                &pool,
                &config,
                headers.clone(),
                Callback {
                    state: Some(expired),
                    code: None,
                    error: Some("access_denied".into())
                },
                TOKEN,
                USER
            )
            .await
            .is_err()
        );
        for _ in 0..8 {
            ok(begin(&pool, &config, headers.clone(), Start::default()).await);
        }
        let limited = begin(&pool, &config, headers, Start::default()).await;
        assert_eq!(limited.err().unwrap().0, StatusCode::TOO_MANY_REQUESTS);
    }
    #[test]
    fn return_paths_and_pkce() {
        for path in [
            "/",
            "/w/123/tasks",
            "/invite#secret",
            "/authorize/device?user_code=ABC",
        ] {
            assert!(safe_return(path));
        }
        for path in [
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "/api/v1/auth/github/start",
            "/\r\nLocation:x",
        ] {
            assert!(!safe_return(path));
        }
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
}
