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
        ["local", "github", "gitlab"].contains(&config.auth.mode.as_str()),
        "auth.mode must be local, github or gitlab"
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
    match config.auth.mode.as_str() {
        "github" => {
            github_credentials(config)?;
            anyhow::ensure!(
                !config.auth.allow_registration,
                "password registration cannot be enabled in GitHub mode"
            );
        }
        "gitlab" => {
            let provider = config.auth.gitlab.as_ref().ok_or_else(|| {
                anyhow::anyhow!("GitLab authentication configuration is required")
            })?;
            gitlab_origin(config)?;
            anyhow::ensure!(
                provider.client_id_env.is_some() || provider.allow_token,
                "GitLab authentication requires client_id_env or allow_token"
            );
            if provider.client_secret_env.is_some() {
                anyhow::ensure!(
                    provider.client_id_env.is_some(),
                    "GitLab client_secret_env requires client_id_env"
                );
            }
            if provider.client_id_env.is_some() {
                gitlab_oauth_credentials(config)?;
            }
            anyhow::ensure!(
                !config.auth.allow_registration,
                "password registration cannot be enabled in GitLab mode"
            );
        }
        _ => {}
    }
    anyhow::ensure!(
        !config.auth.demo_workspace || matches!(config.auth.mode.as_str(), "github" | "gitlab"),
        "demo workspace requires GitHub or GitLab authentication"
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
pub(crate) fn gitlab_origin(config: &GatewayConfig) -> anyhow::Result<String> {
    let provider = config
        .auth
        .gitlab
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitLab authentication configuration is required"))?;
    wakeit_core::device_auth::validate_url(provider.url.trim()).map_err(|_| {
        anyhow::anyhow!(
            "GitLab URL must be an HTTPS origin without credentials, path, query or fragment (HTTP is allowed only on loopback)"
        )
    })
}
pub(crate) fn gitlab_oauth_credentials(
    config: &GatewayConfig,
) -> anyhow::Result<(String, Option<String>)> {
    let provider = config
        .auth
        .gitlab
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GitLab authentication configuration is required"))?;
    let id_env = provider
        .client_id_env
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("GitLab OAuth is not configured"))?;
    let id = std::env::var(id_env)
        .map_err(|_| anyhow::anyhow!("GitLab Client ID environment variable is missing"))?;
    anyhow::ensure!(
        !id.trim().is_empty(),
        "GitLab credentials must not be empty"
    );
    let secret = match &provider.client_secret_env {
        Some(secret_env) => {
            let value = std::env::var(secret_env).map_err(|_| {
                anyhow::anyhow!("GitLab Client Secret environment variable is missing")
            })?;
            anyhow::ensure!(
                !value.trim().is_empty(),
                "GitLab credentials must not be empty"
            );
            Some(value)
        }
        None => None,
    };
    Ok((id, secret))
}
