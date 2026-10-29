pub(crate) mod dag;
pub(crate) mod dag_recovery;
pub(crate) mod dag_retry;
pub(crate) mod demo;
pub(crate) mod events;
pub(crate) mod outbox;
pub(crate) mod runners;
pub(crate) mod runs;
pub(crate) mod scheduler;
pub(crate) mod tasks;
pub(crate) mod terminals;
pub(crate) mod workspaces;
use anyhow::{Context, Result};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

pub async fn pool(env: &str) -> Result<PgPool> {
    Ok(sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&std::env::var(env).with_context(|| format!("set {env}"))?)
        .await?)
}

pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    workspace: Option<Uuid>,
    action: &str,
    target: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO audit(user_id,action,target,workspace_id) VALUES($1,$2,$3,$4)")
        .bind(user)
        .bind(action)
        .bind(target)
        .bind(workspace)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(Debug)]
pub(crate) enum Error {
    Missing,
    Conflict(String),
}
impl Error {
    pub(crate) fn missing() -> anyhow::Error {
        Self::Missing.into()
    }
    pub(crate) fn conflict(message: impl Into<String>) -> anyhow::Error {
        Self::Conflict(message.into()).into()
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("not found"),
            Self::Conflict(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for Error {}
