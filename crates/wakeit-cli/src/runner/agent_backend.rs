//! Runner-owned Agent backends. SDK processes are scoped to one authorized invocation.
use crate::{
    config::{AgentConfig, AgentMode, OutputFormat, OutputSpec, ParameterSpec, TaskConfig},
    process::{self, CommandStartedError, LogSink, ProcessGroup},
};
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
pub use wakeit_plugin_protocol::adapter::{
    AgentContext, AgentDescriptor, AgentFact, AgentOperation, AgentRecord, AgentReply,
    AgentRequest, EnvironmentValue, ExecutionOutcome, NativeFact,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkBackend {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_directory: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ConfigurationSnapshot>,
    pub plugin: String,
    pub id: String,
    pub descriptor: AgentDescriptor,
    pub source: String,
    pub version: String,
    pub revision: Option<String>,
    pub root: PathBuf,
    pub command: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}
/// Only non-secret configuration is retained in the Runner-local Run snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationSnapshot {
    pub revision: i64,
    pub value: Value,
    /// Instance-scoped comparisons; credential values are never retained here.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credential_fingerprints: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "backend", content = "config", rename_all = "snake_case")]
pub enum Backend {
    LegacyCommand(Box<AgentConfig>),
    SdkBackend(Box<SdkBackend>),
}
/// Metadata is derived from the selected backend, never a synthetic command definition.
#[derive(Clone, Debug)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub logo: Option<String>,
    pub mode: AgentMode,
    pub parameters: BTreeMap<String, ParameterSpec>,
    pub output: OutputSpec,
    pub poll_interval: String,
    pub backend: Backend,
}
impl Serialize for Agent {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.backend.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Agent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Snapshot {
            Backend(Backend),
            Legacy(Box<AgentConfig>),
        }
        match Snapshot::deserialize(deserializer)? {
            Snapshot::Legacy(config) => Ok((*config).into()),
            Snapshot::Backend(Backend::LegacyCommand(config)) => Ok((*config).into()),
            Snapshot::Backend(Backend::SdkBackend(config)) => {
                Self::sdk(*config).map_err(serde::de::Error::custom)
            }
        }
    }
}
impl From<AgentConfig> for Agent {
    fn from(config: AgentConfig) -> Self {
        Self {
            id: config.id.clone(),
            name: config.name.clone(),
            logo: config.logo.clone(),
            mode: config.mode.clone(),
            parameters: config.parameters.clone(),
            output: config.output.clone(),
            poll_interval: config.poll_interval.clone(),
            backend: Backend::LegacyCommand(Box::new(config)),
        }
    }
}
impl From<&AgentConfig> for Agent {
    fn from(config: &AgentConfig) -> Self {
        config.clone().into()
    }
}
impl From<&Agent> for Agent {
    fn from(agent: &Agent) -> Self {
        agent.clone()
    }
}
impl Agent {
    pub fn sdk(config: SdkBackend) -> Result<Self> {
        crate::config::validate_id(&config.id)?;
        config.descriptor.validate()?;
        ensure!(
            config.root.is_absolute() && config.command.is_absolute(),
            "SDK package and executable paths must be absolute"
        );
        ensure!(
            !config.plugin.is_empty() && !config.source.is_empty() && !config.version.is_empty(),
            "SDK backend identity is required"
        );
        crate::config::resolve_parameters(&config.descriptor.parameters, &BTreeMap::new())?;
        let d = &config.descriptor;
        ensure!(
            d.mode == AgentMode::Sync
                || !d.operations.iter().any(|op| matches!(
                    op,
                    AgentOperation::SessionList
                        | AgentOperation::SessionInspect
                        | AgentOperation::SessionResume
                        | AgentOperation::SessionRename
                        | AgentOperation::SessionInteractive
                )),
            "sessions require a synchronous Agent"
        );
        Ok(Self {
            id: config.id.clone(),
            name: d.name.clone(),
            logo: d.logo.clone(),
            mode: d.mode.clone(),
            parameters: d.parameters.clone(),
            output: OutputSpec {
                format: if d.mode == AgentMode::Sync {
                    OutputFormat::Events
                } else {
                    OutputFormat::Json
                },
                result_pointer: None,
            },
            poll_interval: "5s".into(),
            backend: Backend::SdkBackend(Box::new(config)),
        })
    }
    /// Only the owning Runner supplies its instance selection and local data namespace.
    pub fn bind_runner(&mut self, directory: &Path, instance: Option<Uuid>) -> Result<()> {
        if let Backend::SdkBackend(config) = &mut self.backend {
            config.runtime_directory = Some(std::env::current_dir()?.join(directory));
            config.instance_id = instance;
            config.configuration = None;
        }
        Ok(())
    }
    /// Preserve a recorded instance/revision when continuing an existing Run.
    pub fn bind_saved_runner(&mut self, directory: &Path) -> Result<()> {
        if let Backend::SdkBackend(config) = &mut self.backend {
            config.runtime_directory = Some(std::env::current_dir()?.join(directory));
        }
        Ok(())
    }
    pub async fn pin_configuration(&mut self) -> Result<()> {
        if let Backend::SdkBackend(config) = &mut self.backend {
            let host = AgentHost::load(config).await?;
            config.configuration = Some(host.configuration);
        }
        Ok(())
    }
    pub(crate) async fn interactive_environment(
        &self,
        values: &BTreeMap<String, EnvironmentValue>,
    ) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>)> {
        let Backend::SdkBackend(backend) = &self.backend else {
            bail!("SDK environment references require an SDK backend")
        };
        let host = AgentHost::load(backend).await?;
        let request = Uuid::new_v4();
        let mut env = BTreeMap::new();
        let mut inherited = BTreeMap::new();
        for (name, value) in values {
            ensure!(
                !reserved_environment(name),
                "Wakeit credentials cannot be injected into interactive execution"
            );
            match value {
                EnvironmentValue::Literal(value) => {
                    ensure!(
                        host.redactor.text(value) == *value,
                        "literal interactive environment contains a credential value; use a credential reference"
                    );
                    env.insert(name.clone(), value.clone());
                }
                EnvironmentValue::Inherited(source) => {
                    ensure!(
                        !reserved_environment(source),
                        "Wakeit credentials cannot be inherited by interactive execution"
                    );
                    inherited.insert(name.clone(), source.clone());
                }
                EnvironmentValue::Credential(slot) => {
                    let value = host
                        .call(
                            "credentials.get",
                            &json!({"slot":slot,"agent_request_id":request}),
                            request,
                        )
                        .await?;
                    let value = value
                        .as_str()
                        .context("interactive credential slot must contain a configured string")?;
                    ensure!(
                        !value.is_empty() && !value.contains('\0'),
                        "interactive credential slot is empty or invalid"
                    );
                    env.insert(name.clone(), value.to_owned());
                }
            }
        }
        Ok((env, inherited))
    }
    pub fn legacy(&self) -> Option<&AgentConfig> {
        match &self.backend {
            Backend::LegacyCommand(config) => Some(config),
            _ => None,
        }
    }
    pub fn digest(&self) -> String {
        match &self.backend {
            Backend::LegacyCommand(config) => config.digest(),
            Backend::SdkBackend(config) => {
                let mut definition = config.clone();
                definition.instance_id = None;
                definition.runtime_directory = None;
                definition.configuration = None;
                hex::encode(Sha256::digest(
                    serde_json::to_vec(&Backend::SdkBackend(definition))
                        .expect("backend serializes"),
                ))
            }
        }
    }
    pub fn supports(&self, operation: AgentOperation) -> bool {
        match &self.backend {
            Backend::SdkBackend(config) => config.descriptor.operations.contains(&operation),
            Backend::LegacyCommand(config) => match operation {
                AgentOperation::Detect | AgentOperation::Execute => {
                    config.mode == AgentMode::Sync || operation == AgentOperation::Detect
                }
                AgentOperation::Submit | AgentOperation::Poll => config.mode == AgentMode::Async,
                AgentOperation::Cancel => config.mode == AgentMode::Sync || config.cancel.is_some(),
                AgentOperation::Models => config.models.is_some(),
                AgentOperation::SessionInspect
                | AgentOperation::SessionResume
                | AgentOperation::SessionInteractive => config.session.is_some(),
                AgentOperation::SessionList => {
                    config.session.as_ref().is_some_and(|s| s.list.is_some())
                }
                AgentOperation::SessionRename => {
                    config.session.as_ref().is_some_and(|s| s.rename.is_some())
                }
                AgentOperation::Reconcile => false,
            },
        }
    }
    pub fn session_supported(&self) -> bool {
        self.supports(AgentOperation::SessionInspect)
            && self.supports(AgentOperation::SessionResume)
            && self.supports(AgentOperation::SessionInteractive)
    }
    pub fn normalize(&mut self) -> Result<()> {
        if let Backend::LegacyCommand(config) = &mut self.backend {
            crate::launch::normalize_commands(config)?;
            config.validate()?;
        }
        Ok(())
    }
    pub fn request(
        &self,
        operation: AgentOperation,
        task: &TaskConfig,
        run: Option<Uuid>,
        timeout: Duration,
    ) -> AgentRequest {
        AgentRequest {
            id: Uuid::new_v4(),
            agent: self.id.clone(),
            operation,
            context: AgentContext {
                cwd: task.cwd.clone(),
                deadline: Utc::now()
                    + chrono::Duration::from_std(timeout).expect("bounded timeout"),
                run_id: run,
                task_id: run.map(|_| task.id.clone()),
                parameters: task.parameters.clone(),
                prompt: operation.is_execution().then(|| task.prompt.clone()),
                session_id: task.session.as_ref().map(|s| s.id.clone()),
                session_title: None,
                job_id: None,
            },
            input: Value::Null,
        }
    }
    pub async fn invoke(
        &self,
        mut request: AgentRequest,
        cancel: CancellationToken,
        sink: Option<LogSink>,
    ) -> Result<Invocation> {
        let Backend::SdkBackend(config) = &self.backend else {
            bail!("SDK invocation requires an SDK backend")
        };
        request.context.cwd = Path::new(&request.context.cwd)
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        request.context.parameters =
            crate::config::resolve_parameters(&self.parameters, &request.context.parameters)?;
        request.validate(&config.descriptor)?;
        ensure!(
            sink.as_ref()
                .is_none_or(|sink| Some(sink.id) == request.context.run_id),
            "Agent sink belongs to another Run"
        );
        invoke(config, request, cancel, sink).await
    }
    pub(super) async fn reconcile(
        &self,
        run: &super::journal::LocalRun,
        cancel: CancellationToken,
        sink: Option<LogSink>,
    ) -> Result<ExecutionOutcome> {
        ensure!(
            self.supports(AgentOperation::Reconcile),
            "saved Agent backend does not support reconciliation"
        );
        let mut request = self.request(
            AgentOperation::Reconcile,
            &run.assignment.task,
            Some(run.assignment.run_id),
            Duration::from_secs(10),
        );
        request.context.job_id = run.job.clone();
        request.context.session_id = run
            .assignment
            .task
            .session
            .as_ref()
            .map(|binding| binding.id.clone())
            .or_else(|| run.session.as_ref().map(|session| session.id.clone()));
        request.context.session_title = run.session_title.clone();
        // The RPC has its own bounded observation deadline. The original execution budget never changes.
        let timeout = crate::config::duration(
            run.assignment
                .task
                .execution
                .timeout
                .as_deref()
                .unwrap_or("1h"),
        )?;
        request.input = json!({"started_at":run.started,"execution_deadline":run.started+chrono::Duration::from_std(timeout)?});
        let observed = self.invoke(request, cancel, sink).await?;
        ensure!(
            observed.confirmed && observed.stopped.is_none(),
            "reconciliation process cleanup could not be confirmed"
        );
        let AgentReply::Execution(outcome) = observed.reply else {
            bail!("invalid Agent reconciliation reply")
        };
        match &outcome {
            ExecutionOutcome::Running { job_id } | ExecutionOutcome::Submitted { job_id } => {
                ensure!(
                    self.mode == AgentMode::Async && !job_id.is_empty() && job_id.len() <= 4096,
                    "invalid reconciled job identity"
                );
                ensure!(
                    run.job.as_ref() == Some(job_id),
                    "reconciliation returned another job identity"
                );
            }
            ExecutionOutcome::CancelRequested {
                job_id: Some(job_id),
            } => ensure!(
                run.job.as_ref() == Some(job_id),
                "reconciliation returned another job identity"
            ),
            ExecutionOutcome::NotStarted { .. } => ensure!(
                run.job.is_none(),
                "reconciliation contradicts the recorded submitted job"
            ),
            _ => {}
        }
        Ok(outcome)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn job_command(
        &self,
        operation: AgentOperation,
        task: &TaskConfig,
        run: Uuid,
        job: Option<&str>,
        timeout: Duration,
        cancel: CancellationToken,
        sink: Option<LogSink>,
    ) -> Result<process::Output> {
        if let Some(config) = self.legacy() {
            let spec = match operation {
                AgentOperation::Submit => &config.submit,
                AgentOperation::Poll => config.status.as_ref().context("status required")?,
                AgentOperation::Cancel => config.cancel.as_ref().context("cancel unsupported")?,
                _ => bail!("invalid job operation"),
            };
            return process::execute(spec, task, run, job, timeout, cancel, sink).await;
        }
        let mut request = self.request(operation, task, Some(run), timeout);
        request.context.job_id = job.map(str::to_owned);
        let invocation = self.invoke(request, cancel, sink).await?;
        ensure!(
            operation == AgentOperation::Submit
                || (invocation.confirmed && invocation.stopped.is_none()),
            "Agent job request termination could not be confirmed"
        );
        let AgentReply::Execution(outcome) = invocation.reply else {
            bail!("unexpected Agent job reply")
        };
        if let ExecutionOutcome::NotStarted { message } = outcome {
            if invocation.confirmed {
                return Err(NativeNotStarted(message).into());
            }
            return Err(CommandStartedError(anyhow::anyhow!(
                "Agent start failure cleanup could not be confirmed"
            ))
            .into());
        }
        let value = match (operation, outcome) {
            (AgentOperation::Submit, ExecutionOutcome::Submitted { job_id }) => {
                ensure!(
                    !job_id.is_empty() && job_id.len() <= 4096,
                    "invalid native job identity"
                );
                json!({"job_id":job_id})
            }
            (AgentOperation::Poll, ExecutionOutcome::Running { job_id }) => {
                ensure!(Some(job_id.as_str()) == job, "native job identity changed");
                json!({"state":"running"})
            }
            (
                AgentOperation::Poll,
                ExecutionOutcome::Completed {
                    exit_code,
                    result,
                    confirmed: true,
                },
            ) => {
                json!({"state":if exit_code == Some(0) {"succeeded"} else {"failed"},"result":if exit_code == Some(0) {result} else {Value::Null}})
            }
            (AgentOperation::Poll, ExecutionOutcome::Cancelled { confirmed: true }) => {
                json!({"state":"cancelled"})
            }
            (AgentOperation::Cancel, ExecutionOutcome::CancelRequested { job_id }) => {
                ensure!(
                    job_id.as_deref().is_none_or(|id| Some(id) == job),
                    "native job identity changed"
                );
                json!({})
            }
            (AgentOperation::Cancel, ExecutionOutcome::Cancelled { confirmed: true }) => json!({}),
            _ => bail!("Agent job state could not be confirmed"),
        };
        let mut output = process::Output::job(value);
        output.confirmed = invocation.confirmed;
        Ok(output)
    }
    pub async fn execute(
        &self,
        task: &TaskConfig,
        run: Uuid,
        timeout: Duration,
        cancel: CancellationToken,
        sink: Option<LogSink>,
        title: Option<&str>,
    ) -> Result<process::Output> {
        if let Some(config) = self.legacy() {
            let command = if task.session.is_some() {
                &config
                    .session
                    .as_ref()
                    .context("adapter cannot resume sessions")?
                    .resume
            } else {
                &config.submit
            };
            return process::execute_output_named_format(
                command,
                task,
                run,
                None,
                timeout,
                cancel,
                sink,
                config.output.format.clone(),
                title,
            )
            .await;
        }
        let mut request = self.request(
            if task.session.is_some() {
                AgentOperation::SessionResume
            } else {
                AgentOperation::Execute
            },
            task,
            Some(run),
            timeout,
        );
        request.context.session_title = title.map(str::to_owned);
        let invocation = self.invoke(request, cancel, sink).await?;
        match invocation.reply {
            AgentReply::Execution(ExecutionOutcome::Completed {
                exit_code,
                result,
                confirmed,
            }) => {
                let succeeded = exit_code == Some(0)
                    && confirmed
                    && invocation.confirmed
                    && invocation.stopped.is_none();
                let result = if !succeeded {
                    Value::Null
                } else if result.is_null() {
                    invocation.record_result.unwrap_or(Value::Null)
                } else {
                    result
                };
                Ok(process::Output::agent(
                    exit_code,
                    result,
                    invocation.stopped,
                    confirmed && invocation.confirmed,
                ))
            }
            AgentReply::Execution(ExecutionOutcome::NotStarted { message }) => {
                if invocation.confirmed {
                    Err(NativeNotStarted(message).into())
                } else {
                    Err(CommandStartedError(anyhow::anyhow!(
                        "Agent start failure cleanup could not be confirmed"
                    ))
                    .into())
                }
            }
            AgentReply::Execution(ExecutionOutcome::Cancelled { confirmed }) => {
                if invocation.stopped.is_none() {
                    return Err(CommandStartedError(anyhow::anyhow!(
                        "Agent reported cancellation without a host stop request"
                    ))
                    .into());
                }
                Ok(process::Output::agent(
                    None,
                    Value::Null,
                    invocation.stopped,
                    confirmed && invocation.confirmed,
                ))
            }
            AgentReply::Execution(ExecutionOutcome::Uncertain { reason }) => {
                Err(CommandStartedError(anyhow::anyhow!(reason)).into())
            }
            _ => {
                Err(CommandStartedError(anyhow::anyhow!("unexpected Agent execution reply")).into())
            }
        }
    }
}

