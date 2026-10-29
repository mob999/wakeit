//! Gateway-owned orchestration. Runners only receive ordinary version-1 assignments.
use crate::config::TaskConfig;
use anyhow::Result;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Debug)]
pub(crate) struct RunnerBindingError(String);

impl std::fmt::Display for RunnerBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for RunnerBindingError {}

pub async fn runner(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    c: &TaskConfig,
) -> Result<Uuid> {
    let row=sqlx::query("SELECT id,agents FROM runners WHERE workspace_id=$1 AND (id::text=$2 OR name=$2) AND NOT revoked")
        .bind(workspace).bind(&c.runner).fetch_optional(&mut **tx).await?
        .ok_or_else(|| RunnerBindingError("runner does not exist or is revoked".into()))?;
    let agents: Value = row.get("agents");
    let agent = agents
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == c.agent))
        .ok_or_else(|| RunnerBindingError("agent is not advertised by this runner".into()))?;
    if c.session.is_some() && agent["session_supported"] != true {
        return Err(RunnerBindingError("agent does not advertise session support".into()).into());
    }
    crate::config::resolve_parameters(
        &serde_json::from_value(agent.get("parameters").cloned().unwrap_or(json!({})))?,
        &c.parameters,
    )
    .map_err(|e| RunnerBindingError(e.to_string()))?;
    Ok(row.get("id"))
}
