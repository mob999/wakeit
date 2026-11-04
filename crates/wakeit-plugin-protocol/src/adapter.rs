//! Public Agent capability contract. It describes native work; only Runner owns Run state.
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A declared operation must have an implementation registered during initialization.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AgentOperation {
    Detect,
    Execute,
    Submit,
    Poll,
    Cancel,
    Reconcile,
    Models,
    SessionList,
    SessionInspect,
    SessionResume,
    SessionRename,
    SessionInteractive,
}
impl AgentOperation {
    pub fn is_read_only(self) -> bool {
        matches!(
            self,
            Self::Detect
                | Self::Models
                | Self::Poll
                | Self::Reconcile
                | Self::SessionList
                | Self::SessionInspect
                | Self::SessionInteractive
        )
    }
    pub fn is_execution(self) -> bool {
        matches!(self, Self::Execute | Self::Submit | Self::SessionResume)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentDescriptor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, crate::agent::ParameterSpec>,
    pub mode: crate::agent::AgentMode,
    pub operations: BTreeSet<AgentOperation>,
}
impl AgentDescriptor {
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "Agent name is required");
        if let Some(logo) = &self.logo {
            let local = logo.starts_with('/') && !logo.starts_with("//") && !logo.contains('\\');
            let remote = url::Url::parse(logo).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            });
            ensure!(
                local || remote,
                "Agent logo must be a Gateway-root path or HTTPS URL"
            );
        }
        for (id, spec) in &self.parameters {
            crate::validate_id(id)?;
            ensure!(spec.kind == "string", "unsupported Agent parameter type");
            if let Some(value) = spec.default.as_ref().filter(|value| !value.is_empty()) {
                ensure!(
                    value.len() <= 4096
                        && (spec.choices.is_empty() || spec.choices.contains(value)),
                    "invalid Agent parameter default"
                );
            }
        }
        ensure!(
            self.operations.contains(&AgentOperation::Detect),
            "Agent must declare detection"
        );
        match self.mode {
            crate::agent::AgentMode::Sync => ensure!(
                self.operations.contains(&AgentOperation::Execute),
                "synchronous Agent must declare execute"
            ),
            crate::agent::AgentMode::Async => ensure!(
                self.operations.contains(&AgentOperation::Submit)
                    && self.operations.contains(&AgentOperation::Poll),
                "asynchronous Agent must declare submit and poll"
            ),
        }
        let sessions = [
            AgentOperation::SessionList,
            AgentOperation::SessionResume,
            AgentOperation::SessionRename,
            AgentOperation::SessionInteractive,
        ];
        ensure!(
            !sessions.iter().any(|op| self.operations.contains(op))
                || self.operations.contains(&AgentOperation::SessionInspect),
            "Session operations require inspection"
        );
        Ok(())
    }
}