fn reserved_environment(name: &str) -> bool {
    [
        "WAKEIT_TOKEN",
        "WAKEIT_TOKEN_FILE",
        "WAKEIT_PASSWORD",
        "WAKEIT_ENROLL_TOKEN",
    ]
    .contains(&name.to_ascii_uppercase().as_str())
}
/// Ephemeral values from this instance's declared slots. Never Debug/Serialize this cache.
#[derive(Clone, Default)]
struct AgentRedactor(std::sync::Arc<std::sync::RwLock<Vec<String>>>);
impl AgentRedactor {
    fn remember(&self, value: &Value) {
        fn collect(value: &Value, patterns: &mut Vec<String>) {
            match value {
                Value::String(value) if !value.is_empty() => {
                    let mut escaped = value.clone();
                    for _ in 0..3 {
                        patterns.push(escaped.clone());
                        let encoded = serde_json::to_string(&escaped).expect("string serializes");
                        escaped = encoded[1..encoded.len() - 1].to_owned();
                    }
                    if let Ok(nested) = serde_json::from_str::<Value>(value) {
                        collect(&nested, patterns);
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        collect(value, patterns);
                    }
                }
                Value::Object(values) => {
                    for value in values.values() {
                        collect(value, patterns);
                    }
                }
                _ => {}
            }
        }
        let mut patterns = self.0.write().expect("credential redaction lock");
        collect(value, &mut patterns);
        patterns.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
        patterns.dedup();
    }
    fn text(&self, input: &str) -> String {
        let mut text = input.to_owned();
        for pattern in self.0.read().expect("credential redaction lock").iter() {
            text = text.replace(pattern, "[redacted]");
        }
        text
    }
    fn value(&self, value: &mut Value) {
        match value {
            Value::String(text) => *text = self.text(text),
            Value::Array(values) => {
                for value in values {
                    self.value(value);
                }
            }
            Value::Object(values) => {
                for (key, mut value) in std::mem::take(values) {
                    self.value(&mut value);
                    values.insert(self.text(&key), value);
                }
            }
            _ => {}
        }
    }
    fn dto<T: Serialize + serde::de::DeserializeOwned>(&self, value: &mut T) -> Result<()> {
        let mut encoded = serde_json::to_value(&*value)?;
        self.value(&mut encoded);
        *value = serde_json::from_value(encoded)
            .context("Agent metadata contained a credential value")?;
        Ok(())
    }
    fn record(&self, record: &mut crate::trace::StreamRecord) {
        use crate::trace::RecordData;
        match &mut record.record {
            RecordData::Log(log) => {
                log.text = self.text(&log.text);
                log.stream = self.text(&log.stream);
            }
            RecordData::Trace(node) => {
                node.id = self.text(&node.id);
                node.parent_id = node.parent_id.as_deref().map(|id| self.text(id));
                node.title = self.text(&node.title);
                self.value(&mut node.input);
                self.value(&mut node.output);
            }
            RecordData::Session(info) => {
                info.id = self.text(&info.id);
                info.cwd = self.text(&info.cwd);
                info.title = info.title.as_deref().map(|title| self.text(title));
            }
            RecordData::Result(value) => self.value(value),
        }
    }
    fn reply(&self, reply: &mut AgentReply) -> Result<()> {
        match reply {
            AgentReply::Execution(outcome) => match outcome {
                ExecutionOutcome::NotStarted { message }
                | ExecutionOutcome::Uncertain { reason: message } => *message = self.text(message),
                ExecutionOutcome::Completed { result, .. } => self.value(result),
                ExecutionOutcome::Submitted { job_id } | ExecutionOutcome::Running { job_id } => {
                    ensure!(
                        self.text(job_id) == *job_id,
                        "Agent job identity contains a credential value"
                    )
                }
                ExecutionOutcome::CancelRequested {
                    job_id: Some(job_id),
                } => ensure!(
                    self.text(job_id) == *job_id,
                    "Agent job identity contains a credential value"
                ),
                _ => {}
            },
            AgentReply::Detection(info) => {
                info.version = info.version.as_deref().map(|value| self.text(value));
                info.reason = info.reason.as_deref().map(|value| self.text(value));
            }
            AgentReply::Models(catalog) => self.dto(catalog)?,
            AgentReply::Sessions(catalog) => self.dto(catalog)?,
            AgentReply::Session(info) => self.dto(info)?,
            AgentReply::Interactive(plan) => {
                ensure!(
                    self.text(&plan.cwd) == plan.cwd
                        && plan.argv.iter().all(|arg| self.text(arg) == *arg)
                        && plan.env.iter().all(|(name, value)| self.text(name) == *name
                            && match value {
                                EnvironmentValue::Literal(value) => self.text(value) == *value,
                                _ => true,
                            }),
                    "interactive plan contains a credential value"
                );
            }
            AgentReply::Renamed => {}
        }
        Ok(())
    }
    /// Retain only a suffix that could still complete a secret across pipe reads.
    fn stream_prefix(&self, pending: &mut String, eof: bool) -> String {
        let patterns = self.0.read().expect("credential redaction lock");
        let mut position = 0;
        let mut output = String::new();
        while position < pending.len() {
            let rest = &pending[position..];
            if let Some(pattern) = patterns
                .iter()
                .find(|pattern| rest.starts_with(pattern.as_str()))
            {
                output.push_str("[redacted]");
                position += pattern.len();
            } else if !eof && patterns.iter().any(|pattern| pattern.starts_with(rest)) {
                break;
            } else {
                let ch = rest.chars().next().expect("remaining character");
                output.push(ch);
                position += ch.len_utf8();
            }
        }
        pending.drain(..position);
        output
    }
}
async fn read_agent_diagnostics(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    sink: Option<LogSink>,
    exited: CancellationToken,
    redactor: AgentRedactor,
) -> Result<bool> {
    let mut buffer = [0u8; 8192];
    let mut utf8 = Vec::new();
    let mut pending = String::new();
    let eof = loop {
        let read = reader.read(&mut buffer);
        tokio::pin!(read);
        let next = tokio::select! {value=&mut read=>Some(value),_=exited.cancelled()=>tokio::time::timeout(Duration::from_secs(5),&mut read).await.ok()};
        let Some(n) = next else { break false };
        let n = n?;
        if n == 0 {
            break true;
        }
        utf8.extend_from_slice(&buffer[..n]);
        let mut ready = 0;
        while ready < utf8.len() {
            match std::str::from_utf8(&utf8[ready..]) {
                Ok(_) => {
                    ready = utf8.len();
                    break;
                }
                Err(error) => {
                    ready += error.valid_up_to();
                    if let Some(length) = error.error_len() {
                        ready += length;
                    } else {
                        break;
                    }
                }
            }
        }
        if ready > 0 {
            pending.push_str(&String::from_utf8_lossy(&utf8[..ready]));
            utf8.drain(..ready);
            let text = redactor.stream_prefix(&mut pending, false);
            if let Some(sink) = &sink
                && !text.is_empty()
            {
                sink.journal.log(sink.id, "stderr", &text).await?;
            }
        }
    };
    pending.push_str(&String::from_utf8_lossy(&utf8));
    let text = redactor.stream_prefix(&mut pending, true);
    if let Some(sink) = &sink
        && !text.is_empty()
    {
        sink.journal.log(sink.id, "stderr", &text).await?;
    }
    Ok(eof)
}

