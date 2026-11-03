//! Generic OAuth flows. Verifiers, device codes and tokens remain in the Runner database.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub async fn initialize(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS extension_auth_flows(id TEXT PRIMARY KEY,instance TEXT NOT NULL,status TEXT NOT NULL,state_hash TEXT NOT NULL,data TEXT NOT NULL,error TEXT,expires_at TEXT NOT NULL)").execute(pool).await?;
    Ok(())
}
async fn save_token(pool: &SqlitePool, instance: Uuid, slot: &str, token: Value) -> Result<()> {
    ensure!(
        token["access_token"].as_str().is_some(),
        "OAuth token response has no access token"
    );
    let mut token = token;
    token["obtained_at"] = json!(Utc::now());
    let mut tx = pool.begin().await?;
    let value: String = sqlx::query_scalar("SELECT secrets FROM extension_instances WHERE id=?")
        .bind(instance.to_string())
        .fetch_one(&mut *tx)
        .await?;
    let mut value: Value = serde_json::from_str(&value)?;
    value[slot] = Value::String(token.to_string());
    sqlx::query("UPDATE extension_instances SET secrets=?,revision=revision+1 WHERE id=?")
        .bind(value.to_string())
        .bind(instance.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
async fn token_request(
    client: &reqwest::Client,
    url: &str,
    fields: &[(String, String)],
) -> Result<Value> {
    let response = client
        .post(url)
        .form(fields)
        .timeout(Duration::from_secs(20))
        .send()
        .await?;
    let status = response.status();
    let value: Value = response.json().await?;
    if !status.is_success() {
        return Ok(json!({"error":value["error"].as_str().unwrap_or("token_exchange_failed")}));
    }
    Ok(value)
}
pub async fn begin(
    pool: &SqlitePool,
    client: &reqwest::Client,
    instance: Uuid,
    params: &Value,
    gateway: &str,
    device_token: &str,
    stop: CancellationToken,
) -> Result<Value> {
    let flow = Uuid::new_v4();
    let mode = params["mode"].as_str().context("OAuth mode required")?;
    let slot = params["slot"]
        .as_str()
        .context("credential slot required")?;
    let client_id = params["client_id"]
        .as_str()
        .context("OAuth client ID required")?;
    let token_url = params["token_url"].as_str().context("token URL required")?;
    let state = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let verifier = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let expires = Utc::now() + chrono::Duration::minutes(10);
    let mut data = params.clone();
    data["verifier"] = json!(verifier);
    data["state"] = json!(state);
    let mut public = json!({"id":flow,"state":"pending","expires_at":expires});
    if mode == "authorization_code" {
        let response=client.post(format!("{}/api/v1/device/plugins/auth/register",gateway.trim_end_matches('/'))).bearer_auth(device_token.trim()).json(&json!({"id":flow,"instance_id":instance,"state_hash":hex::encode(Sha256::digest(state.as_bytes())),"expires_at":expires})).send().await?;
        ensure!(
            response.status().is_success(),
            "OAuth callback registration failed"
        );
        let registration: Value = response.json().await?;
        let callback = registration["callback_url"]
            .as_str()
            .context("OAuth callback URL unavailable")?;
        data["redirect_uri"] = json!(callback);
        let mut url = reqwest::Url::parse(
            params["authorization_url"]
                .as_str()
                .context("authorization URL required")?,
        )?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", callback)
            .append_pair("state", &state)
            .append_pair(
                "code_challenge",
                &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
            )
            .append_pair("code_challenge_method", "S256")
            .append_pair("scope", params["scope"].as_str().unwrap_or(""));
        public["url"] = json!(url.as_str());
    } else if mode == "device_code" {
        let response = client
            .post(
                params["device_authorization_url"]
                    .as_str()
                    .context("device authorization URL required")?,
            )
            .form(&[
                ("client_id", client_id),
                ("scope", params["scope"].as_str().unwrap_or("")),
            ])
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "device authorization failed"
        );
        let response: Value = response.json().await?;
        data["device_code"] = response["device_code"].clone();
        data["interval"] = response["interval"].clone();
        public["url"] = response["verification_uri"].clone();
        public["code"] = response["user_code"].clone();
    } else {
        anyhow::bail!("unsupported OAuth flow");
    }
    sqlx::query("INSERT INTO extension_auth_flows(id,instance,status,state_hash,data,expires_at) VALUES(?,?,'pending',?,?,?)").bind(flow.to_string()).bind(instance.to_string()).bind(hex::encode(Sha256::digest(state.as_bytes()))).bind(data.to_string()).bind(expires.to_rfc3339()).execute(pool).await?;
    if mode == "device_code" {
        let pool = pool.clone();
        let client = client.clone();
        let token_url = token_url.to_owned();
        let client_id = client_id.to_owned();
        let slot = slot.to_owned();
        tokio::spawn(async move {
            let mut interval = data["interval"].as_u64().unwrap_or(5).max(1);
            loop {
                tokio::select! {_=stop.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(interval))=>{}}
                if Utc::now() > expires {
                    let _=sqlx::query("UPDATE extension_auth_flows SET status='expired' WHERE id=? AND status='pending'").bind(flow.to_string()).execute(&pool).await;
                    break;
                }
                let status = sqlx::query_scalar::<_, String>(
                    "SELECT status FROM extension_auth_flows WHERE id=?",
                )
                .bind(flow.to_string())
                .fetch_one(&pool)
                .await;
                if !status.is_ok_and(|s| s == "pending") {
                    break;
                }
                let fields = vec![
                    (
                        "grant_type".into(),
                        "urn:ietf:params:oauth:grant-type:device_code".into(),
                    ),
                    ("client_id".into(), client_id.clone()),
                    (
                        "device_code".into(),
                        data["device_code"].as_str().unwrap_or("").into(),
                    ),
                ];
                let Ok(token) = token_request(&client, &token_url, &fields).await else {
                    continue;
                };
                match token["error"].as_str() {
                    Some("authorization_pending") => continue,
                    Some("slow_down") => {
                        interval += 5;
                        continue;
                    }
                    Some(error) => {
                        let _ = sqlx::query(
                            "UPDATE extension_auth_flows SET status='failed',error=? WHERE id=?",
                        )
                        .bind(error)
                        .bind(flow.to_string())
                        .execute(&pool)
                        .await;
                        break;
                    }
                    None => {
                        if save_token(&pool, instance, &slot, token).await.is_ok() {
                            let _ = sqlx::query(
                                "UPDATE extension_auth_flows SET status='authorized' WHERE id=?",
                            )
                            .bind(flow.to_string())
                            .execute(&pool)
                            .await;
                        }
                        break;
                    }
                }
            }
        });
    }
    Ok(public)
}
pub async fn callback(
    pool: &SqlitePool,
    client: &reqwest::Client,
    state: &str,
    code: &str,
) -> Result<Value> {
    let row =
        sqlx::query("SELECT * FROM extension_auth_flows WHERE state_hash=? AND status='pending'")
            .bind(hex::encode(Sha256::digest(state.as_bytes())))
            .fetch_one(pool)
            .await?;
    let expiry: DateTime<Utc> = row.get::<String, _>("expires_at").parse()?;
    ensure!(Utc::now() < expiry, "OAuth flow expired");
    let data: Value = serde_json::from_str(row.get("data"))?;
    let fields = [
        ("grant_type".into(), "authorization_code".into()),
        ("code".into(), code.into()),
        (
            "client_id".into(),
            data["client_id"]
                .as_str()
                .context("client ID missing")?
                .into(),
        ),
        (
            "redirect_uri".into(),
            data["redirect_uri"]
                .as_str()
                .context("redirect URI missing")?
                .into(),
        ),
        (
            "code_verifier".into(),
            data["verifier"]
                .as_str()
                .context("PKCE verifier missing")?
                .into(),
        ),
    ];
    let token = token_request(
        client,
        data["token_url"]
            .as_str()
            .context("token endpoint missing")?,
        &fields,
    )
    .await?;
    ensure!(token["error"].is_null(), "OAuth code exchange failed");
    save_token(
        pool,
        row.get::<String, _>("instance").parse()?,
        data["slot"].as_str().context("credential slot missing")?,
        token,
    )
    .await?;
    sqlx::query("UPDATE extension_auth_flows SET status='authorized',data='{}' WHERE id=?")
        .bind(row.get::<String, _>("id"))
        .execute(pool)
        .await?;
    Ok(json!({"state":"authorized"}))
}
pub async fn status(pool: &SqlitePool, instance: Uuid, id: &str, cancel: bool) -> Result<Value> {
    if cancel {
        sqlx::query("UPDATE extension_auth_flows SET status='cancelled',data='{}' WHERE id=? AND instance=? AND status='pending'").bind(id).bind(instance.to_string()).execute(pool).await?;
    }
    let row = sqlx::query(
        "SELECT id,status,expires_at,error FROM extension_auth_flows WHERE id=? AND instance=?",
    )
    .bind(id)
    .bind(instance.to_string())
    .fetch_one(pool)
    .await?;
    Ok(
        json!({"id":row.get::<String,_>("id"),"state":row.get::<String,_>("status"),"expires_at":row.get::<String,_>("expires_at"),"error":row.get::<Option<String>,_>("error")}),
    )
}

