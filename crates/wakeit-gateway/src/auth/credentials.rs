use crate::config::GatewayConfig;
use anyhow::{Result, ensure};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString, rand_core::OsRng},
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn secret() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}
pub fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
pub fn password(password: &str) -> Result<String> {
    ensure!(
        (12..=1024).contains(&password.len()),
        "password must contain 12–1024 bytes"
    );
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .to_string())
}
pub fn verify(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|p| {
        Argon2::default()
            .verify_password(password.as_bytes(), &p)
            .is_ok()
    })
}
pub fn role(role: &str) -> u8 {
    match role {
        "admin" => 3,
        "operator" => 2,
        "viewer" => 1,
        _ => 0,
    }
}
pub async fn bootstrap(pool: &PgPool, username: &str, pw: &str) -> Result<()> {
    ensure!(
        !username.trim().is_empty() && username.len() <= 100,
        "invalid username"
    );
    let pw = password(pw)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(743273)")
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT count(*) AS count FROM users")
        .fetch_one(&mut *tx)
        .await?;
    ensure!(
        row.get::<i64, _>("count") == 0,
        "workspace already initialized"
    );
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,username,password_hash,role,instance_admin) VALUES($1,$2,$3,'viewer',true)")
        .bind(id)
        .bind(username)
        .bind(pw)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workspace_members(workspace_id,user_id,role) SELECT id,$1,'admin' FROM workspaces WHERE is_default")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) fn validate_github(config: &GatewayConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        ["local", "github"].contains(&config.auth.mode.as_str()),
        "auth.mode must be local or github"
    );
    wakeit_core::device_auth::validate_url(&config.public_url)?;
    anyhow::ensure!(
        !config.auth.session_cookie.is_empty()
            && config.auth.session_cookie.len() <= 64
            && config
                .auth
                .session_cookie
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid session cookie name"
    );
    if config.auth.mode == "github" {
        github_credentials(config)?;
        anyhow::ensure!(
            !config.auth.allow_registration,
            "password registration cannot be enabled in GitHub mode"
        );
    }
    anyhow::ensure!(
        !config.auth.demo_workspace || config.auth.mode == "github",
        "demo workspace requires GitHub authentication"
    );
    Ok(())
}
pub(crate) fn github_credentials(config: &GatewayConfig) -> anyhow::Result<(String, String)> {
    let provider = config
        .auth
        .github
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitHub authentication configuration is required"))?;
    let id = std::env::var(&provider.client_id_env)
        .map_err(|_| anyhow::anyhow!("GitHub Client ID environment variable is missing"))?;
    let secret = std::env::var(&provider.client_secret_env)
        .map_err(|_| anyhow::anyhow!("GitHub Client Secret environment variable is missing"))?;
    anyhow::ensure!(
        !id.trim().is_empty() && !secret.trim().is_empty(),
        "GitHub credentials must not be empty"
    );
    Ok((id, secret))
}