struct AgentHost {
    redactor: AgentRedactor,
    backend: SdkBackend,
    declaration: wakeit_core::extensions::Extension,
    configuration: ConfigurationSnapshot,
    pool: Option<SqlitePool>,
}
impl AgentHost {
    async fn load(backend: &SdkBackend) -> Result<Self> {
        let manifest = crate::plugins::manifest::read(&backend.root)?;
        ensure!(
            manifest.version == backend.version,
            "saved SDK package version changed"
        );
        let declaration = manifest
            .wakeit
            .plugins
            .get(&backend.plugin)
            .context("SDK plugin resource is unavailable")?
            .clone();
        ensure!(
            declaration
                .agents
                .get(&backend.id)
                .is_some_and(|descriptor| serde_json::to_value(descriptor).ok()
                    == serde_json::to_value(&backend.descriptor).ok()),
            "SDK instance Agent declaration changed"
        );
        if backend.instance_id.is_none() {
            let empty_schema = declaration.config == json!({"type":"object","properties":{}})
                || declaration.config == json!({})
                || declaration.config == Value::Bool(true);
            ensure!(
                empty_schema
                    && declaration.credentials.is_empty()
                    && declaration.permissions.is_empty(),
                "Agent requires an explicitly selected plugin instance in agent_instances"
            );
            return Ok(Self {
                backend: backend.clone(),
                declaration,
                configuration: ConfigurationSnapshot {
                    revision: 0,
                    value: json!({}),
                    credential_fingerprints: BTreeMap::new(),
                },
                pool: None,
                redactor: AgentRedactor::default(),
            });
        }
        let directory = backend
            .runtime_directory
            .as_ref()
            .context("Agent instance requires its owning Runner data directory")?;
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.join("journal.sqlite"))
            .read_only(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("Agent instance store is unavailable")?;
        let mut host = Self {
            backend: backend.clone(),
            declaration,
            configuration: ConfigurationSnapshot {
                revision: 0,
                value: Value::Null,
                credential_fingerprints: BTreeMap::new(),
            },
            pool: Some(pool),
            redactor: AgentRedactor::default(),
        };
        let row = host.current(false).await?;
        host.configuration = host.saved_configuration(&row).await?;
        if !host.declaration.credentials.is_empty() {
            let row = host.current(true).await?;
            let secrets: Value = serde_json::from_str(row.get("secrets"))?;
            for slot in host.declaration.credentials.keys() {
                if let Some(value) = secrets.get(slot) {
                    host.redactor.remember(value);
                    if backend.configuration.is_none() {
                        host.configuration
                            .credential_fingerprints
                            .insert(slot.clone(), host.credential_fingerprint(slot, value)?);
                    }
                }
            }
        }
        let mut safe_configuration = host.configuration.value.clone();
        host.redactor.value(&mut safe_configuration);
        ensure!(
            safe_configuration == host.configuration.value,
            "Agent configuration contains a credential value; use the declared credential slot instead"
        );
        wakeit_core::extensions::validate_schema(
            &host.declaration.config,
            &host.configuration.value,
        )
        .map_err(|error| anyhow::anyhow!(host.redactor.text(&error.to_string())))?;
        Ok(host)
    }
    fn same_code(
        &self,
        root: &str,
        version: &str,
        declaration: &wakeit_core::extensions::Extension,
    ) -> Result<bool> {
        Ok(version == self.backend.version
            && Path::new(root).canonicalize().ok().as_ref()
                == Some(&self.backend.root.canonicalize()?)
            && serde_json::to_value(declaration)? == serde_json::to_value(&self.declaration)?)
    }
    async fn saved_configuration(
        &self,
        row: &sqlx::sqlite::SqliteRow,
    ) -> Result<ConfigurationSnapshot> {
        let current_declaration: wakeit_core::extensions::Extension =
            serde_json::from_str(row.get("declaration"))?;
        let same = self.same_code(row.get("root"), row.get("version"), &current_declaration)?;
        let revision = row.get::<i64, _>("revision");
        if same
            && self
                .backend
                .configuration
                .as_ref()
                .is_none_or(|config| config.revision == revision)
        {
            return Ok(self
                .backend
                .configuration
                .clone()
                .unwrap_or(ConfigurationSnapshot {
                    revision,
                    value: serde_json::from_str(row.get("config"))?,
                    credential_fingerprints: BTreeMap::new(),
                }));
        }
        let pool = self
            .pool
            .as_ref()
            .context("Agent version store is unavailable")?;
        let instance = self
            .backend
            .instance_id
            .context("Agent instance is not selected")?
            .to_string();
        let raw: Option<String> = if let Some(config) = &self.backend.configuration {
            sqlx::query_scalar(
                "SELECT snapshot FROM extension_versions WHERE instance=? AND revision=?",
            )
            .bind(&instance)
            .bind(config.revision)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_scalar("SELECT snapshot FROM extension_versions WHERE instance=? AND json_extract(snapshot,'$.version')=? AND json_extract(snapshot,'$.root')=? ORDER BY revision DESC LIMIT 1").bind(&instance).bind(&self.backend.version).bind(self.backend.root.to_string_lossy().as_ref()).fetch_optional(pool).await?
        };
        let snapshot: Value =
            serde_json::from_str(&raw.context("saved Agent instance revision is unavailable")?)?;
        let declaration: wakeit_core::extensions::Extension =
            serde_json::from_value(snapshot["declaration"].clone())?;
        ensure!(
            self.same_code(
                snapshot["root"]
                    .as_str()
                    .context("saved package root is missing")?,
                snapshot["version"]
                    .as_str()
                    .context("saved package version is missing")?,
                &declaration
            )?,
            "saved Agent instance revision belongs to another code definition"
        );
        let revision = snapshot["revision"]
            .as_i64()
            .context("saved instance revision is missing")?;
        if let Some(config) = &self.backend.configuration {
            ensure!(
                config.revision == revision && config.value == snapshot["config"],
                "saved Agent configuration does not match its recorded instance revision"
            );
            Ok(config.clone())
        } else {
            Ok(ConfigurationSnapshot {
                revision,
                value: snapshot["config"].clone(),
                credential_fingerprints: BTreeMap::new(),
            })
        }
    }
    fn credential_fingerprint(&self, slot: &str, value: &Value) -> Result<String> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
            self.backend.instance_id,
            slot,
            value,
        ))?)))
    }
    async fn current(&self, credentials: bool) -> Result<sqlx::sqlite::SqliteRow> {
        let pool = self
            .pool
            .as_ref()
            .context("Agent has no selected plugin instance")?;
        let query = if credentials {
            "SELECT id,source,extension,root,version,revision,enabled,removed,grants,declaration,config,secrets FROM extension_instances WHERE id=?"
        } else {
            "SELECT id,source,extension,root,version,revision,enabled,removed,grants,declaration,config FROM extension_instances WHERE id=?"
        };
        let row = sqlx::query(query)
            .bind(
                self.backend
                    .instance_id
                    .context("Agent instance is not selected")?
                    .to_string(),
            )
            .fetch_optional(pool)
            .await?
            .context("selected Agent instance is unavailable on this Runner")?;
        ensure!(
            row.get::<String, _>("source") == self.backend.source
                && row.get::<String, _>("extension") == self.backend.plugin,
            "selected Agent instance belongs to another plugin source or resource"
        );
        ensure!(
            row.get::<i64, _>("enabled") != 0 && row.get::<i64, _>("removed") == 0,
            "selected Agent instance is disabled or removed"
        );
        let grants: Vec<String> = serde_json::from_str(row.get("grants"))?;
        ensure!(
            self.declaration
                .permissions
                .iter()
                .all(|permission| grants.contains(permission)),
            "Agent instance permission was not granted or has been revoked"
        );
        Ok(row)
    }
    async fn call(&self, method: &str, params: &Value, request: Uuid) -> Result<Value> {
        ensure!(
            ["config.get", "credentials.get"].contains(&method),
            "Agent host API is not supported by this Runner bridge"
        );
        if let Some(id) = params.get("agent_request_id") {
            ensure!(*id == json!(request), "Agent host request identity changed");
        }
        if let Some(id) = params.get("instance_id") {
            ensure!(
                Some(id.as_str().context("invalid instance identity")?)
                    == self
                        .backend
                        .instance_id
                        .as_ref()
                        .map(|id| id.to_string())
                        .as_deref(),
                "Agent host request belongs to another instance"
            );
        }
        match method {
            "config.get" => {
                if self.pool.is_some() {
                    self.current(false).await?;
                }
                Ok(json!({"value":self.configuration.value,"revision":self.configuration.revision}))
            }
            "credentials.get" => {
                let slot = params["slot"]
                    .as_str()
                    .context("credential slot is required")?;
                let declaration = self
                    .declaration
                    .credentials
                    .get(slot)
                    .context("credential slot is not declared by this plugin")?;
                let row = self.current(true).await?;
                let current: wakeit_core::extensions::Extension =
                    serde_json::from_str(row.get("declaration"))?;
                ensure!(
                    current.credentials.contains_key(slot),
                    "credential slot was removed from the current plugin authorization"
                );
                let secrets: Value = serde_json::from_str(row.get("secrets"))?;
                let value = secrets.get(slot).cloned().unwrap_or(Value::Null);
                if row.get::<i64, _>("revision") != self.configuration.revision {
                    ensure!(
                        self.configuration.credential_fingerprints.get(slot)
                            == Some(&self.credential_fingerprint(slot, &value)?),
                        "Agent credential changed since the saved configuration revision"
                    );
                }
                ensure!(
                    !declaration.required || value.as_str().is_some_and(|value| !value.is_empty()),
                    "required Agent credential is not configured"
                );
                self.redactor.remember(&value);
                Ok(value)
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct NativeNotStarted(pub String);
impl std::fmt::Display for NativeNotStarted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for NativeNotStarted {}
#[derive(Debug)]
struct AgentRemoteError {
    code: Option<String>,
    message: String,
}
impl AgentRemoteError {
    fn confirms_host_stop(&self, stopped: Option<&str>) -> bool {
        match self.code.as_deref() {
            Some("cancelled") => stopped.is_some(),
            Some("timeout") => stopped == Some("timed_out"),
            _ => false,
        }
    }
}
impl std::fmt::Display for AgentRemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for AgentRemoteError {}
#[derive(Clone, Default)]
enum NativeEvidence {
    #[default]
    Unseen,
    Running {
        pid: u32,
        input_submitted: bool,
    },
    Finished {
        exit_code: Option<i32>,
    },
    StartFailed(String),
}
impl NativeEvidence {
    fn apply(&mut self, fact: &NativeFact, stopped: bool) -> Result<()> {
        match (&*self, fact) {
            (Self::Unseen, NativeFact::Started { pid }) if !stopped => {
                *self = Self::Running {
                    pid: *pid,
                    input_submitted: false,
                }
            }
            (
                Self::Running {
                    input_submitted: false,
                    ..
                },
                NativeFact::InputSubmitted,
            ) if !stopped => {
                let Self::Running { pid, .. } = self else {
                    unreachable!()
                };
                *self = Self::Running {
                    pid: *pid,
                    input_submitted: true,
                }
            }
            (Self::Running { .. }, NativeFact::NativeFinished { exit_code }) => {
                *self = Self::Finished {
                    exit_code: *exit_code,
                }
            }
            (Self::Unseen, NativeFact::StartFailed { message }) => {
                ensure!(
                    !message.is_empty() && message.len() <= 8192,
                    "invalid native start failure evidence"
                );
                *self = Self::StartFailed(message.clone());
            }
            _ => bail!(
                "native lifecycle fact is duplicate, out of order, or starts new work during cancellation"
            ),
        }
        Ok(())
    }
    fn check_reply(&self, reply: &AgentReply, work_observed: bool) -> Result<()> {
        ensure!(
            !matches!(self, Self::Running { .. }),
            "Agent replied before native completion was observed"
        );
        if let AgentReply::Execution(ExecutionOutcome::NotStarted { .. }) = reply {
            ensure!(
                !work_observed && matches!(self, Self::Unseen | Self::StartFailed(_)),
                "NotStarted contradicts observed execution"
            );
        }
        if matches!(self, Self::StartFailed(_)) {
            ensure!(
                matches!(
                    reply,
                    AgentReply::Execution(ExecutionOutcome::NotStarted { .. })
                ),
                "Agent reply contradicts native start failure"
            );
        }
        if let Self::Finished { exit_code } = self
            && *exit_code != Some(0)
        {
            ensure!(
                !matches!(
                    reply,
                    AgentReply::Execution(ExecutionOutcome::Completed {
                        exit_code: Some(0),
                        ..
                    })
                ),
                "Agent success contradicts native exit status"
            );
        }
        Ok(())
    }
}

pub struct Invocation {
    pub reply: AgentReply,
    pub record_result: Option<Value>,
    pub stopped: Option<&'static str>,
    pub confirmed: bool,
}
#[derive(Debug)]
struct PeerDisconnected;
impl std::fmt::Display for PeerDisconnected {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Agent disconnected before completing its request")
    }
}
impl std::error::Error for PeerDisconnected {}
#[derive(Debug)]
struct StopDeadline;
impl std::fmt::Display for StopDeadline {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Agent did not complete within the cancellation grace")
    }
}
impl std::error::Error for StopDeadline {}
#[derive(Default)]
struct ControlState {
    submitted: bool,
    sealed: bool,
    stopped: Option<&'static str>,
}
#[derive(Default)]
struct InvocationControl {
    state: std::sync::Mutex<ControlState>,
    submitted: CancellationToken,
    stopping: CancellationToken,
    escalated: CancellationToken,
    done: CancellationToken,
}
impl InvocationControl {
    fn reason(&self) -> Option<&'static str> {
        self.state.lock().expect("Agent control lock").stopped
    }
    fn stop(&self, reason: &'static str) -> Option<bool> {
        let mut state = self.state.lock().expect("Agent control lock");
        if state.sealed {
            return None;
        }
        if state.stopped.is_none() {
            state.stopped = Some(reason);
            self.stopping.cancel();
        }
        Some(state.submitted)
    }
    async fn admit(
        &self,
        writes: &mpsc::Sender<Value>,
        value: Value,
        cancel: &CancellationToken,
        limit: tokio::time::Instant,
    ) -> Result<()> {
        let permit = writes
            .reserve()
            .await
            .context("Agent input disconnected before invocation")?;
        let mut state = self.state.lock().expect("Agent control lock");
        ensure!(
            !state.sealed
                && state.stopped.is_none()
                && !cancel.is_cancelled()
                && tokio::time::Instant::now() < limit,
            "Agent stopped before invocation"
        );
        state.submitted = true;
        permit.send(value);
        self.submitted.cancel();
        Ok(())
    }
    fn seal(
        &self,
        cancel: &CancellationToken,
        deadline: tokio::time::Instant,
    ) -> Option<&'static str> {
        let mut state = self.state.lock().expect("Agent control lock");
        if state.stopped.is_none() {
            if cancel.is_cancelled() {
                state.stopped = Some("cancelled");
            } else if tokio::time::Instant::now() >= deadline {
                state.stopped = Some("timed_out");
            }
            if state.stopped.is_some() {
                self.stopping.cancel();
            }
        }
        state.sealed = true;
        state.stopped
    }
}
async fn monitor_control(
    control: std::sync::Arc<InvocationControl>,
    group: std::sync::Arc<ProcessGroup>,
    priority: mpsc::Sender<Value>,
    cancel: CancellationToken,
    request: Uuid,
    initialization: tokio::time::Instant,
    deadline: tokio::time::Instant,
) {
    let reason = tokio::select! {
        biased;
        _=control.done.cancelled()=>return,
        _=control.stopping.cancelled()=>control.reason().expect("stop reason"),
        _=cancel.cancelled()=>"cancelled",
        _=control.submitted.cancelled()=>tokio::select! {
            biased;
            _=control.done.cancelled()=>return,
            _=control.stopping.cancelled()=>control.reason().expect("stop reason"),
            _=cancel.cancelled()=>"cancelled",
            _=tokio::time::sleep_until(deadline)=>"timed_out",
        },
        _=tokio::time::sleep_until(initialization)=>"timed_out",
    };
    let Some(submitted) = control.stop(reason) else {
        return;
    };
    if submitted {
        // One reserved control slot; a stuck data write never postpones the signal timers.
        let _ =
            priority.try_send(json!({"jsonrpc":"2.0","method":"cancel","params":{"id":request}}));
        tokio::select! {_=control.done.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(2))=>{}}
    }
    group.terminate();
    control.escalated.cancel();
    tokio::select! {_=control.done.cancelled()=>return,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
    group.kill();
}
const FRAME_LIMIT: usize = 8 * 1024 * 1024;

