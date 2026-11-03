//! Shared extension contract. Agent configuration and Agent output remain version 1.
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const SDK_PROTOCOL: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Extension {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub translations: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default = "object_schema")]
    pub config: Value,
    #[serde(default)]
    pub credentials: BTreeMap<String, CredentialSlot>,
    #[serde(default)]
    pub handlers: BTreeMap<String, Handler>,
    #[serde(default)]
    pub hooks: BTreeMap<String, Hook>,
    #[serde(default)]
    pub actions: BTreeMap<String, Action>,
    #[serde(default)]
    pub providers: BTreeMap<String, Provider>,
    #[serde(default)]
    pub events: BTreeMap<String, Value>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "sdk_version")]
    pub sdk_protocol: u32,
}
fn sdk_version() -> u32 {
    SDK_PROTOCOL
}
fn object_schema() -> Value {
    json!({"type":"object","properties":{}})
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialSlot {
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    pub events: Vec<String>,
    #[serde(default = "handler_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub description: String,
}
fn handler_timeout() -> u64 {
    60
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub stage: String,
    #[serde(default = "handler_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub description: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "object_schema")]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default)]
    pub side_effects: bool,
    #[serde(default = "handler_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub permission: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    pub kind: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Instance {
    pub id: Uuid,
    pub runner_id: Uuid,
    pub source: String,
    pub extension: String,
    pub name: String,
    pub revision: i64,
    pub enabled: bool,
    pub version: String,
    pub grants: Vec<String>,
    pub config: Value,
    #[serde(default)]
    pub credential_status: BTreeMap<String, bool>,
    pub state: String,
    pub applied_revision: Option<i64>,
    pub error: Option<String>,
    pub declaration: Extension,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", content = "value", rename_all = "snake_case")]
pub enum SecretUpdate {
    Keep,
    Replace(String),
    Clear,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigUpdate {
    pub operation_id: Uuid,
    pub expected_revision: i64,
    pub config: Value,
    #[serde(default)]
    pub secrets: BTreeMap<String, SecretUpdate>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Actor {
    pub kind: String,
    pub id: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Event {
    pub id: Uuid,
    pub event_type: String,
    pub schema_version: u32,
    pub workspace_id: Uuid,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub aggregate_revision: i64,
    pub occurred_at: DateTime<Utc>,
    pub committed_at: DateTime<Utc>,
    pub operation_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub causation_id: Option<Uuid>,
    pub origin: Actor,
    pub source_runner_id: Option<Uuid>,
    pub data: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    Reject,
    Wait,
    Continue,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub runner_id: Uuid,
    pub handler: String,
    pub kind: String,
    pub event: String,
    pub enabled: bool,
    #[serde(default)]
    pub filter: BTreeMap<String, Value>,
    #[serde(default)]
    pub position: i32,
    pub failure_policy: FailurePolicy,
    pub timeout_seconds: u64,
    pub max_attempts: u32,
    pub expires_seconds: u64,
    pub revision: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny {
        code: String,
        message: String,
        #[serde(default)]
        paths: Vec<String>,
    },
    Patch {
        value: Value,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Invocation {
    pub id: Uuid,
    pub operation_id: Uuid,
    pub instance_id: Uuid,
    pub kind: String,
    pub handler: String,
    pub input: Value,
    pub deadline: DateTime<Utc>,
    pub expected_revision: Option<i64>,
    pub actor: Actor,
    pub causation_id: Option<Uuid>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Receipt {
    pub id: Uuid,
    pub state: String,
    pub revision: i64,
    pub result: Option<Value>,
    pub error: Option<ExtensionError>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExtensionError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(default)]
    pub paths: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Interaction {
    Progress {
        message: String,
        fraction: Option<f64>,
    },
    Form {
        title: String,
        schema: Value,
        #[serde(default)]
        value: Value,
    },
    Secret {
        slot: String,
        label: String,
    },
    Confirm {
        title: String,
        description: String,
    },
    Choice {
        title: String,
        options: Vec<Value>,
    },
    OpenUrl {
        url: String,
        label: String,
    },
    DeviceCode {
        url: String,
        code: String,
        expires_at: DateTime<Utc>,
    },
    Download {
        reference: String,
        name: String,
    },
    Navigate {
        path: String,
    },
    Toast {
        message: String,
    },
}

pub const EVENT_TYPES: &[&str] = &[
    "workspace.created",
    "workspace.settings_changed",
    "member.joined",
    "member.left",
    "member.role_changed",
    "task.created",
    "task.updated",
    "task.enabled_changed",
    "task.schedule_changed",
    "task.archived",
    "task.restored",
    "template.created",
    "template.updated",
    "template.archived",
    "template.restored",
    "template.instantiated",
    "schedule.occurrence_created",
    "schedule.evaluated",
    "schedule.next_changed",
    "run.created",
    "run.queued",
    "run.started",
    "run.state_changed",
    "run.cancel_requested",
    "run.retry_requested",
    "run.reopened",
    "run.uncertain",
    "run.reconciled",
    "run.finished",
    "dag.node.ready",
    "dag.node.state_changed",
    "dag.node.retry_scheduled",
    "dag.node.attempt_created",
    "dag.node.attempt_finished",
    "dag.node.finished",
    "dag.recovery_planned",
    "dag.recovered",
    "execution.claimed",
    "execution.prepared",
    "execution.started",
    "execution.input_submitted",
    "execution.native_finished",
    "execution.cancel_requested",
    "execution.cancel_confirmed",
    "execution.uncertain",
    "execution.reconciled",
    "execution.finished",
    "execution.message_updated",
    "execution.tool_updated",
    "execution.log",
    "execution.recording_changed",
    "session.created",
    "session.bound",
    "session.title_changed",
    "session.availability_changed",
    "session.continuation_started",
    "session.continuation_finished",
    "result.native_available",
    "result.published",
    "result.derived_published",
    "result.processing_failed",
    "result.expired",
    "artifact.registered",
    "artifact.exported",
    "artifact.expired",
    "artifact.removed",
    "file.changed",
    "device.enrolled",
    "device.online",
    "device.offline",
    "device.capabilities_changed",
    "device.lease_lost",
    "device.lease_recovered",
    "device.revoked",
    "agent.availability_changed",
    "agent.catalog_changed",
    "plugin.installed",
    "plugin.updated",
    "plugin.removed",
    "instance.activating",
    "instance.activated",
    "instance.deactivated",
    "instance.failed",
    "instance.recovered",
    "instance.config_changed",
    "instance.permissions_changed",
    "credential.status_changed",
    "auth.flow_changed",
    "action.started",
    "action.progress",
    "action.finished",
    "operation.state_changed",
    "delivery.state_changed",
];
pub const HOOK_STAGES: &[(&str, &[&str])] = &[
    ("workspace.settings.prepare", &["settings"]),
    ("task.save.prepare", &["config"]),
    ("task.activation.prepare", &[]),
    ("template.save.prepare", &["definition"]),
    ("template.compile.prepare", &["values", "unset", "bindings"]),
    ("template.compile.result", &["config"]),
    ("schedule.fire.prepare", &["prompt", "parameters"]),
    ("run.create.prepare", &["prompt", "parameters"]),
    ("run.retry.prepare", &["prompt", "parameters"]),
    ("dag.recovery.prepare", &[]),
    ("dag.node.dispatch.prepare", &["prompt", "parameters"]),
    ("execution.prepare", &["prompt", "parameters"]),
    ("session.bind.prepare", &[]),
    ("session.continue.prepare", &["prompt", "parameters"]),
    ("session.title.prepare", &["title"]),
    (
        "result.publish.prepare",
        &["result", "artifacts", "metadata"],
    ),
    ("artifact.export.prepare", &["metadata"]),
    ("action.prepare", &["input"]),
];

pub fn apply_decision(stage: &str, proposal: &mut Value, decision: Decision) -> Result<()> {
    let allowed = HOOK_STAGES
        .iter()
        .find(|(s, _)| *s == stage)
        .context("unsupported hook stage")?
        .1;
    match decision {
        Decision::Allow => Ok(()),
        Decision::Deny { code, message, .. } => bail!("hook_rejected:{code}: {message}"),
        Decision::Patch { value } => {
            let patch = value.as_object().context("hook patch must be an object")?;
            ensure!(
                patch.keys().all(|k| allowed.contains(&k.as_str())),
                "hook patch changes a protected field"
            );
            let target = proposal
                .as_object_mut()
                .context("hook proposal must be an object")?;
            for (k, v) in patch {
                target.insert(k.clone(), v.clone());
            }
            Ok(())
        }
    }
}

/// Validate portable JSON Schema without enabling network or file reference retrieval.
pub fn validate_schema(schema: &Value, value: &Value) -> Result<()> {
    if schema.is_null() {
        return Ok(());
    }
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| anyhow::anyhow!("invalid configuration schema: {error}"))?;
    validator
        .validate(value)
        .map_err(|error| anyhow::anyhow!("{}: {}", error.instance_path, error))
}
impl Extension {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.sdk_protocol == SDK_PROTOCOL,
            "unsupported extension SDK protocol"
        );
        ensure!(
            !self.command.is_empty() && !self.name.is_empty(),
            "extension command and name are required"
        );
        for (id, h) in &self.handlers {
            ensure!(!id.is_empty() && !h.events.is_empty(), "invalid handler");
            for e in &h.events {
                ensure!(
                    EVENT_TYPES.contains(&e.as_str()) || e.starts_with("plugin:"),
                    "unsupported event: {e}"
                );
            }
        }
        for h in self.hooks.values() {
            ensure!(
                HOOK_STAGES.iter().any(|(s, _)| *s == h.stage),
                "unsupported hook stage: {}",
                h.stage
            );
        }
        for p in self.providers.values() {
            ensure!(
                ["models", "sessions", "artifacts"].contains(&p.kind.as_str()),
                "unsupported provider kind"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_changes_are_explicit_and_never_change_identity() {
        let mut value = json!({"prompt":"original","runner_id":"protected"});
        assert!(
            apply_decision(
                "execution.prepare",
                &mut value,
                Decision::Patch {
                    value: json!({"runner_id":"other"})
                }
            )
            .is_err()
        );
        assert_eq!(value["runner_id"], "protected");
        apply_decision(
            "execution.prepare",
            &mut value,
            Decision::Patch {
                value: json!({"prompt":"effective"}),
            },
        )
        .unwrap();
        assert_eq!(value["prompt"], "effective");
        assert!(
            apply_decision(
                "execution.prepare",
                &mut value,
                Decision::Deny {
                    code: "policy".into(),
                    message: "denied".into(),
                    paths: vec![]
                }
            )
            .is_err()
        );
    }
    #[test]
    fn configuration_checks_nested_values_and_conditional_requirements() {
        let schema = json!({"type":"object","properties":{"enabled":{"type":"boolean"},"targets":{"type":"array","minItems":1,"items":{"type":"object","properties":{"name":{"type":"string","minLength":1}},"required":["name"]}}},"if":{"properties":{"enabled":{"const":true}},"required":["enabled"]},"then":{"required":["targets"]},"additionalProperties":false});
        assert!(validate_schema(&schema, &json!({"enabled":false})).is_ok());
        assert!(validate_schema(&schema, &json!({"enabled":true})).is_err());
        assert!(
            validate_schema(
                &schema,
                &json!({"enabled":true,"targets":[{"name":"example"}]})
            )
            .is_ok()
        );
        assert!(
            validate_schema(&schema, &json!({"enabled":true,"targets":[{"name":""}]})).is_err()
        );
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct Contract {
    pub extension: Extension,
    pub instance: Instance,
    pub config_update: ConfigUpdate,
    pub event: Event,
    pub binding: Binding,
    pub invocation: Invocation,
    pub decision: Decision,
    pub receipt: Receipt,
    pub interaction: Interaction,
}
pub fn schema() -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(Contract)).expect("extension contract schema");
    schema["x-wakeit-events"] = json!(EVENT_TYPES);
    schema["x-wakeit-hooks"] = json!(
        HOOK_STAGES
            .iter()
            .map(|(stage, fields)| json!({"stage":stage,"mutable_fields":fields}))
            .collect::<Vec<_>>()
    );
    schema
}
