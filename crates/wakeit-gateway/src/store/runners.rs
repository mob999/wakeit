//! Gateway-owned orchestration. Runners only receive ordinary version-1 assignments.
use anyhow::Result;
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;
use wakeit_core::config::TaskConfig;

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
    let row=sqlx::query("SELECT id,agents,info FROM runners WHERE workspace_id=$1 AND (id::text=$2 OR name=$2) AND NOT revoked")
        .bind(workspace).bind(&c.runner).fetch_optional(&mut **tx).await?
        .ok_or_else(|| RunnerBindingError("runner does not exist or is revoked".into()))?;
    let agents: Value = row.get("agents");
    let agent = agents
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == c.agent))
        .ok_or_else(|| RunnerBindingError("agent is not advertised by this runner".into()))?;
    let (parameters, session_supported) = if let Some(instance) = c.agent_instance {
        let info: Value = row.get("info");
        if info["agent_instances"] != 1 {
            return Err(RunnerBindingError(
                "runner does not support explicit Agent instances".into(),
            )
            .into());
        }
        let instance=sqlx::query("SELECT source,extension,declaration FROM plugin_instances WHERE id=$1 AND runner_id=$2 AND workspace_id=$3 AND enabled AND state!='removed'")
            .bind(instance).bind(row.get::<Uuid,_>("id")).bind(workspace).fetch_optional(&mut **tx).await?
            .ok_or_else(||RunnerBindingError("Agent instance is unavailable or disabled".into()))?;
        if agent["plugin_source"].as_str() != Some(instance.get::<String, _>("source").as_str())
            || agent["plugin_resource"].as_str()
                != Some(instance.get::<String, _>("extension").as_str())
        {
            return Err(RunnerBindingError(
                "Agent instance belongs to another selected source or resource".into(),
            )
            .into());
        }
        let declaration: wakeit_core::extensions::Extension =
            serde_json::from_value(instance.get("declaration"))?;
        let descriptor = declaration.agents.get(&c.agent).ok_or_else(|| {
            RunnerBindingError("plugin instance does not provide this Agent".into())
        })?;
        use wakeit_plugin_protocol::adapter::AgentOperation;
        (
            descriptor.parameters.clone(),
            [
                AgentOperation::SessionInspect,
                AgentOperation::SessionResume,
                AgentOperation::SessionInteractive,
            ]
            .iter()
            .all(|op| descriptor.operations.contains(op)),
        )
    } else {
        (
            serde_json::from_value(agent.get("parameters").cloned().unwrap_or(json!({})))?,
            agent["session_supported"] == true,
        )
    };
    if c.session.is_some() && !session_supported {
        return Err(RunnerBindingError("agent does not advertise session support".into()).into());
    }
    wakeit_core::config::resolve_parameters(&parameters, &c.parameters)
        .map_err(|e| RunnerBindingError(e.to_string()))?;
    Ok(row.get("id"))
}