/// Snapshot of authorized inputs, not an internal TaskConfig, Assignment or database row.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentContext {
    pub cwd: String,
    pub deadline: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRequest {
    pub id: Uuid,
    pub agent: String,
    pub operation: AgentOperation,
    pub context: AgentContext,
    /// Operation-specific input (query/cursor/limit or a requested native title).
    #[serde(default)]
    pub input: Value,
}
impl AgentRequest {
    pub fn validate(&self, descriptor: &AgentDescriptor) -> Result<()> {
        descriptor.validate()?;
        ensure!(
            descriptor.operations.contains(&self.operation),
            "Agent operation is not supported"
        );
        ensure!(
            !self.context.cwd.is_empty(),
            "Agent working directory is required"
        );
        ensure!(self.context.deadline > Utc::now(), "Agent request expired");
        if self.operation.is_execution() {
            ensure!(
                self.context.run_id.is_some()
                    && self
                        .context
                        .task_id
                        .as_ref()
                        .is_some_and(|id| !id.is_empty()),
                "Agent execution identity is required"
            );
            ensure!(
                self.context.prompt.is_some(),
                "Agent execution input is required"
            );
        } else {
            ensure!(
                self.context.prompt.is_none(),
                "Non-execution operations cannot receive task instructions"
            );
        }
        if matches!(
            self.operation,
            AgentOperation::SessionInspect
                | AgentOperation::SessionResume
                | AgentOperation::SessionRename
                | AgentOperation::SessionInteractive
        ) {
            crate::sessions::validate_id(
                self.context
                    .session_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Session ID is required"))?,
            )?;
        }
        if matches!(
            self.operation,
            AgentOperation::Poll | AgentOperation::Cancel | AgentOperation::Reconcile
        ) && descriptor.mode == crate::agent::AgentMode::Async
        {
            ensure!(
                self.context
                    .job_id
                    .as_ref()
                    .is_some_and(|id| !id.is_empty()),
                "Native job identity is required"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Detection {
    pub available: bool,
    pub version: Option<String>,
    pub reason: Option<String>,
}

/// References keep native credentials on the device and out of RPC plans.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "source", content = "value", rename_all = "snake_case")]
pub enum EnvironmentValue {
    Literal(String),
    Inherited(String),
    Credential(String),
}
/// Runner validates this plan against the selected Session/cwd, then owns the terminal.
/// No shell script, task instructions, native secrets or arbitrary Gateway argv are accepted.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InteractivePlan {
    pub cwd: String,
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, EnvironmentValue>,
}
impl InteractivePlan {
    pub fn validate(&self, cwd: &str) -> Result<()> {
        ensure!(
            self.cwd == cwd,
            "Interactive continuation changes the working directory"
        );
        ensure!(
            !self.argv.is_empty() && !self.argv[0].is_empty(),
            "Native executable is required"
        );
        ensure!(
            self.argv.len() <= 512
                && self
                    .argv
                    .iter()
                    .all(|arg| arg.len() <= 32768 && !arg.chars().any(char::is_control)),
            "Invalid native continuation arguments"
        );
        for (name, value) in &self.env {
            ensure!(
                !name.is_empty()
                    && name.len() <= 256
                    && !name.starts_with(|c: char| c.is_ascii_digit())
                    && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                "Invalid native environment name"
            );
            ensure!(
                ![
                    "WAKEIT_TOKEN",
                    "WAKEIT_TOKEN_FILE",
                    "WAKEIT_PASSWORD",
                    "WAKEIT_ENROLL_TOKEN"
                ]
                .contains(&name.to_ascii_uppercase().as_str()),
                "Wakeit credentials cannot be injected into native execution"
            );
            match value {
                EnvironmentValue::Literal(value) => ensure!(
                    !value.contains('\0') && value.len() <= 32768,
                    "Invalid native environment value"
                ),
                EnvironmentValue::Inherited(source) => {
                    ensure!(
                        ![
                            "WAKEIT_TOKEN",
                            "WAKEIT_TOKEN_FILE",
                            "WAKEIT_PASSWORD",
                            "WAKEIT_ENROLL_TOKEN"
                        ]
                        .contains(&source.to_ascii_uppercase().as_str()),
                        "Wakeit credentials cannot be inherited"
                    );
                    ensure!(
                        !source.is_empty()
                            && source
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                        "Invalid inherited environment name"
                    );
                }
                EnvironmentValue::Credential(slot) => crate::validate_id(slot)?,
            }
        }
        Ok(())
    }
}

/// Native completion is separate from the authoritative Run lifecycle.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionOutcome {
    /// Explicit acknowledgement that no native work was submitted or started.
    NotStarted {
        message: String,
    },
    Completed {
        exit_code: Option<i32>,
        result: Value,
        confirmed: bool,
    },
    Submitted {
        job_id: String,
    },
    Running {
        job_id: String,
    },
    CancelRequested {
        job_id: Option<String>,
    },
    Cancelled {
        confirmed: bool,
    },
    Uncertain {
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum AgentReply {
    Detection(Detection),
    Execution(ExecutionOutcome),
    Models(crate::protocol::ModelCatalog),
    Sessions(crate::session_catalog::Catalog),
    Session(crate::sessions::SessionInfo),
    Interactive(InteractivePlan),
    Renamed,
}

/// Acknowledged, per-request public output. The host rejects wrong identities,
/// out-of-order records, unsupported versions and writes from read-only queries.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentRecord {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub seq: u64,
    pub record: crate::trace::StreamRecord,
}

/// Observed native lifecycle, distinct from the SDK worker and authoritative Run state.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeFact {
    Started { pid: u32 },
    InputSubmitted,
    NativeFinished { exit_code: Option<i32> },
    StartFailed { message: String },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentFact {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub seq: u64,
    pub fact: NativeFact,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor() -> AgentDescriptor {
        AgentDescriptor {
            name: "Synthetic".into(),
            logo: None,
            parameters: BTreeMap::new(),
            mode: crate::agent::AgentMode::Sync,
            operations: [AgentOperation::Detect, AgentOperation::Execute].into(),
        }
    }
    fn request(operation: AgentOperation) -> AgentRequest {
        AgentRequest {
            id: Uuid::new_v4(),
            agent: "synthetic".into(),
            operation,
            context: AgentContext {
                cwd: "/synthetic".into(),
                deadline: Utc::now() + chrono::Duration::seconds(60),
                run_id: Some(Uuid::new_v4()),
                task_id: Some("task".into()),
                parameters: BTreeMap::new(),
                prompt: Some("synthetic instructions".into()),
                session_id: None,
                session_title: None,
                job_id: None,
            },
            input: Value::Null,
        }
    }
    #[test]
    fn query_context_never_contains_task_instructions() {
        let mut query = request(AgentOperation::Detect);
        assert!(query.validate(&descriptor()).is_err());
        query.context.prompt = None;
        assert!(query.validate(&descriptor()).is_ok());
        let mut execute = request(AgentOperation::Execute);
        assert!(execute.validate(&descriptor()).is_ok());
        execute.context.run_id = None;
        assert!(execute.validate(&descriptor()).is_err());
    }
    #[test]
    fn interactive_environment_does_not_inherit_wakeit_authentication_aliases() {
        let mut plan = InteractivePlan {
            cwd: "/synthetic".into(),
            argv: vec!["native-agent".into()],
            env: BTreeMap::new(),
        };
        plan.env.insert(
            "AGENT_AUTH".into(),
            EnvironmentValue::Inherited("wakeit_token".into()),
        );
        assert!(plan.validate("/synthetic").is_err());
        plan.env.clear();
        plan.env.insert(
            "WaKeIt_ToKeN".into(),
            EnvironmentValue::Literal("anything".into()),
        );
        assert!(plan.validate("/synthetic").is_err());
        plan.env.clear();
        plan.env.insert(
            "AGENT_AUTH".into(),
            EnvironmentValue::Credential("token".into()),
        );
        assert!(plan.validate("/synthetic").is_ok());
    }
    #[test]
    fn descriptor_metadata_does_not_weaken_legacy_logo_or_parameter_validation() {
        let mut d = descriptor();
        for logo in [
            "javascript:alert(1)",
            "//example.test/icon.svg",
            "https://user:secret@example.test/icon.svg",
            "/\\example.test/icon.svg",
        ] {
            d.logo = Some(logo.into());
            assert!(d.validate().is_err());
        }
        d.logo = Some("/agent-logos/synthetic.svg".into());
        assert!(d.validate().is_ok());
        d.parameters.insert(
            "model".into(),
            crate::agent::ParameterSpec {
                kind: "string".into(),
                label: String::new(),
                default: Some("unsupported".into()),
                choices: vec!["allowed".into()],
            },
        );
        assert!(d.validate().is_err());
    }
    #[test]
    fn unsupported_operations_and_incomplete_session_capabilities_fail() {
        let mut d = descriptor();
        assert!(request(AgentOperation::SessionResume).validate(&d).is_err());
        d.operations.insert(AgentOperation::SessionResume);
        assert!(d.validate().is_err());
        d.operations.insert(AgentOperation::SessionInspect);
        assert!(d.validate().is_ok());
    }
}