async fn write_frame(writes: &mpsc::Sender<Value>, value: Value) -> Result<()> {
    writes.send(value).await.context("Agent input disconnected")
}
async fn frame(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    bytes: &mut Vec<u8>,
) -> Result<Value> {
    let n = reader
        .take((FRAME_LIMIT + 1 - bytes.len()) as u64)
        .read_until(b'\n', bytes)
        .await?;
    if n == 0 {
        return Err(PeerDisconnected.into());
    }
    ensure!(bytes.len() <= FRAME_LIMIT, "Agent RPC frame exceeds 8 MiB");
    let value: Value = serde_json::from_slice(bytes).context("invalid Agent RPC")?;
    bytes.clear();
    ensure!(value["jsonrpc"] == "2.0", "invalid Agent RPC version");
    Ok(value)
}
async fn invoke(
    config: &SdkBackend,
    request: AgentRequest,
    cancel: CancellationToken,
    sink: Option<LogSink>,
) -> Result<Invocation> {
    ensure!(
        !cancel.is_cancelled(),
        "Agent request cancelled before start"
    );
    let host = AgentHost::load(config).await?;
    let cwd = Path::new(&request.context.cwd).canonicalize()?;
    config
        .root
        .canonicalize()
        .context("saved SDK package is unavailable")?;
    let executable = config
        .command
        .canonicalize()
        .context("saved SDK entry is unavailable")?;
    let mut command = Command::new(wakeit_core::native_command::external_path(&executable));
    command
        .args(&config.args)
        .current_dir(&cwd)
        .env("PWD", &cwd)
        .env("WAKEIT_PLUGIN_ROOT", &config.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(run) = request.context.run_id {
        command.env("WAKEIT_RUN_ID", run.to_string());
    }
    if let Some(task) = &request.context.task_id {
        command.env("WAKEIT_TASK_ID", task);
    }
    for key in [
        "WAKEIT_TOKEN",
        "WAKEIT_TOKEN_FILE",
        "WAKEIT_PASSWORD",
        "WAKEIT_ENROLL_TOKEN",
    ] {
        command.env_remove(key);
    }
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x00000200);
    let mut child = command.spawn().context("start Agent SDK process")?;
    let worker_pid = child
        .id()
        .context("Agent worker process identity is unavailable")?;
    let group = match ProcessGroup::new(&child) {
        Ok(group) => std::sync::Arc::new(group),
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    let mut input = child.stdin.take().context("Agent stdin")?;
    let mut output = BufReader::new(child.stdout.take().context("Agent stdout")?);
    let stderr = child.stderr.take().context("Agent stderr")?;
    let exited = CancellationToken::new();
    let stderr_task = tokio::spawn(read_agent_diagnostics(
        stderr,
        sink.clone(),
        exited.clone(),
        host.redactor.clone(),
    ));
    let (writes, mut queue) = mpsc::channel::<Value>(8);
    let (priority, mut controls) = mpsc::channel::<Value>(1);
    let writer = tokio::spawn(async move {
        let mut invoked = false;
        let mut pending_cancel = None;
        loop {
            // Never overtake agent.invoke: an SDK cannot cancel a request it has not received.
            let value = if invoked && pending_cancel.is_some() {
                pending_cancel.take().expect("pending cancellation")
            } else {
                tokio::select! {
                    biased;
                    Some(value)=controls.recv(), if pending_cancel.is_none()=>{
                        if !invoked {pending_cancel=Some(value);continue;} value
                    }
                    value=queue.recv()=>match value {Some(value)=>value,None=>break},
                }
            };
            let invoking = value["method"] == "agent.invoke";
            input.write_all(&serde_json::to_vec(&value)?).await?;
            input.write_all(b"\n").await?;
            input.flush().await?;
            invoked |= invoking;
        }
        input.shutdown().await?;
        Ok::<_, anyhow::Error>(())
    });
    let init = Uuid::new_v4();
    let mut submitted = false;
    let mut stopped = None;
    let deadline = tokio::time::Instant::now()
        + (request.context.deadline - Utc::now())
            .to_std()
            .unwrap_or_default();
    let initialization = deadline.min(tokio::time::Instant::now() + Duration::from_secs(20));
    let control = std::sync::Arc::new(InvocationControl::default());
    let _control_guard = control.done.clone().drop_guard();
    let monitor = tokio::spawn(monitor_control(
        control.clone(),
        group.clone(),
        priority,
        cancel.clone(),
        request.id,
        initialization,
        deadline,
    ));
    let mut bytes = Vec::new();
    let mut seq = 0u64;
    let mut record_result = None;
    // One bounded wire value, held only in memory, preserves precise equality before redaction.
    let mut observed_result: Option<Value> = None;
    let mut native = NativeEvidence::default();
    let mut work_observed = false;
    let mut trace_ids = std::collections::HashSet::new();
    let mut diagnosed = std::collections::BTreeSet::new();
    let result: Result<AgentReply> = async {
        write_frame(&writes, json!({"jsonrpc":"2.0","id":init,"method":"initialize","params":{"protocol_version":1,"plugin":config.plugin}})).await?;
        loop {
            stopped=control.reason();
            if stopped.is_some() && !submitted {bail!("Agent stopped before invocation");}
            let value = tokio::select! {
                biased;
                _=control.escalated.cancelled()=>return Err(StopDeadline.into()),
                _=cancel.cancelled(), if stopped.is_none()=>{control.stop("cancelled");continue;},
                _=control.stopping.cancelled(), if stopped.is_none()=>{continue;},
                value=frame(&mut output,&mut bytes)=>value?,
            };
            stopped=control.reason();
            if let Some(method) = value["method"].as_str() {
                if !["agent.record", "agent.fact"].contains(&method) {
                    ensure!(!value["id"].is_null(), "unsupported Agent notification");
                    let host_result = if control.reason().is_some() { Err(anyhow::anyhow!("Agent request is stopping; only bounded execution observations may finish")) } else { host.call(method, &value["params"], request.id).await };
                    let host_result=if control.reason().is_some() {Err(anyhow::anyhow!("Agent request is stopping; host access is closed"))} else {host_result};
                    let response = match host_result {
                        Ok(result) => json!({"jsonrpc":"2.0","id":value["id"],"result":result}),
                        Err(error) => json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32000,"message":host.redactor.text(&error.to_string()),"data":{"code":if ["config.get","credentials.get"].contains(&method) {"agent_host_denied"} else {"unsupported"},"retryable":false}}}),
                    };
                    write_frame(&writes, response).await?;
                    continue;
                }
                ensure!(!value["id"].is_null(), "Agent output requires acknowledgement");
                ensure!(submitted && request.operation.is_execution(), "Agent output requires an executing request");
                if method == "agent.fact" {
                    let mut fact: AgentFact = serde_json::from_value(value["params"].clone())?;
                    ensure!(fact.request_id == request.id && Some(fact.run_id) == request.context.run_id && fact.seq == seq + 1, "Agent fact identity or sequence changed");
                    if let NativeFact::StartFailed { message } = &mut fact.fact { *message = host.redactor.text(message); }
                    if let NativeFact::Started { pid } = &fact.fact { group.validate_native_pid(*pid, worker_pid)?; }
                    if matches!(fact.fact, NativeFact::StartFailed { .. }) { ensure!(!work_observed, "native start failure contradicts observed work"); }
                    let mut next = native.clone();
                    next.apply(&fact.fact, control.reason().is_some())?;
                    if let Some(sink) = &sink { sink.journal.agent_fact(&fact, control.reason()).await?; }
                    native = next;
                    seq = fact.seq;
                    write_frame(&writes, json!({"jsonrpc":"2.0","id":value["id"],"result":{"seq":seq}})).await?;
                    continue;
                }
                ensure!(submitted && request.operation.is_execution(), "Agent records require an executing request");
                let mut record: AgentRecord = serde_json::from_value(value["params"].clone())?;
                ensure!(record.request_id == request.id && Some(record.run_id) == request.context.run_id && record.seq == seq + 1, "Agent record identity or sequence changed");
                match &record.record.record {
                    crate::trace::RecordData::Log(_) => {},
                    _ => {
                        ensure!(!matches!(native, NativeEvidence::StartFailed(_)), "execution output contradicts native start failure");
                        if let crate::trace::RecordData::Trace(node) = &record.record.record {
                            ensure!(control.reason().is_none() || node.status != crate::trace::TraceState::Running || trace_ids.contains(&node.id), "new trace work during cancellation");
                            trace_ids.insert(node.id.clone());
                        }
                        work_observed = true;
                    }
                }
                let raw_result=match &record.record.record {crate::trace::RecordData::Result(value)=>Some(value.clone()),_=>None};
                host.redactor.record(&mut record.record);
                let encoded = serde_json::to_vec(&record.record)?;
                let parsed = if encoded.len() > crate::trace::RECORD_LIMIT { Err(crate::trace::RecordTooLarge.into()) } else { crate::trace::StreamRecord::parse(&encoded) };
                let accepted=parsed.is_ok();
                // Sequential persistence and ACK remain the data barrier, independent of supervision.
                process::stream_record(parsed, &sink, &mut record_result, &mut diagnosed).await?;
                if accepted && raw_result.is_some() {observed_result=raw_result;}
                seq = record.seq;
                write_frame(&writes, json!({"jsonrpc":"2.0","id":value["id"],"result":{"seq":seq}})).await?;
                continue;
            }
            let expected = if submitted { request.id } else { init };
            ensure!(value["id"] == json!(expected), "unexpected Agent RPC response");
            if value.get("error").is_some() {
                let error=&value["error"];
                return Err(AgentRemoteError {
                    code:error["data"]["code"].as_str().or(error["code"].as_str()).map(|code|host.redactor.text(code)),
                    message:host.redactor.text(error["message"].as_str().unwrap_or("Agent request failed")),
                }.into());
            }
            if !submitted {
                ensure!(value["result"]["protocol_version"] == 1, "unsupported Agent SDK protocol");
                let registered: BTreeMap<String, AgentDescriptor> = serde_json::from_value(value["result"]["agents"].clone()).context("Agent SDK registration missing")?;
                let descriptor = registered.get(&config.id).context("Agent is not registered by SDK entry")?;
                ensure!(serde_json::to_value(descriptor)? == serde_json::to_value(&config.descriptor)?, "Agent SDK registration differs from its manifest");
                // Admission and stop share a gate; reserving the bounded queue is not native work.
                control.admit(&writes,json!({"jsonrpc":"2.0","id":request.id,"method":"agent.invoke","params":request}),&cancel,initialization).await?;
                submitted=true;
            } else {
                let mut reply: AgentReply = serde_json::from_value(value["result"].clone()).context("invalid typed Agent reply")?;
                if let AgentReply::Execution(ExecutionOutcome::Completed {result:inline,..})=&reply
                    && let Some(streamed)=&observed_result {
                    ensure!(inline.is_null() || streamed.is_null() || inline==streamed,"conflicting final Agent result");
                }
                host.redactor.reply(&mut reply)?;
                native.check_reply(&reply, work_observed)?;
                return Ok(reply);
            }
        }
    }.await;
    stopped = control.seal(&cancel, deadline);
    drop(writes);
    // A stuck SDK writer must not keep stdin open or hold the Run beyond cleanup.
    writer.abort();
    let _ = writer.await;
    if result.is_err() || stopped.is_some() {
        group.terminate();
    }
    let status = match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
        Ok(status) => status,
        Err(_) => {
            group.terminate();
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(status) => status,
                Err(_) => {
                    group.kill();
                    let _ = child.kill().await;
                    child.wait().await
                }
            }
        }
    };
    let tree_confirmed = group.finish_confirmed().await;
    control.done.cancel();
    let _ = monitor.await;
    exited.cancel();
    let diagnostics = stderr_task.await;
    let clean = tree_confirmed
        && diagnostics
            .as_ref()
            .is_ok_and(|r| r.as_ref().is_ok_and(|eof| *eof));
    let host_finished = if let NativeEvidence::Running { pid, .. } = &native {
        if clean && stopped.is_some() {
            if let Some(sink) = &sink {
                sink.journal.extension_event("execution.native_finished", json!({"run_id":request.context.run_id,"request_id":request.id,"after_seq":seq,"pid":pid,"exit_code":null,"stopped":stopped,"source":"runner_cleanup"})).await.map_err(CommandStartedError)?;
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    let result = match result {
        // SDK cancellation is an error response, not a native fact or Run state.
        // Only the host's sealed stop request plus confirmed teardown authorizes it.
        Err(error)
            if clean
                && submitted
                && (host_finished || matches!(native, NativeEvidence::Finished { .. }))
                && matches!(
                    request.operation,
                    AgentOperation::Execute | AgentOperation::SessionResume
                )
                && error
                    .downcast_ref::<AgentRemoteError>()
                    .is_some_and(|error| error.confirms_host_stop(stopped)) =>
        {
            Ok(Invocation {
                reply: AgentReply::Execution(ExecutionOutcome::Cancelled { confirmed: true }),
                record_result: None,
                stopped,
                confirmed: true,
            })
        }
        Err(error)
            if clean
                && stopped.is_some()
                && (host_finished || matches!(native, NativeEvidence::Finished { .. }))
                && error.is::<StopDeadline>() =>
        {
            Ok(Invocation {
                reply: AgentReply::Execution(ExecutionOutcome::Cancelled { confirmed: true }),
                record_result: None,
                stopped,
                confirmed: true,
            })
        }
        Ok(reply) if status.as_ref().is_ok_and(|s| s.success()) || stopped.is_some() => {
            Ok(Invocation {
                reply,
                record_result,
                stopped,
                confirmed: clean,
            })
        }
        Ok(AgentReply::Execution(ExecutionOutcome::NotStarted { message })) if clean => {
            Err(NativeNotStarted(message).into())
        }
        Ok(_) => Err(anyhow::anyhow!("Agent SDK process exited unsuccessfully")),
        Err(error)
            if clean
                && error.is::<AgentRemoteError>()
                && matches!(native, NativeEvidence::StartFailed(_)) =>
        {
            let NativeEvidence::StartFailed(message) = native else {
                unreachable!()
            };
            Err(NativeNotStarted(message).into())
        }
        Err(error) => Err(error),
    };
    result.map_err(|error| {
        // Drop the original chain as well: serde errors can quote credential-bearing values.
        let message = host.redactor.text(&format!("{error:#}"));
        if error.is::<NativeNotStarted>() {
            NativeNotStarted(message).into()
        } else if submitted && request.operation.is_execution() {
            CommandStartedError(anyhow::anyhow!(message)).into()
        } else {
            anyhow::anyhow!(message)
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{protocol::Assignment, runner::journal::Journal};
    use tempfile::TempDir;

    fn directory() -> TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/agent-bridge");
        std::fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }
    fn legacy() -> AgentConfig {
        serde_json::from_value(json!({"version":1,"id":"synthetic","name":"Synthetic","mode":"sync","submit":{"command":"unused"},"output":{"format":"events"}})).unwrap()
    }
    pub(crate) fn fixture(root: &Path, mode: &str) -> Agent {
        let script = root.join("peer.cjs");
        std::fs::write(&script, PEER).unwrap();
        let mut descriptor = AgentDescriptor {
            name: "Synthetic".into(),
            logo: None,
            parameters: BTreeMap::new(),
            mode: AgentMode::Sync,
            operations: [
                AgentOperation::Detect,
                AgentOperation::Execute,
                AgentOperation::Models,
                AgentOperation::SessionInspect,
                AgentOperation::SessionResume,
                AgentOperation::SessionInteractive,
            ]
            .into(),
        };
        if mode.starts_with("sync_reconcile") {
            descriptor.operations.insert(AgentOperation::Reconcile);
        }
        if mode.starts_with("async") {
            descriptor.mode = AgentMode::Async;
            descriptor.operations = [
                AgentOperation::Detect,
                AgentOperation::Submit,
                AgentOperation::Poll,
                AgentOperation::Cancel,
                AgentOperation::Reconcile,
            ]
            .into();
        }
        std::fs::write(root.join("wakeit-plugin.json"), serde_json::to_vec(&json!({"manifest_version":1,"name":"@synthetic/agents","version":"1.0.0","wakeit":{"protocol":1,"plugins":{"synthetic-entry":{"name":"Synthetic entry","command":"node","agents":{"synthetic":descriptor.clone()}}}}})).unwrap()).unwrap();
        Agent::sdk(SdkBackend {
            instance_id: None,
            runtime_directory: None,
            configuration: None,
            plugin: "synthetic-entry".into(),
            id: "synthetic".into(),
            descriptor: descriptor.clone(),
            source: "synthetic:test".into(),
            version: "1.0.0".into(),
            revision: None,
            root: root.canonicalize().unwrap(),
            command: which::which("node").expect("Node is required for the synthetic IPC peer"),
            args: vec![
                script.to_string_lossy().into(),
                serde_json::to_string(&descriptor).unwrap(),
                mode.into(),
                root.to_string_lossy().into(),
            ],
        })
        .unwrap()
    }
    fn typescript_fixture(root: &Path) -> Agent {
        let mut agent = fixture(root, "unused");
        let sdk = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sdk/typescript/src/index.ts")
            .canonicalize()
            .unwrap();
        let script = root.join("actual-sdk.mts");
        let code = r#"
import { Plugin, Agent, PluginError } from __SDK__;
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
const descriptor=JSON.parse(process.argv[2]),root=process.argv[3];
const agent=new Agent(descriptor);
for(const operation of descriptor.operations) agent.on(operation,async(request,ctx)=>{
 if(operation==='detect') return {kind:'detection',value:{available:true,version:'synthetic',reason:null}};
 if(operation==='models') return {kind:'models',value:{version:1,models:[]}};
 if(operation!=='execute') throw new Error('unsupported synthetic operation');
 if(request.context.prompt==='self-cancel') throw new PluginError('cancelled','Invocation cancelled');
 if(request.context.prompt==='message-only') throw new Error('Invocation cancelled');
 if(request.context.prompt==='typed-self-cancel') return {kind:'execution',value:{outcome:'cancelled',confirmed:true}};
 if(request.context.prompt==='independent') {await ctx.result('independent synthetic report');return {kind:'execution',value:{outcome:'completed',exit_code:0,result:null,confirmed:true}};}
 writeFileSync(join(root,'ts-ready-'+request.context.run_id),'ready');
 try {await new Promise((resolve,reject)=>{
  const stop=()=>{writeFileSync(join(root,'ts-aborted-'+request.context.run_id),'aborted');reject(ctx.signal.reason);};
  if(ctx.signal.aborted) stop();else ctx.signal.addEventListener('abort',stop,{once:true});
 });} catch(error) {
  if(request.context.prompt==='throw-on-abort') throw error;
  return {kind:'execution',value:{outcome:'cancelled',confirmed:true}};
 }
 throw new Error('unreachable synthetic execution');
});
await new Plugin().agent('synthetic',agent).serve();
"#;
        std::fs::write(
            &script,
            code.replace(
                "__SDK__",
                &serde_json::to_string(reqwest::Url::from_file_path(&sdk).unwrap().as_str())
                    .unwrap(),
            ),
        )
        .unwrap();
        let Backend::SdkBackend(backend) = &mut agent.backend else {
            unreachable!()
        };
        backend.args = vec![
            "--experimental-strip-types".into(),
            "--no-warnings".into(),
            wakeit_core::native_command::external_path(&script),
            serde_json::to_string(&backend.descriptor).unwrap(),
            root.to_string_lossy().into_owned(),
        ];
        agent
    }
    fn task(root: &Path) -> TaskConfig {
        serde_json::from_value(json!({"version":1,"id":"synthetic-task","name":"Synthetic task","runner":"synthetic-runner","agent":"synthetic","cwd":root,"prompt":"synthetic instructions","parameters":{}})).unwrap()
    }
    async fn await_file(path: &Path) {
        tokio::time::timeout(Duration::from_secs(8), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("synthetic peer did not reach its gate");
    }
    async fn configured_fixture(root: &Path, mode: &str) -> (Agent, SqlitePool, Uuid) {
        let mut agent = fixture(root, mode);
        let manifest_path = root.join("wakeit-plugin.json");
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        let declaration = &mut manifest["wakeit"]["plugins"]["synthetic-entry"];
        declaration["config"] = json!({"type":"object","properties":{"endpoint":{"type":"string"}},"required":["endpoint"],"additionalProperties":false});
        declaration["credentials"] = json!({"token":{"label":"Synthetic token","required":true}});
        declaration["permissions"] = json!(["network:outbound"]);
        std::fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let directory = root.join("runtime");
        let journal = Journal::open(&directory, Uuid::new_v4()).await.unwrap();
        sqlx::query("CREATE TABLE extension_instances(id TEXT PRIMARY KEY,source TEXT NOT NULL,extension TEXT NOT NULL,root TEXT NOT NULL,version TEXT NOT NULL,revision INTEGER NOT NULL,enabled INTEGER NOT NULL,removed INTEGER NOT NULL,grants TEXT NOT NULL,declaration TEXT NOT NULL,config TEXT NOT NULL,secrets TEXT NOT NULL)").execute(&journal.pool).await.unwrap();
        sqlx::query("CREATE TABLE extension_versions(instance TEXT NOT NULL,revision INTEGER NOT NULL,snapshot TEXT NOT NULL,PRIMARY KEY(instance,revision))").execute(&journal.pool).await.unwrap();
        let id = Uuid::new_v4();
        let declaration =
            crate::plugins::manifest::read(root).unwrap().wakeit.plugins["synthetic-entry"].clone();
        sqlx::query("INSERT INTO extension_instances VALUES(?,?,?,?,?,1,1,0,?,?,?,?)")
            .bind(id.to_string())
            .bind("synthetic:test")
            .bind("synthetic-entry")
            .bind(root.canonicalize().unwrap().to_string_lossy().into_owned())
            .bind("1.0.0")
            .bind(json!(["network:outbound"]).to_string())
            .bind(serde_json::to_string(&declaration).unwrap())
            .bind(json!({"endpoint":"https://synthetic.invalid/one"}).to_string())
            .bind(json!({"token":"synthetic-credential"}).to_string())
            .execute(&journal.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO extension_versions SELECT id,revision,json_object('root',root,'version',version,'declaration',json(declaration),'revision',revision,'config',json(config)) FROM extension_instances WHERE id=?").bind(id.to_string()).execute(&journal.pool).await.unwrap();
        agent.bind_runner(&directory, Some(id)).unwrap();
        (agent, journal.pool, id)
    }
    #[tokio::test]
    async fn configured_peer_reads_only_its_selected_instance_and_never_snapshots_credentials() {
        let root = directory();
        let (mut agent, _pool, _instance) = configured_fixture(root.path(), "configuration").await;
        let digest = agent.digest();
        agent.pin_configuration().await.unwrap();
        assert_eq!(
            agent.digest(),
            digest,
            "Runner-local settings do not change the declaration digest"
        );
        let encoded = serde_json::to_string(&agent).unwrap();
        assert!(!encoded.contains("synthetic-credential"));
        assert!(encoded.contains("https://synthetic.invalid/one"));
        let output = agent
            .execute(
                &task(root.path()),
                Uuid::new_v4(),
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(output.confirmed);
        assert_eq!(
            output.result,
            Some(json!(true)),
            "the peer must receive its declared synthetic credential"
        );
        let configuration: Value =
            serde_json::from_slice(&std::fs::read(root.path().join("host-config")).unwrap())
                .unwrap();
        assert_eq!(
            configuration,
            json!({"revision":1,"value":{"endpoint":"https://synthetic.invalid/one"}})
        );
    }
    #[tokio::test]
    async fn configuration_is_pinned_and_credential_rotation_or_revocation_cannot_cross_it() {
        let root = directory();
        let (mut agent, pool, instance) = configured_fixture(root.path(), "configuration").await;
        agent.pin_configuration().await.unwrap();
        sqlx::query("UPDATE extension_instances SET revision=2,config=?,secrets=? WHERE id=?")
            .bind(json!({"endpoint":"https://synthetic.invalid/two"}).to_string())
            .bind(json!({"token":"rotated-synthetic-credential"}).to_string())
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
        let Backend::SdkBackend(backend) = &agent.backend else {
            unreachable!()
        };
        let host = AgentHost::load(backend).await.unwrap();
        let request = Uuid::new_v4();
        let config = host
            .call("config.get", &json!({"agent_request_id":request}), request)
            .await
            .unwrap();
        assert_eq!(
            config,
            json!({"revision":1,"value":{"endpoint":"https://synthetic.invalid/one"}})
        );
        assert!(
            host.call("credentials.get", &json!({"slot":"token"}), request)
                .await
                .is_err()
        );
        assert!(
            host.call("credentials.get", &json!({"slot":"undeclared"}), request)
                .await
                .is_err()
        );
        assert!(
            host.call(
                "config.get",
                &json!({"agent_request_id":Uuid::new_v4()}),
                request
            )
            .await
            .is_err()
        );
        assert!(
            host.call(
                "config.get",
                &json!({"instance_id":Uuid::new_v4()}),
                request
            )
            .await
            .is_err()
        );
        sqlx::query("UPDATE extension_instances SET grants='[]' WHERE id=?")
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert!(host.call("config.get", &json!({}), request).await.is_err());
    }
    #[tokio::test]
    async fn configured_agents_require_explicit_local_source_and_resource_binding() {
        let root = directory();
        let (agent, pool, instance) = configured_fixture(root.path(), "configuration").await;
        let Backend::SdkBackend(backend) = agent.backend else {
            unreachable!()
        };
        let mut unbound = backend.clone();
        unbound.instance_id = None;
        assert!(
            AgentHost::load(&unbound).await.is_err(),
            "even a single matching instance is not selected implicitly"
        );
        for (column, value) in [
            ("source", "synthetic:foreign"),
            ("extension", "foreign-entry"),
        ] {
            sqlx::query(&format!(
                "UPDATE extension_instances SET {column}=? WHERE id=?"
            ))
            .bind(value)
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
            assert!(AgentHost::load(&backend).await.is_err());
            sqlx::query("UPDATE extension_instances SET source='synthetic:test',extension='synthetic-entry' WHERE id=?").bind(instance.to_string()).execute(&pool).await.unwrap();
        }
        sqlx::query("UPDATE extension_instances SET enabled=0 WHERE id=?")
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert!(AgentHost::load(&backend).await.is_err());
        let mut foreign_runner = backend;
        foreign_runner.runtime_directory = Some(root.path().join("another-runner"));
        assert!(AgentHost::load(&foreign_runner).await.is_err());
    }
    #[tokio::test]
    async fn stateless_defaults_are_real_but_do_not_grant_credentials_or_mutating_apis() {
        let root = directory();
        let agent = fixture(root.path(), "query");
        let Backend::SdkBackend(backend) = agent.backend else {
            unreachable!()
        };
        let host = AgentHost::load(&backend).await.unwrap();
        let request = Uuid::new_v4();
        assert_eq!(
            host.call("config.get", &json!({}), request).await.unwrap(),
            json!({"value":{},"revision":0})
        );
        assert!(
            host.call("credentials.get", &json!({"slot":"token"}), request)
                .await
                .is_err()
        );
        assert!(host.call("config.save", &json!({}), request).await.is_err());
    }
    async fn upgrade_instance(root: &Path, pool: &SqlitePool, instance: Uuid) {
        let directory = root.join("version-two");
        std::fs::create_dir_all(&directory).unwrap();
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(root.join("wakeit-plugin.json")).unwrap())
                .unwrap();
        manifest["version"] = json!("2.0.0");
        manifest["wakeit"]["plugins"]["synthetic-entry"]["name"] = json!("Updated synthetic entry");
        std::fs::write(
            directory.join("wakeit-plugin.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let declaration = crate::plugins::manifest::read(&directory)
            .unwrap()
            .wakeit
            .plugins["synthetic-entry"]
            .clone();
        sqlx::query("UPDATE extension_instances SET root=?,version='2.0.0',revision=2,declaration=?,config=? WHERE id=?")
            .bind(directory.canonicalize().unwrap().to_string_lossy().into_owned()).bind(serde_json::to_string(&declaration).unwrap()).bind(json!({"endpoint":"https://synthetic.invalid/two"}).to_string()).bind(instance.to_string()).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO extension_versions SELECT id,revision,json_object('root',root,'version',version,'declaration',json(declaration),'revision',revision,'config',json(config)) FROM extension_instances WHERE id=?").bind(instance.to_string()).execute(pool).await.unwrap();
    }
    #[tokio::test]
    async fn historical_instance_code_and_config_survive_upgrade_but_current_authorization_still_applies()
     {
        for mode in ["async", "query"] {
            let root = directory();
            let (mut agent, pool, instance) = configured_fixture(root.path(), mode).await;
            agent.pin_configuration().await.unwrap();
            let digest = agent.digest();
            upgrade_instance(root.path(), &pool, instance).await;
            let restored: Agent =
                serde_json::from_str(&serde_json::to_string(&agent).unwrap()).unwrap();
            assert_eq!(restored.digest(), digest);
            let Backend::SdkBackend(backend) = &restored.backend else {
                unreachable!()
            };
            let host = AgentHost::load(backend).await.unwrap();
            assert_eq!(
                host.call("config.get", &json!({}), Uuid::new_v4())
                    .await
                    .unwrap(),
                json!({"revision":1,"value":{"endpoint":"https://synthetic.invalid/one"}})
            );
            assert!(
                host.call("credentials.get", &json!({"slot":"token"}), Uuid::new_v4())
                    .await
                    .unwrap()
                    == json!("synthetic-credential")
            );
            let mut task = task(root.path());
            let run = Uuid::new_v4();
            if mode == "async" {
                for operation in [AgentOperation::Poll, AgentOperation::Cancel] {
                    let output = restored
                        .job_command(
                            operation,
                            &task,
                            run,
                            Some("synthetic-job"),
                            Duration::from_secs(10),
                            CancellationToken::new(),
                            None,
                        )
                        .await
                        .unwrap();
                    assert!(output.confirmed);
                }
            } else {
                task.session = Some(crate::config::SessionBinding {
                    id: "synthetic-session".into(),
                });
                let info =
                    crate::runner::sessions::inspect(&restored, &task, run, "synthetic-session")
                        .await
                        .unwrap();
                assert!(info.persisted);
                let command = crate::runner::sessions::resolved_continuation_command(
                    &restored,
                    &task,
                    run,
                    "synthetic-session",
                )
                .await
                .unwrap();
                assert!(command.contains("--version"));
            }
            for statement in [
                "UPDATE extension_instances SET enabled=0 WHERE id=?",
                "UPDATE extension_instances SET removed=1 WHERE id=?",
                "UPDATE extension_instances SET grants='[]' WHERE id=?",
            ] {
                sqlx::query(statement)
                    .bind(instance.to_string())
                    .execute(&pool)
                    .await
                    .unwrap();
                assert!(AgentHost::load(backend).await.is_err());
                sqlx::query(
                    "UPDATE extension_instances SET enabled=1,removed=0,grants=? WHERE id=?",
                )
                .bind(json!(["network:outbound"]).to_string())
                .bind(instance.to_string())
                .execute(&pool)
                .await
                .unwrap();
            }
            sqlx::query("UPDATE extension_instances SET secrets=? WHERE id=?")
                .bind(json!({"token":"rotated-synthetic-token"}).to_string())
                .bind(instance.to_string())
                .execute(&pool)
                .await
                .unwrap();
            assert!(
                AgentHost::load(backend)
                    .await
                    .unwrap()
                    .call("credentials.get", &json!({"slot":"token"}), Uuid::new_v4())
                    .await
                    .is_err()
            );
        }
    }
    #[tokio::test]
    async fn private_interactive_environment_uses_instance_refs_without_export_or_template_expansion()
     {
        let root = directory();
        let (mut agent, pool, instance) =
            configured_fixture(root.path(), "interactive_environment").await;
        let secret = "synthetic-private-\"\n{{task.prompt}}";
        sqlx::query("UPDATE extension_instances SET secrets=? WHERE id=?")
            .bind(json!({"token":secret}).to_string())
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
        agent.pin_configuration().await.unwrap();
        let expected: BTreeMap<_, _> = [
            ("SYNTHETIC_TOKEN", secret.to_owned()),
            ("SYNTHETIC_LITERAL", "literal {{task.prompt}}".into()),
            ("SYNTHETIC_INHERITED", std::env::var("PATH").unwrap()),
        ]
        .into();
        let hashes: BTreeMap<_, _> = expected
            .iter()
            .map(|(name, value)| (*name, hex::encode(Sha256::digest(value.as_bytes()))))
            .collect();
        std::fs::write(
            root.path().join("environment-hashes"),
            serde_json::to_vec(&hashes).unwrap(),
        )
        .unwrap();
        let runner = Uuid::new_v4();
        let run = Uuid::new_v4();
        let mut task = task(root.path());
        task.session = Some(crate::config::SessionBinding {
            id: "synthetic-session".into(),
        });
        let directory = root.path().join("runtime");
        let journal = Journal::open(&directory, runner).await.unwrap();
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task: task.clone(),
                    deadline: Utc::now() + chrono::Duration::minutes(1),
                },
                &agent,
            )
            .await
            .unwrap();
        journal.phase(run, "done", None).await.unwrap();
        journal
            .save_session(
                run,
                crate::runner::sessions::RunSession {
                    id: "synthetic-session".into(),
                    runner_id: runner,
                    agent: agent.id.clone(),
                    cwd: task.cwd.clone(),
                    ready: true,
                    reason: None,
                    command: None,
                    title: None,
                },
            )
            .await
            .unwrap();
        let config = crate::runner::config::RunnerConfig {
            version: 1,
            id: runner,
            name: "synthetic".into(),
            gateway: "http://127.0.0.1:1".into(),
            nats_url: "nats://127.0.0.1:1".into(),
            token_file: root.path().join("unused.token"),
            credentials_file: root.path().join("unused.creds"),
            data_dir: directory,
            concurrency: 2,
            agents: vec![],
            agent_instances: [(agent.id.clone(), instance)].into(),
            bundled_agents: false,
            allowed_roots: vec![root.path().canonicalize().unwrap()],
        };
        let Err(exported) = crate::runner::sessions::resolved_continuation_command(
            &agent,
            &task,
            run,
            "synthetic-session",
        )
        .await
        else {
            panic!("private environments must not be exported");
        };
        assert!(!exported.to_string().contains(secret));
        let prepared = crate::runner::sessions::prepare_local(&config, &journal, run)
            .await
            .unwrap();
        assert!(prepared.literal_environment);
        let values = crate::runner::sessions::continuation_environment(
            &prepared.command,
            &prepared.task,
            run,
            prepared.literal_environment,
        )
        .unwrap();
        assert!(
            expected
                .iter()
                .all(|(name, value)| values.get(*name) == Some(value)),
            "literal SDK environment values must not be re-expanded"
        );
        drop(prepared);
        let path = root.path().join("runner.yaml");
        std::fs::write(&path, yaml_serde::to_string(&config).unwrap()).unwrap();
        assert_eq!(
            crate::runner::sessions::resume(run, Some(&path))
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("environment-ok")).unwrap(),
            "ok"
        );
        let snapshot: String = sqlx::query_scalar("SELECT agent FROM local_runs WHERE id=?")
            .bind(run.to_string())
            .fetch_one(&journal.pool)
            .await
            .unwrap();
        assert_no_secret(&snapshot, secret);
        assert!(
            journal
                .session(run)
                .await
                .unwrap()
                .unwrap()
                .command
                .is_none()
        );
        for source in [
            "WAKEIT_TOKEN",
            "WAKEIT_TOKEN_FILE",
            "WAKEIT_PASSWORD",
            "WAKEIT_ENROLL_TOKEN",
            "wakeit_token",
        ] {
            assert!(
                agent
                    .interactive_environment(
                        &[(
                            "SYNTHETIC_ENV".into(),
                            EnvironmentValue::Inherited(source.into())
                        )]
                        .into()
                    )
                    .await
                    .is_err()
            );
        }
        assert!(
            agent
                .interactive_environment(
                    &[(
                        "SYNTHETIC_ENV".into(),
                        EnvironmentValue::Credential("undeclared".into())
                    )]
                    .into()
                )
                .await
                .is_err()
        );
    }
    fn synthetic_secret() -> String {
        ["synthetic-secret", "\"", "\n", "尾"].concat()
    }
    fn assert_no_secret(text: &str, secret: &str) {
        let mut pattern = secret.to_owned();
        for _ in 0..3 {
            assert!(
                !text.contains(&pattern),
                "credential or its JSON-escaped form appeared in a persisted or returned output"
            );
            let encoded = serde_json::to_string(&pattern).unwrap();
            pattern = encoded[1..encoded.len() - 1].to_owned();
        }
    }
    #[test]
    fn credential_redaction_spans_pipe_boundaries_and_json_escaping_without_erasing_reports() {
        let secret = synthetic_secret();
        let redactor = AgentRedactor::default();
        redactor.remember(&json!(secret));
        let encoded = serde_json::to_string(&secret).unwrap();
        let encoded_twice = serde_json::to_string(&encoded).unwrap();
        let input = format!(
            "ordinary prefix {secret} encoded {encoded} nested {encoded_twice} ordinary suffix"
        );
        for split in input.char_indices().map(|(index, _)| index) {
            let mut pending = input[..split].to_owned();
            let mut output = redactor.stream_prefix(&mut pending, false);
            pending.push_str(&input[split..]);
            output.push_str(&redactor.stream_prefix(&mut pending, true));
            assert_no_secret(&output, &secret);
            assert!(output.starts_with("ordinary prefix ") && output.ends_with(" ordinary suffix"));
        }
        let mut report =
            json!({"ordinary":"ordinary synthetic report","echo":secret,"escaped":encoded});
        redactor.value(&mut report);
        assert_eq!(report["ordinary"], "ordinary synthetic report");
        assert_no_secret(&report.to_string(), &secret);
    }
    #[tokio::test]
    async fn selected_instance_credentials_are_redacted_from_errors_stderr_logs_facts_and_reports()
    {
        for mode in [
            "credential_leak_error",
            "credential_leak_fact",
            "credential_leak_not_started",
            "credential_leak_malformed",
            "credential_leak_conflict",
            "credential_leak_report",
        ] {
            let root = directory();
            let (mut agent, pool, instance) = configured_fixture(root.path(), mode).await;
            let secret = synthetic_secret();
            sqlx::query("UPDATE extension_instances SET secrets=? WHERE id=?")
                .bind(json!({"token":secret}).to_string())
                .bind(instance.to_string())
                .execute(&pool)
                .await
                .unwrap();
            agent.pin_configuration().await.unwrap();
            assert_no_secret(&serde_json::to_string(&agent).unwrap(), &secret);
            let runner = Uuid::new_v4();
            let run = Uuid::new_v4();
            let task = task(root.path());
            let directory = root.path().join("runtime");
            let journal = Journal::open(&directory, runner).await.unwrap();
            journal
                .insert(
                    &Assignment {
                        version: 1,
                        run_id: run,
                        runner_id: runner,
                        task: task.clone(),
                        deadline: Utc::now() + chrono::Duration::minutes(1),
                    },
                    &agent,
                )
                .await
                .unwrap();
            journal.phase(run, "started", None).await.unwrap();
            let result = agent
                .execute(
                    &task,
                    run,
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    Some(LogSink {
                        journal: journal.clone(),
                        id: run,
                        stdout: true,
                    }),
                    None,
                )
                .await;
            if mode == "credential_leak_report" {
                let output = result.unwrap();
                let value = output.result_value(&agent.output).unwrap();
                assert_eq!(value["ordinary"], "ordinary synthetic report");
                assert_no_secret(&value.to_string(), &secret);
            } else {
                let error = result.err().expect("synthetic peer must fail");
                let reason = format!("{error:#}");
                assert_no_secret(&reason, &secret);
                if ["credential_leak_fact", "credential_leak_not_started"].contains(&mode) {
                    assert!(error.is::<NativeNotStarted>());
                } else {
                    assert!(error.is::<CommandStartedError>());
                }
                journal
                    .emit(
                        run,
                        "state",
                        json!({"state":"failed","reason":reason}),
                        Some("done"),
                    )
                    .await
                    .unwrap();
            }
            let events: Vec<String> = sqlx::query_scalar("SELECT payload FROM pending_events")
                .fetch_all(&journal.pool)
                .await
                .unwrap();
            for event in events {
                assert_no_secret(&event, &secret);
            }
            let events: Vec<String> = sqlx::query_scalar("SELECT data FROM extension_events")
                .fetch_all(&journal.pool)
                .await
                .unwrap();
            for event in events {
                assert_no_secret(&event, &secret);
            }
            let log =
                std::fs::read_to_string(directory.join("logs").join(format!("{run}.log"))).unwrap();
            assert_no_secret(&log, &secret);
            assert!(log.contains("ordinary log prefix") && log.contains("ordinary log suffix"));
            let snapshot: String = sqlx::query_scalar("SELECT agent FROM local_runs WHERE id=?")
                .bind(run.to_string())
                .fetch_one(&journal.pool)
                .await
                .unwrap();
            assert_no_secret(&snapshot, &secret);
        }
    }
    #[tokio::test]
    async fn credential_stderr_filter_handles_bytewise_utf8_and_escaped_values() {
        let root = directory();
        let secret = synthetic_secret();
        let redactor = AgentRedactor::default();
        redactor.remember(&json!(secret));
        let journal = Journal::open(root.path(), Uuid::new_v4()).await.unwrap();
        let run = Uuid::new_v4();
        let task = task(root.path());
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: journal.runner,
                    task,
                    deadline: Utc::now() + chrono::Duration::minutes(1),
                },
                legacy(),
            )
            .await
            .unwrap();
        let input = format!(
            "ordinary prefix {secret} escaped {} ordinary suffix",
            serde_json::to_string(&secret).unwrap()
        );
        let (mut writer, reader) = tokio::io::duplex(1);
        let writing = tokio::spawn(async move {
            writer.write_all(input.as_bytes()).await.unwrap();
            writer.shutdown().await.unwrap();
        });
        assert!(
            read_agent_diagnostics(
                reader,
                Some(LogSink {
                    journal: journal.clone(),
                    id: run,
                    stdout: true
                }),
                CancellationToken::new(),
                redactor
            )
            .await
            .unwrap()
        );
        writing.await.unwrap();
        let log =
            std::fs::read_to_string(root.path().join("logs").join(format!("{run}.log"))).unwrap();
        assert_no_secret(&log, &secret);
        assert!(log.starts_with("ordinary prefix ") && log.ends_with(" ordinary suffix"));
        let events: Vec<String> = sqlx::query_scalar("SELECT payload FROM pending_events")
            .fetch_all(&journal.pool)
            .await
            .unwrap();
        for event in events {
            assert_no_secret(&event, &secret);
        }
    }
    #[tokio::test]
    async fn credential_redactor_never_scans_other_instances_or_snapshots_a_secret_in_config() {
        let root = directory();
        let (mut agent, pool, instance) = configured_fixture(root.path(), "configuration").await;
        let secret = synthetic_secret();
        sqlx::query("INSERT INTO extension_instances SELECT ?,source,extension,root,version,revision,enabled,removed,grants,declaration,config,? FROM extension_instances WHERE id=?")
            .bind(Uuid::new_v4().to_string()).bind(json!({"token":secret}).to_string()).bind(instance.to_string()).execute(&pool).await.unwrap();
        let Backend::SdkBackend(backend) = &agent.backend else {
            unreachable!()
        };
        let host = AgentHost::load(backend).await.unwrap();
        assert!(
            host.redactor.text(&secret) == secret,
            "an unrelated instance must not contribute redaction values"
        );
        sqlx::query("UPDATE extension_instances SET config=? WHERE id=?")
            .bind(json!({"endpoint":"synthetic-credential"}).to_string())
            .bind(instance.to_string())
            .execute(&pool)
            .await
            .unwrap();
        let error = agent
            .pin_configuration()
            .await
            .expect_err("credential-bearing config must not be snapshotted");
        assert!(!error.to_string().contains("synthetic-credential"));
        let Backend::SdkBackend(backend) = agent.backend else {
            unreachable!()
        };
        assert!(backend.configuration.is_none());
    }
    #[test]
    fn snapshots_preserve_real_backends_and_read_old_command_journals() {
        let legacy = legacy();
        let agent: Agent = serde_json::from_value(serde_json::to_value(&legacy).unwrap()).unwrap();
        assert!(matches!(&agent.backend, Backend::LegacyCommand(_)));
        assert_eq!(agent.digest(), legacy.digest());
        let restored: Agent =
            serde_json::from_value(serde_json::to_value(&agent).unwrap()).unwrap();
        assert_eq!(restored.digest(), agent.digest());
        let root = directory();
        let sdk = fixture(root.path(), "records");
        let encoded = serde_json::to_value(&sdk).unwrap();
        assert!(encoded.get("submit").is_none());
        assert!(encoded["config"].get("submit").is_none());
        let restored: Agent = serde_json::from_value(encoded).unwrap();
        assert!(matches!(restored.backend, Backend::SdkBackend(_)));
        assert_eq!(restored.digest(), sdk.digest());
    }
    #[tokio::test]
    async fn record_acknowledgement_commits_session_before_execution_returns() {
        let root = directory();
        let agent = fixture(root.path(), "records");
        let task = task(root.path());
        let id = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let journal = Journal::open(&root.path().join("journal"), runner)
            .await
            .unwrap();
        let assignment = Assignment {
            version: 1,
            runner_id: runner,
            run_id: id,
            task: task.clone(),
            deadline: Utc::now() + chrono::Duration::minutes(1),
        };
        journal.insert(&assignment, &agent).await.unwrap();
        journal.phase(id, "started", None).await.unwrap();
        let output = agent
            .execute(
                &task,
                id,
                Duration::from_secs(15),
                CancellationToken::new(),
                Some(LogSink {
                    journal: journal.clone(),
                    id,
                    stdout: true,
                }),
                None,
            )
            .await
            .unwrap();
        assert!(output.confirmed);
        assert_eq!(output.code, Some(0));
        assert_eq!(output.result, Some(json!("synthetic result")));
        assert_eq!(
            output.result_value(&agent.output).unwrap(),
            json!("synthetic result")
        );
        let session = journal.session(id).await.unwrap().unwrap();
        assert_eq!(session.id, "synthetic-session");
        assert!(!session.ready);
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pending_events WHERE run_id=?")
            .bind(id.to_string())
            .fetch_one(&journal.pool)
            .await
            .unwrap();
        assert_eq!(
            count, 2,
            "Session and log must already be durable; Result is not a forged Run event"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("ack")).unwrap(),
            "3"
        );
    }
    #[tokio::test]
    async fn invalid_identities_queries_and_handshakes_never_become_success() {
        for mode in [
            "wrong_run",
            "wrong_seq",
            "query_record",
            "wrong_registration",
            "disconnect",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let task = task(root.path());
            let operation = if mode == "query_record" {
                AgentOperation::Models
            } else {
                AgentOperation::Execute
            };
            let request = agent.request(
                operation,
                &task,
                Some(Uuid::new_v4()),
                Duration::from_secs(10),
            );
            let error = agent
                .invoke(request, CancellationToken::new(), None)
                .await
                .err()
                .expect(mode);
            let uncertain = error.downcast_ref::<CommandStartedError>().is_some();
            assert_eq!(
                uncertain,
                !matches!(mode, "query_record" | "wrong_registration"),
                "{mode}: {error:#}"
            );
        }
    }
    #[tokio::test]
    async fn cancellation_is_scoped_to_one_peer_and_reaches_it_before_cleanup() {
        let root = directory();
        let agent = fixture(root.path(), "wait");
        let task = task(root.path());
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let first = tokio::spawn({
            let agent = agent.clone();
            let task = task.clone();
            let cancel = cancel.clone();
            async move {
                agent
                    .execute(&task, a, Duration::from_secs(20), cancel, None, None)
                    .await
            }
        });
        let second = tokio::spawn({
            let agent = agent.clone();
            let task = task.clone();
            async move {
                agent
                    .execute(
                        &task,
                        b,
                        Duration::from_secs(20),
                        CancellationToken::new(),
                        None,
                        None,
                    )
                    .await
            }
        });
        await_file(&root.path().join(format!("accepted-{a}"))).await;
        await_file(&root.path().join(format!("accepted-{b}"))).await;
        cancel.cancel();
        let stopped = first.await.unwrap().unwrap();
        assert_eq!(stopped.stopped, Some("cancelled"));
        assert!(stopped.confirmed);
        assert!(root.path().join(format!("cancelled-{a}")).exists());
        assert!(!second.is_finished(), "cancelling A must leave B alive");
        std::fs::write(root.path().join(format!("release-{b}")), "release").unwrap();
        let completed = second.await.unwrap().unwrap();
        assert_eq!(completed.code, Some(0));
        assert!(completed.confirmed && completed.stopped.is_none());
    }
    #[tokio::test]
    async fn cancellation_cannot_hide_a_protocol_identity_violation() {
        let root = directory();
        let agent = fixture(root.path(), "cancel_wrong_run");
        let task = task(root.path());
        let id = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let pending = tokio::spawn({
            let agent = agent.clone();
            let cancel = cancel.clone();
            async move {
                agent
                    .execute(&task, id, Duration::from_secs(15), cancel, None, None)
                    .await
            }
        });
        await_file(&root.path().join(format!("accepted-{id}"))).await;
        cancel.cancel();
        let error = pending.await.unwrap().err().unwrap();
        assert!(error.is::<CommandStartedError>());
    }
    #[tokio::test]
    async fn query_and_interactive_plan_never_receive_the_saved_prompt() {
        let root = directory();
        let agent = fixture(root.path(), "query");
        let mut task = task(root.path());
        task.session = Some(crate::config::SessionBinding {
            id: "synthetic-session".into(),
        });
        for operation in [
            AgentOperation::Models,
            AgentOperation::SessionInspect,
            AgentOperation::SessionInteractive,
        ] {
            let request = agent.request(operation, &task, None, Duration::from_secs(10));
            let id = request.id;
            let response = agent
                .invoke(request, CancellationToken::new(), None)
                .await
                .unwrap();
            assert!(response.confirmed);
            let received: Value = serde_json::from_slice(
                &std::fs::read(root.path().join(format!("request-{id}"))).unwrap(),
            )
            .unwrap();
            assert!(received["context"].get("prompt").is_none());
            assert_eq!(
                received["context"]["cwd"],
                root.path()
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            );
        }
    }
    #[tokio::test]
    async fn saved_job_keeps_backend_identity_and_cancel_ack_is_not_terminal() {
        let root = directory();
        let agent = fixture(root.path(), "async");
        let task = task(root.path());
        let id = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let journal = Journal::open(&root.path().join("journal"), runner)
            .await
            .unwrap();
        let assignment = Assignment {
            version: 1,
            runner_id: runner,
            run_id: id,
            task: task.clone(),
            deadline: Utc::now() + chrono::Duration::minutes(1),
        };
        journal.insert(&assignment, &agent).await.unwrap();
        let submitted = agent
            .job_command(
                AgentOperation::Submit,
                &task,
                id,
                None,
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        let response: Value = serde_json::from_str(&submitted.stdout).unwrap();
        assert_eq!(response["job_id"], "synthetic-job");
        journal
            .phase(id, "started", Some("synthetic-job"))
            .await
            .unwrap();
        let newer = fixture(root.path(), "async_wrong_job");
        assert_ne!(newer.digest(), agent.digest());
        let saved = journal.get(id).await.unwrap().unwrap();
        assert_eq!(saved.agent.digest(), agent.digest());
        let cancel = saved
            .agent
            .job_command(
                AgentOperation::Cancel,
                &saved.assignment.task,
                id,
                saved.job.as_deref(),
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&cancel.stdout).unwrap(),
            json!({})
        );
        let status = saved
            .agent
            .job_command(
                AgentOperation::Poll,
                &saved.assignment.task,
                id,
                saved.job.as_deref(),
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&status.stdout).unwrap()["state"],
            "running"
        );
        assert!(
            newer
                .job_command(
                    AgentOperation::Poll,
                    &task,
                    id,
                    saved.job.as_deref(),
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    None
                )
                .await
                .is_err()
        );
        let calls = std::fs::read_to_string(root.path().join("operations")).unwrap();
        assert_eq!(
            calls
                .lines()
                .filter(|operation| *operation == "submit")
                .count(),
            1
        );
    }
    #[tokio::test]
    async fn lost_submit_reply_is_uncertain_and_is_never_replayed_by_bridge() {
        let root = directory();
        let agent = fixture(root.path(), "async_disconnect");
        let error = agent
            .job_command(
                AgentOperation::Submit,
                &task(root.path()),
                Uuid::new_v4(),
                None,
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await
            .err()
            .unwrap();
        assert!(error.is::<CommandStartedError>());
        assert_eq!(
            std::fs::read_to_string(root.path().join("operations")).unwrap(),
            "submit\n"
        );
    }
    #[tokio::test]
    async fn pre_cancelled_and_expired_requests_never_deliver_an_invocation() {
        let root = directory();
        let agent = fixture(root.path(), "query");
        let task = task(root.path());
        let token = CancellationToken::new();
        token.cancel();
        let request = agent.request(
            AgentOperation::Execute,
            &task,
            Some(Uuid::new_v4()),
            Duration::from_secs(10),
        );
        let id = request.id;
        let error = agent.invoke(request, token, None).await.err().unwrap();
        assert!(!error.is::<CommandStartedError>());
        assert!(!root.path().join(format!("request-{id}")).exists());
        let mut expired = agent.request(
            AgentOperation::Execute,
            &task,
            Some(Uuid::new_v4()),
            Duration::from_secs(10),
        );
        expired.context.deadline = Utc::now() - chrono::Duration::seconds(1);
        let id = expired.id;
        assert!(
            agent
                .invoke(expired, CancellationToken::new(), None)
                .await
                .is_err()
        );
        assert!(!root.path().join(format!("request-{id}")).exists());
    }
    #[tokio::test]
    async fn acknowledged_result_is_not_published_after_a_nonzero_exit() {
        let root = directory();
        let agent = fixture(root.path(), "partial_nonzero");
        let output = agent
            .execute(
                &task(root.path()),
                Uuid::new_v4(),
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(output.code, Some(1));
        assert_eq!(output.result, Some(Value::Null));
    }
    async fn journalled_native(
        root: &Path,
        mode: &str,
        cancel: CancellationToken,
    ) -> (Result<process::Output>, Journal, Uuid) {
        let agent = fixture(root, mode);
        let task = task(root);
        let run = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let journal = Journal::open(&root.join("journal"), runner).await.unwrap();
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task: task.clone(),
                    deadline: Utc::now() + chrono::Duration::minutes(1),
                },
                &agent,
            )
            .await
            .unwrap();
        journal.phase(run, "started", None).await.unwrap();
        let output = agent
            .execute(
                &task,
                run,
                Duration::from_secs(15),
                cancel,
                Some(LogSink {
                    journal: journal.clone(),
                    id: run,
                    stdout: true,
                }),
                None,
            )
            .await;
        (output, journal, run)
    }
    #[tokio::test]
    async fn native_facts_are_durable_and_keep_os_exit_separate_from_normalized_outcome() {
        for mode in ["facts_success", "facts_normalized_failure"] {
            let root = directory();
            let (output, journal, run) =
                journalled_native(root.path(), mode, CancellationToken::new()).await;
            let output = output.unwrap();
            assert!(output.confirmed);
            assert_eq!(
                output.code,
                Some(if mode == "facts_success" { 0 } else { 1 })
            );
            assert_eq!(
                output.result,
                Some(if mode == "facts_success" {
                    json!("ACKed native report")
                } else {
                    Value::Null
                })
            );
            let events:Vec<(String,String)>=sqlx::query_as("SELECT event_type,data FROM extension_events WHERE event_type IN ('execution.started','execution.input_submitted','execution.native_finished') ORDER BY rowid").fetch_all(&journal.pool).await.unwrap();
            assert_eq!(events.len(), 3);
            assert_eq!(
                events
                    .iter()
                    .map(|event| event.0.as_str())
                    .collect::<Vec<_>>(),
                vec![
                    "execution.started",
                    "execution.input_submitted",
                    "execution.native_finished"
                ]
            );
            for (index, (_, payload)) in events.iter().enumerate() {
                let payload: Value = serde_json::from_str(payload).unwrap();
                assert_eq!(payload["run_id"], json!(run));
                assert_eq!(payload["seq"], json!(index + 1));
            }
            let finished: Value = serde_json::from_str(&events[2].1).unwrap();
            assert_eq!(
                finished["exit_code"], 0,
                "normalizer failure must not rewrite the actual OS exit"
            );
        }
    }
    #[tokio::test]
    async fn malformed_native_facts_and_missing_completion_stay_uncertain() {
        for mode in [
            "facts_finished_first",
            "facts_duplicate_start",
            "facts_duplicate_input",
            "facts_worker_pid",
            "facts_foreign_pid",
            "facts_wrong_run",
            "facts_wrong_seq",
            "facts_start_failed_after_start",
            "facts_no_finish",
            "facts_disconnect",
            "start_failed_disconnect",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let error = agent
                .execute(
                    &task(root.path()),
                    Uuid::new_v4(),
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    None,
                    None,
                )
                .await
                .err()
                .expect(mode);
            assert!(error.is::<CommandStartedError>(), "{mode}: {error:#}");
        }
    }
    #[tokio::test]
    async fn explicit_not_started_evidence_and_clean_teardown_is_definitive_failure() {
        for mode in [
            "start_failed_reply",
            "start_failed_error",
            "not_started_reply",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let error = agent
                .execute(
                    &task(root.path()),
                    Uuid::new_v4(),
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    None,
                    None,
                )
                .await
                .err()
                .expect(mode);
            assert!(error.is::<NativeNotStarted>(), "{mode}: {error:#}");
            assert!(!error.is::<CommandStartedError>());
        }
    }
    #[tokio::test]
    async fn cancelled_native_can_finish_but_cannot_start_new_work() {
        for mode in [
            "facts_cancel_finish",
            "facts_cancel_finished_no_reply",
            "facts_cancel_config_request",
            "facts_cancel_new_start",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let task = task(root.path());
            let run = Uuid::new_v4();
            let cancel = CancellationToken::new();
            let pending = tokio::spawn({
                let cancel = cancel.clone();
                async move {
                    agent
                        .execute(&task, run, Duration::from_secs(15), cancel, None, None)
                        .await
                }
            });
            await_file(&root.path().join(format!("accepted-{run}"))).await;
            cancel.cancel();
            let output = pending.await.unwrap();
            if mode != "facts_cancel_new_start" {
                let output = output.unwrap();
                assert_eq!(output.stopped, Some("cancelled"));
                assert!(output.confirmed);
                assert_eq!(output.result, Some(Value::Null));
            } else {
                assert!(output.err().unwrap().is::<CommandStartedError>());
            }
        }
    }
    #[tokio::test]
    async fn forced_cancel_records_host_cleanup_without_inventing_a_native_exit_code() {
        let root = directory();
        let agent = fixture(root.path(), "facts_cancel_no_finish");
        let task = task(root.path());
        let run = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let journal = Journal::open(&root.path().join("journal"), runner)
            .await
            .unwrap();
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task: task.clone(),
                    deadline: Utc::now() + chrono::Duration::minutes(1),
                },
                &agent,
            )
            .await
            .unwrap();
        journal.phase(run, "started", None).await.unwrap();
        let pending = tokio::spawn({
            let cancel = cancel.clone();
            let journal = journal.clone();
            async move {
                agent
                    .execute(
                        &task,
                        run,
                        Duration::from_secs(15),
                        cancel,
                        Some(LogSink {
                            journal,
                            id: run,
                            stdout: true,
                        }),
                        None,
                    )
                    .await
            }
        });
        await_file(&root.path().join(format!("accepted-{run}"))).await;
        cancel.cancel();
        let output = pending.await.unwrap().unwrap();
        assert!(output.confirmed);
        assert_eq!(output.stopped, Some("cancelled"));
        let payload: String = sqlx::query_scalar(
            "SELECT data FROM extension_events WHERE event_type='execution.native_finished'",
        )
        .fetch_one(&journal.pool)
        .await
        .unwrap();
        let payload: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["source"], "runner_cleanup");
        assert!(payload["exit_code"].is_null());
        assert_eq!(payload["stopped"], "cancelled");
        assert_eq!(payload["after_seq"], 2);
    }
    #[test]
    fn host_stop_is_sealed_even_when_sdk_error_arrives_before_monitor_update() {
        let control = InvocationControl::default();
        let cancel = CancellationToken::new();
        let cancelled = AgentRemoteError {
            code: Some("cancelled".into()),
            message: "Invocation cancelled".into(),
        };
        assert!(control.reason().is_none());
        cancel.cancel();
        assert!(!cancelled.confirms_host_stop(control.reason()));
        let stopped = control.seal(
            &cancel,
            tokio::time::Instant::now() + Duration::from_secs(30),
        );
        assert_eq!(stopped, Some("cancelled"));
        assert!(cancelled.confirms_host_stop(stopped));
        assert!(!cancelled.confirms_host_stop(None));
        let message_only = AgentRemoteError {
            code: Some("plugin_error".into()),
            message: "Invocation cancelled".into(),
        };
        assert!(!message_only.confirms_host_stop(stopped));
        let timeout = AgentRemoteError {
            code: Some("timeout".into()),
            message: "Agent request deadline exceeded".into(),
        };
        assert!(timeout.confirms_host_stop(Some("timed_out")));
        assert!(!timeout.confirms_host_stop(Some("cancelled")));
    }
    #[tokio::test]
    async fn explicit_uncertain_or_unconfirmed_reply_is_not_overridden_by_host_cancel_and_local_cleanup()
     {
        for mode in [
            "facts_cancel_explicit_uncertain",
            "facts_cancel_explicit_unconfirmed",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let task = task(root.path());
            let run = Uuid::new_v4();
            let cancel = CancellationToken::new();
            let pending = tokio::spawn({
                let cancel = cancel.clone();
                async move {
                    agent
                        .execute(&task, run, Duration::from_secs(15), cancel, None, None)
                        .await
                }
            });
            await_file(&root.path().join(format!("accepted-{run}"))).await;
            cancel.cancel();
            let result = pending.await.unwrap();
            if mode == "facts_cancel_explicit_uncertain" {
                let error = result
                    .err()
                    .expect("handler uncertainty must not become confirmed cancellation");
                assert!(
                    error.is::<CommandStartedError>(),
                    "explicit uncertainty must retain the Runner unknown boundary"
                );
                assert!(
                    error
                        .to_string()
                        .contains("synthetic remote native execution remains unverified")
                );
                assert!(!error.is::<NativeNotStarted>());
            } else {
                let output = result.unwrap();
                assert_eq!(output.stopped, Some("cancelled"));
                assert!(
                    !output.confirmed,
                    "local cleanup must not upgrade the handler's unconfirmed cancellation"
                );
                assert_eq!(output.result, Some(Value::Null));
            }
        }
    }
    #[tokio::test]
    async fn actual_typescript_sdk_handler_confirms_cancel_without_affecting_another_run() {
        let root = directory();
        let agent = typescript_fixture(root.path());
        let mut task = task(root.path());
        task.prompt = "wait-for-cancel".into();
        let run = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let pending = tokio::spawn({
            let agent = agent.clone();
            let task = task.clone();
            let cancel = cancel.clone();
            async move {
                agent
                    .execute(&task, run, Duration::from_secs(15), cancel, None, None)
                    .await
            }
        });
        await_file(&root.path().join(format!("ts-ready-{run}"))).await;
        let mut independent = task.clone();
        independent.prompt = "independent".into();
        let second = tokio::spawn({
            let agent = agent.clone();
            async move {
                agent
                    .execute(
                        &independent,
                        Uuid::new_v4(),
                        Duration::from_secs(10),
                        CancellationToken::new(),
                        None,
                        None,
                    )
                    .await
            }
        });
        cancel.cancel();
        let output = pending.await.unwrap().unwrap();
        assert!(output.confirmed);
        assert_eq!(output.stopped, Some("cancelled"));
        assert_eq!(output.result, Some(Value::Null));
        assert!(
            root.path().join(format!("ts-aborted-{run}")).exists(),
            "real TS controller must observe the host cancel frame"
        );
        let second = second.await.unwrap().unwrap();
        assert!(second.confirmed && second.stopped.is_none());
        assert_eq!(second.result, Some(json!("independent synthetic report")));
        for prompt in ["self-cancel", "message-only", "typed-self-cancel"] {
            let mut task = task.clone();
            task.prompt = prompt.into();
            let error = agent
                .execute(
                    &task,
                    Uuid::new_v4(),
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    None,
                    None,
                )
                .await
                .err()
                .expect("plugin must not authorize its own cancellation");
            assert!(error.is::<CommandStartedError>());
        }
    }
    #[tokio::test]
    async fn actual_typescript_wrapper_abort_without_native_facts_cannot_confirm_remote_stop() {
        let root = directory();
        let agent = typescript_fixture(root.path());
        let mut task = task(root.path());
        task.prompt = "throw-on-abort".into();
        let run = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let pending = tokio::spawn({
            let cancel = cancel.clone();
            async move {
                agent
                    .execute(&task, run, Duration::from_secs(15), cancel, None, None)
                    .await
            }
        });
        await_file(&root.path().join(format!("ts-ready-{run}"))).await;
        cancel.cancel();
        let error = pending
            .await
            .unwrap()
            .err()
            .expect("a wrapper abort is not remote termination evidence");
        assert!(error.is::<CommandStartedError>());
    }
    #[tokio::test]
    async fn actual_typescript_sdk_handler_confirms_host_deadline() {
        let root = directory();
        let agent = typescript_fixture(root.path());
        let mut task = task(root.path());
        task.prompt = "wait-for-deadline".into();
        let run = Uuid::new_v4();
        let output = agent
            .execute(
                &task,
                run,
                Duration::from_secs(2),
                CancellationToken::new(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(output.confirmed);
        assert_eq!(output.stopped, Some("timed_out"));
        assert_eq!(output.result, Some(Value::Null));
        assert!(root.path().join(format!("ts-aborted-{run}")).exists());
    }
    #[tokio::test]
    async fn inline_and_acknowledged_results_must_agree_when_both_are_present() {
        for mode in [
            "result_inline_equal",
            "result_inline_conflict",
            "result_inline_null",
            "result_stream_null",
        ] {
            let root = directory();
            let agent = fixture(root.path(), mode);
            let result = agent
                .execute(
                    &task(root.path()),
                    Uuid::new_v4(),
                    Duration::from_secs(10),
                    CancellationToken::new(),
                    None,
                    None,
                )
                .await;
            if mode == "result_inline_conflict" {
                let error = result.err().expect("conflicting final reports must fail");
                assert!(error.is::<CommandStartedError>());
                assert!(error.to_string().contains("conflicting final Agent result"));
            } else {
                let output = result.unwrap();
                assert_eq!(
                    output.result_value(&agent.output).unwrap(),
                    json!({"a":1,"b":2})
                );
            }
        }
    }
    #[cfg(unix)]
    async fn await_native_exit(pid: i32) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if unsafe { libc::kill(pid, 0) } == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    break;
                }
                // A killed orphan may briefly remain a zombie until the OS reaps it.
                let status = Command::new("ps")
                    .args(["-o", "stat=", "-p", &pid.to_string()])
                    .output()
                    .await
                    .unwrap();
                if String::from_utf8_lossy(&status.stdout)
                    .trim_start()
                    .starts_with('Z')
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("synthetic native process survived control escalation");
    }
    #[cfg(unix)]
    async fn blocked_journal_case(expire: bool) {
        let root = directory();
        let agent = fixture(root.path(), "facts_blocked_record");
        let task = task(root.path());
        let run = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let journal = Journal::open(&root.path().join("journal"), runner)
            .await
            .unwrap();
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task: task.clone(),
                    deadline: Utc::now() + chrono::Duration::minutes(1),
                },
                &agent,
            )
            .await
            .unwrap();
        journal.phase(run, "started", None).await.unwrap();
        let pending = tokio::spawn({
            let cancel = cancel.clone();
            let journal = journal.clone();
            async move {
                agent
                    .execute(
                        &task,
                        run,
                        Duration::from_secs(if expire { 3 } else { 30 }),
                        cancel,
                        Some(LogSink {
                            journal,
                            id: run,
                            stdout: true,
                        }),
                        None,
                    )
                    .await
            }
        });
        await_file(&root.path().join(format!("accepted-{run}"))).await;
        await_file(&root.path().join(format!("native-ready-{run}"))).await;
        let held = journal.pool.acquire().await.unwrap();
        std::fs::write(root.path().join(format!("release-record-{run}")), "release").unwrap();
        await_file(&root.path().join(format!("record-sent-{run}"))).await;
        if !expire {
            cancel.cancel();
        }
        // All three observations must occur before releasing the only Journal connection.
        await_file(&root.path().join(format!("cancelled-{run}"))).await;
        await_file(&root.path().join(format!("term-{run}"))).await;
        let pid: i32 = std::fs::read_to_string(root.path().join(format!("native-pid-{run}")))
            .unwrap()
            .parse()
            .unwrap();
        await_native_exit(pid).await;
        assert!(
            !pending.is_finished(),
            "process signals are not durable completion or an ACK"
        );
        assert!(
            !root.path().join(format!("record-ack-{run}")).exists(),
            "a blocked record must never be acknowledged early"
        );
        drop(held);
        let result = tokio::time::timeout(Duration::from_secs(10), pending)
            .await
            .unwrap()
            .unwrap();
        match result {
            Ok(output) => {
                assert!(output.confirmed);
                assert_eq!(
                    output.stopped,
                    Some(if expire { "timed_out" } else { "cancelled" })
                );
            }
            Err(error) => assert!(error.is::<CommandStartedError>()),
        }
        let payload: String = sqlx::query_scalar(
            "SELECT data FROM extension_events WHERE event_type='execution.native_finished'",
        )
        .fetch_one(&journal.pool)
        .await
        .unwrap();
        let data: Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(data["source"], "runner_cleanup");
        assert_eq!(
            data["after_seq"], 3,
            "Root must have been suspended in serial record persistence, not bypassed it"
        );
        assert!(data["exit_code"].is_null());
        let logs:i64=sqlx::query_scalar("SELECT count(*) FROM pending_events WHERE run_id=? AND json_extract(payload,'$.kind')='log'").bind(run.to_string()).fetch_one(&journal.pool).await.unwrap();
        assert_eq!(logs, 1);
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn blocked_journal_does_not_block_cancel_deadline_or_process_escalation() {
        tokio::join!(blocked_journal_case(false), blocked_journal_case(true));
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn completion_reaps_a_descendant_that_outlives_the_sdk_parent() {
        let root = directory();
        let agent = fixture(root.path(), "descendant");
        let output = agent
            .execute(
                &task(root.path()),
                Uuid::new_v4(),
                Duration::from_secs(15),
                CancellationToken::new(),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(output.confirmed);
        let group: i32 = std::fs::read_to_string(root.path().join("group"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(unsafe { libc::kill(-group, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
    // A disposable external protocol peer, not a second implementation of SDK internals.
    const PEER: &str = r#"
const fs = require('node:fs');
const path = require('node:path');
const {spawn} = require('node:child_process');
const readline = require('node:readline');
const {randomUUID} = require('node:crypto');
const descriptor = JSON.parse(process.argv[2]), mode = process.argv[3], root = process.argv[4];
const send = value => process.stdout.write(JSON.stringify({jsonrpc:'2.0',...value})+'\n');
let request, index=0, timer, modeRecordWrong=false, nativeChild, nextNativeAck;
const complete = outcome => {clearInterval(timer); send({id:request.id,result:{kind:'execution',value:outcome}})};
const record = () => {
  index++;
  let data = index===1 ? {type:'session',data:{version:1,id:'synthetic-session',cwd:request.context.cwd,persisted:false,title:null}}
    : index===2 ? {type:'log',data:{stream:'stdout',text:'synthetic log\n'}} : {type:'result',data:'synthetic result'};
  send({id:'record-'+index,method:'agent.record',params:{request_id:request.id,run_id:(mode==='wrong_run' || modeRecordWrong)?randomUUID():request.context.run_id,seq:mode==='wrong_seq'?index+1:index,record:{version:1,...data}}});
};
const nativeOutput = (method, payload, next) => {
 index++; nextNativeAck=next;
 send({id:'native-'+index,method,params:{request_id:request.id,run_id:mode==='facts_wrong_run'?randomUUID():request.context.run_id,seq:mode==='facts_wrong_seq'?index+1:index,...payload}});
};
const nativeFact = (fact,next) => nativeOutput('agent.fact',{fact},next);
const nativeResult = next => nativeOutput('agent.record',{record:{version:1,type:'result',data:'ACKed native report'}},next);
const endNative = (code,next) => {
 nativeChild.once('exit', actualCode => {
  if(mode==='facts_no_finish') {complete({outcome:'completed',exit_code:0,result:null,confirmed:true}); return;}
  nativeFact({event:'native_finished',exit_code:actualCode},next);
 });
 nativeChild.stdin.end(String(code));
};
const beginNative = () => {
 if(mode==='facts_finished_first') {nativeFact({event:'native_finished',exit_code:0},()=>{});return;}
 if(mode.startsWith('start_failed')) {
  const missing=spawn(path.join(root,'nonexistent-native'),[],{stdio:'ignore'});
  missing.once('error',()=>nativeFact({event:'start_failed',message:'synthetic native launch failure'},()=>{
   if(mode==='start_failed_disconnect') process.stdin.destroy();
   else if(mode==='start_failed_error') send({id:request.id,error:{code:-32000,message:'synthetic native launch error'}});
   else complete({outcome:'not_started',message:'synthetic native launch failure'});
  })); return;
 }
 if(mode==='not_started_reply') {complete({outcome:'not_started',message:'synthetic native launch failure'});return;}
 const childScript=mode==='facts_blocked_record'
  ? 'const fs=require("node:fs");process.on("SIGTERM",()=>{});fs.writeFileSync(process.argv[1],"ready");process.stdin.resume()'
  : 'process.stdin.on("data",d=>process.exit(Number(d.toString())));process.stdin.resume()';
 nativeChild=spawn(process.execPath,['-e',childScript,path.join(root,'native-ready-'+request.context.run_id)],{stdio:['pipe','ignore','ignore']});
 if(mode==='facts_blocked_record') fs.writeFileSync(path.join(root,'native-pid-'+request.context.run_id),String(nativeChild.pid));
 nativeChild.once('spawn',()=>nativeFact({event:'started',pid:mode==='facts_worker_pid'?process.pid:mode==='facts_foreign_pid'?process.ppid:nativeChild.pid},()=>{
  if(mode==='facts_duplicate_start') {nativeFact({event:'started',pid:nativeChild.pid},()=>{});return;}
  if(mode==='facts_start_failed_after_start') {nativeFact({event:'start_failed',message:'contradictory failure'},()=>{});return;}
  nativeFact({event:'input_submitted'},()=>{
   if(mode==='facts_duplicate_input') {nativeFact({event:'input_submitted'},()=>{});return;}
   if(mode==='facts_blocked_record') {
    fs.writeFileSync(path.join(root,'accepted-'+request.context.run_id),'accepted');
    process.on('SIGTERM',()=>fs.writeFileSync(path.join(root,'term-'+request.context.run_id),'term'));
    timer=setInterval(()=>{if(fs.existsSync(path.join(root,'release-record-'+request.context.run_id))){clearInterval(timer);nativeOutput('agent.record',{record:{version:1,type:'log',data:{stream:'stdout',text:'synthetic blocked record\n'}}},()=>fs.writeFileSync(path.join(root,'record-ack-'+request.context.run_id),'acked'));fs.writeFileSync(path.join(root,'record-sent-'+request.context.run_id),'sent');}},10);return;
   }
   if(mode.startsWith('facts_cancel')) {fs.writeFileSync(path.join(root,'accepted-'+request.context.run_id),'accepted');return;}
   endNative(0,()=>{
    if(mode==='facts_disconnect') {process.stdin.destroy();return;}
    nativeResult(()=>complete({outcome:'completed',exit_code:mode==='facts_normalized_failure'?1:0,result:null,confirmed:true}));
   });
  });
 }));
};
readline.createInterface({input:process.stdin}).on('close',()=>clearInterval(timer)).on('line', line => {
 const message=JSON.parse(line);
 if(message.method==='initialize') {
  const registered = mode==='wrong_registration'?{...descriptor,name:'Different'}:descriptor;
  send({id:message.id,result:{protocol_version:1,agents:{synthetic:registered}}}); return;
 }
 if(message.method==='cancel') {
  if(mode==='facts_cancel_explicit_uncertain') {endNative(130,()=>complete({outcome:'uncertain',reason:'synthetic remote native execution remains unverified'}));return;}
  if(mode==='facts_cancel_explicit_unconfirmed') {endNative(130,()=>complete({outcome:'cancelled',confirmed:false}));return;}
  if(mode==='facts_blocked_record') {fs.writeFileSync(path.join(root,'cancelled-'+request.context.run_id),'cancelled');return;}
  if(mode==='facts_cancel_finish') {endNative(130,()=>complete({outcome:'cancelled',confirmed:true}));return;}
  if(mode==='facts_cancel_finished_no_reply') {endNative(130,()=>{});return;}
  if(mode==='facts_cancel_no_finish') return;
  if(mode==='facts_cancel_config_request') {send({id:'late-config',method:'config.get',params:{agent_request_id:request.id}});return;}
  if(mode==='facts_cancel_new_start') {nativeFact({event:'started',pid:nativeChild.pid},()=>{});return;}
  if(mode==='cancel_wrong_run') { modeRecordWrong=true; record(); return; }
  if(request && message.params.id===request.id) { fs.writeFileSync(path.join(root,'cancelled-'+request.context.run_id),'cancelled'); complete({outcome:'cancelled',confirmed:true}); }
  return;
 }
 if(message.method==='agent.invoke') {
  request=message.params;
  fs.writeFileSync(path.join(root,'request-'+request.id),JSON.stringify(request));
  if(!['execute','session_resume','submit'].includes(request.operation) && Object.hasOwn(request.context,'prompt')) throw Error('query leaked prompt');
  if(mode.startsWith('result_')) {
   const streamed=mode==='result_stream_null'?null:{a:1,b:2};
   const inline=mode==='result_inline_conflict'?{a:2,b:2}:mode==='result_inline_equal'?{b:2,a:1}:mode==='result_stream_null'?{b:2,a:1}:null;
   nativeOutput('agent.record',{record:{version:1,type:'result',data:streamed}},()=>complete({outcome:'completed',exit_code:0,result:inline,confirmed:true}));return;
  }
  if(mode.startsWith('facts_') || mode.startsWith('start_failed') || mode==='not_started_reply') {beginNative();return;}
  if(mode.startsWith('sync_reconcile')) {
   fs.appendFileSync(path.join(root,'operations'),request.operation+'\n');
   complete({outcome:'uncertain',reason:'synthetic native ownership remains unknown'});return;
  }
  if(mode.startsWith('async')) {
   fs.appendFileSync(path.join(root,'operations'),request.operation+'\n');
   if(request.operation==='reconcile') {
    if(mode==='async_reconcile_done') complete({outcome:'completed',exit_code:0,result:'reconciled synthetic report',confirmed:true});
    else if(mode==='async_reconcile_uncertain') complete({outcome:'uncertain',reason:'synthetic external job remains unverified'});
    else if(mode==='async_reconcile_unconfirmed') complete({outcome:'cancelled',confirmed:false});
    else complete({outcome:'running',job_id:mode==='async_reconcile_conflict'?'foreign-job':request.context.job_id});
    return;
   }
   if(mode==='async_disconnect' && request.operation==='submit') {process.exitCode=2;process.stdin.destroy();return;}
   if(request.operation==='submit') complete({outcome:'submitted',job_id:'synthetic-job'});
   else if(request.operation==='poll' && mode==='async_reconcile_poll_done') complete({outcome:'completed',exit_code:0,result:'polled synthetic report',confirmed:true});
   else if(request.operation==='cancel') complete({outcome:'cancel_requested',job_id:request.context.job_id});
   else complete({outcome:'running',job_id:mode==='async_wrong_job'?'different-job':request.context.job_id});
   return;
  }
  if(mode==='configuration' || mode.startsWith('credential_leak')) {
   send({id:'configuration',method:'config.get',params:{agent_request_id:request.id}}); return;
  }
  if(mode==='disconnect') { process.exitCode=2; process.stdin.destroy(); return; }
  if(['records','partial_nonzero','wrong_run','wrong_seq','query_record'].includes(mode)) { record(); return; }
  if(mode==='wait' || mode==='cancel_wrong_run') {
   fs.writeFileSync(path.join(root,'accepted-'+request.context.run_id),'accepted');
   timer=setInterval(()=>{if(fs.existsSync(path.join(root,'release-'+request.context.run_id))) complete({outcome:'completed',exit_code:0,result:'synthetic result',confirmed:true})},10); return;
  }
  if(mode==='descendant') {
   fs.writeFileSync(path.join(root,'group'),String(process.pid));
   const child=spawn(process.execPath,['-e','process.on("SIGTERM",()=>{});setInterval(()=>{},1000)'],{stdio:'ignore'}); child.unref();
   complete({outcome:'completed',exit_code:0,result:'synthetic result',confirmed:true}); return;
  }
  if(request.operation==='session_interactive' && mode==='interactive_environment') {
   const expected=JSON.parse(fs.readFileSync(path.join(root,'environment-hashes'),'utf8'));
   const script='const fs=require("node:fs"),crypto=require("node:crypto");const expected='+JSON.stringify(expected)+';for(const [name,hash] of Object.entries(expected)){if(crypto.createHash("sha256").update(process.env[name]??"").digest("hex")!==hash)process.exit(7);}fs.writeFileSync('+JSON.stringify(path.join(root,'environment-ok'))+',"ok");';
   send({id:request.id,result:{kind:'interactive',value:{cwd:request.context.cwd,argv:[process.execPath,'-e',script],env:{SYNTHETIC_TOKEN:{source:'credential',value:'token'},SYNTHETIC_LITERAL:{source:'literal',value:'literal {{task.prompt}}'},SYNTHETIC_INHERITED:{source:'inherited',value:'PATH'}}}}});return;
  }
  if(request.operation==='models') send({id:request.id,result:{kind:'models',value:{version:1,models:[]}}});
  else if(request.operation==='session_inspect') send({id:request.id,result:{kind:'session',value:{version:1,id:request.context.session_id,cwd:request.context.cwd,persisted:true,title:null}}});
  else if(request.operation==='session_interactive') send({id:request.id,result:{kind:'interactive',value:{cwd:request.context.cwd,argv:[process.execPath,'--version']}}});
  return;
 }
 if(message.id==='late-config') {if(!message.error) throw Error('host access remained available after cancellation');endNative(130,()=>complete({outcome:'cancelled',confirmed:true}));return;}
 if(message.id==='configuration' && Object.hasOwn(message,'result')) {
  fs.writeFileSync(path.join(root,'host-config'),JSON.stringify(message.result));
  send({id:'credential',method:'credentials.get',params:{agent_request_id:request.id,slot:'token'}}); return;
 }
 if(message.id==='credential' && Object.hasOwn(message,'result')) {
  if(mode.startsWith('credential_leak')) {
   const secret=message.result, encoded=JSON.stringify(secret);
   const rawLog='ordinary log prefix '+secret+' encoded '+encoded+' ordinary log suffix\n';
   process.stderr.write(rawLog.slice(0,22));process.stderr.write(rawLog.slice(22));
   if(mode==='credential_leak_report') {
    nativeOutput('agent.record',{record:{version:1,type:'result',data:{ordinary:'ordinary synthetic report',echo:secret,escaped:encoded}}},()=>complete({outcome:'completed',exit_code:0,result:null,confirmed:true})); return;
   }
   nativeOutput('agent.record',{record:{version:1,type:'log',data:{stream:'stdout',text:rawLog}}},()=>{
    if(mode==='credential_leak_fact') nativeFact({event:'start_failed',message:'native launch: '+secret+' encoded '+encoded},()=>send({id:request.id,error:{code:-32000,message:'native failed: '+secret+' encoded '+encoded}}));
    else if(mode==='credential_leak_not_started') complete({outcome:'not_started',message:'native failed: '+secret+' encoded '+encoded});
    else if(mode==='credential_leak_malformed') send({id:request.id,result:{kind:secret,value:null}});
    else if(mode==='credential_leak_conflict') nativeOutput('agent.record',{record:{version:1,type:'result',data:secret}},()=>complete({outcome:'completed',exit_code:0,result:'[redacted]',confirmed:true}));
    else send({id:request.id,error:{code:-32000,message:'remote failed: '+secret+' encoded '+encoded}});
   });return;
  }
  complete({outcome:'completed',exit_code:0,result:message.result==='synthetic-credential',confirmed:true}); return;
 }
 if(String(message.id).startsWith('native-') && Object.hasOwn(message,'result')) {
  if(message.result.seq!==index) throw Error('wrong native output ACK');
  const next=nextNativeAck; nextNativeAck=null; if(next) next(); return;
 }
 if(Object.hasOwn(message,'result')) {
  if(message.result.seq!==index) throw Error('wrong ACK');
  fs.writeFileSync(path.join(root,'ack'),String(index));
  if(index<3) record(); else complete({outcome:'completed',exit_code:mode==='partial_nonzero'?1:0,result:null,confirmed:true});
 }
});
"#;
}