pub async fn token(
    pool: &SqlitePool,
    client: &reqwest::Client,
    instance: Uuid,
    slot: &str,
    token_url: Option<&str>,
    client_id: Option<&str>,
) -> Result<Value> {
    let row: String = sqlx::query_scalar("SELECT secrets FROM extension_instances WHERE id=?")
        .bind(instance.to_string())
        .fetch_one(pool)
        .await?;
    let secrets: Value = serde_json::from_str(&row)?;
    let mut token: Value = serde_json::from_str(
        secrets[slot]
            .as_str()
            .context("OAuth credential not configured")?,
    )?;
    let obtained = token["obtained_at"]
        .as_str()
        .and_then(|v| v.parse::<DateTime<Utc>>().ok());
    let expires = token["expires_in"].as_i64();
    if obtained.zip(expires).is_some_and(|(at, seconds)| {
        Utc::now() + chrono::Duration::seconds(30) >= at + chrono::Duration::seconds(seconds)
    }) {
        let refresh = token["refresh_token"]
            .as_str()
            .context("OAuth authorization expired; sign in again")?
            .to_owned();
        let fields = vec![
            ("grant_type".into(), "refresh_token".into()),
            ("refresh_token".into(), refresh.clone()),
            (
                "client_id".into(),
                client_id
                    .context("OAuth client ID required for refresh")?
                    .into(),
            ),
        ];
        let mut updated = token_request(
            client,
            token_url.context("OAuth token URL required for refresh")?,
            &fields,
        )
        .await?;
        ensure!(updated["error"].is_null(), "OAuth token refresh failed");
        if updated["refresh_token"].is_null() {
            updated["refresh_token"] = json!(refresh);
        }
        save_token(pool, instance, slot, updated.clone()).await?;
        token = updated;
    }
    Ok(token)
}
pub async fn revoke(
    pool: &SqlitePool,
    client: &reqwest::Client,
    instance: Uuid,
    slot: &str,
    endpoint: Option<&str>,
) -> Result<Value> {
    let mut secrets: Value = serde_json::from_str(
        &sqlx::query_scalar::<_, String>("SELECT secrets FROM extension_instances WHERE id=?")
            .bind(instance.to_string())
            .fetch_one(pool)
            .await?,
    )?;
    if let Some(endpoint) = endpoint
        && let Some(value) = secrets[slot].as_str()
    {
        let token: Value = serde_json::from_str(value)?;
        let response = client
            .post(endpoint)
            .form(&[(
                "token",
                token["refresh_token"]
                    .as_str()
                    .or(token["access_token"].as_str())
                    .context("OAuth token missing")?,
            )])
            .timeout(Duration::from_secs(20))
            .send()
            .await?;
        ensure!(response.status().is_success(), "OAuth revocation failed");
    }
    secrets
        .as_object_mut()
        .context("invalid credential store")?
        .remove(slot);
    sqlx::query("UPDATE extension_instances SET secrets=?,revision=revision+1 WHERE id=?")
        .bind(secrets.to_string())
        .bind(instance.to_string())
        .execute(pool)
        .await?;
    Ok(json!({"state":"revoked"}))
}
