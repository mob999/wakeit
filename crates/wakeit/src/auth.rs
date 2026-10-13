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
    sqlx::query("INSERT INTO users(id,username,password_hash,role) VALUES($1,$2,$3,'admin')")
        .bind(Uuid::new_v4())
        .bind(username)
        .bind(pw)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
