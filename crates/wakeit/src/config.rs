use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

fn version() -> u32 {
    1
}
fn yes() -> bool {
    true
}
fn timeout() -> String {
    "1h".into()
}
fn poll() -> String {
    "5s".into()
}
fn concurrency() -> usize {
    2
}
fn address() -> String {
    "127.0.0.1:8080".into()
}
fn replicas() -> usize {
    1
}
fn log_days() -> i32 {
    7
}
fn run_days() -> i32 {
    90
}
fn log_limit() -> usize {
    20 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Sync,
    Async,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandArg {
    Literal(String),
    Group { when_set: String, args: Vec<String> },
}
impl From<String> for CommandArg {
    fn from(v: String) -> Self {
        Self::Literal(v)
    }
}
impl From<&str> for CommandArg {
    fn from(v: &str) -> Self {
        Self::Literal(v.into())
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParameterSpec {
    #[serde(rename = "type", default = "string_type")]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    pub default: Option<String>,
    #[serde(rename = "enum", default)]
    pub choices: Vec<String>,
}
fn string_type() -> String {
    "string".into()
}
pub fn resolve_parameters(
    specs: &BTreeMap<String, ParameterSpec>,
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    ensure!(
        values.keys().all(|k| specs.contains_key(k)),
        "unknown agent parameter"
    );
    let mut resolved = BTreeMap::new();
    for (key, spec) in specs {
        ensure!(spec.kind == "string", "unsupported parameter type");
        if let Some(value) = values
            .get(key)
            .filter(|v| !v.is_empty())
            .or(spec.default.as_ref())
            .filter(|v| !v.is_empty())
        {
            ensure!(value.len() <= 4096, "parameter {key} exceeds 4096 bytes");
            ensure!(
                spec.choices.is_empty() || spec.choices.contains(value),
                "invalid value for parameter {key}"
            );
            resolved.insert(key.clone(), value.clone());
        }
    }
    Ok(resolved)
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<CommandArg>,
    pub stdin: Option<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub env_from: BTreeMap<String, String>,
    pub timeout: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Jsonl,
    Events,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputSpec {
    #[serde(default)]
    pub format: OutputFormat,
    pub result_pointer: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionAdapter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rename: Option<CommandSpec>,
    pub inspect: CommandSpec,
    pub resume: CommandSpec,
    pub interactive: CommandSpec,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub mode: AgentMode,
    pub submit: CommandSpec,
    pub status: Option<CommandSpec>,
    pub cancel: Option<CommandSpec>,
    pub detect: Option<CommandSpec>,
    pub models: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionAdapter>,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterSpec>,
    #[serde(default)]
    pub output: OutputSpec,
    #[serde(default = "poll")]
    pub poll_interval: String,
}
impl AgentConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported agent configuration version");
        validate_id(&self.id)?;
        ensure!(!self.name.trim().is_empty(), "agent name is required");
        if let Some(logo) = &self.logo {
            let local = logo.starts_with('/') && !logo.starts_with("//") && !logo.contains('\\');
            let remote = reqwest::Url::parse(logo).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            });
            ensure!(
                local || remote,
                "agent logo must be a Gateway-root path or HTTPS URL"
            );
        }
        if self.mode == AgentMode::Async {
            ensure!(self.status.is_some(), "async agent requires status");
            ensure!(
                !matches!(self.output.format, OutputFormat::Events),
                "events output requires a sync adapter"
            );
        }
        if let Some(session) = &self.session {
            ensure!(
                self.mode == AgentMode::Sync && matches!(self.output.format, OutputFormat::Events),
                "sessions require a synchronous events adapter"
            );
            ensure!(
                session.interactive.stdin.is_none(),
                "interactive continuation must inherit stdin"
            );
            let encoded = serde_json::to_string(&session.inspect)?;
            ensure!(
                !encoded.contains("task.prompt"),
                "session inspection cannot use the task prompt"
            );
        }
        duration(&self.poll_interval)?;
        for key in self.parameters.keys() {
            validate_id(key)?;
        }
        resolve_parameters(&self.parameters, &BTreeMap::new())?;
        for c in [
            Some(&self.submit),
            self.status.as_ref(),
            self.cancel.as_ref(),
            self.detect.as_ref(),
            self.models.as_ref(),
            self.session.as_ref().and_then(|s| s.list.as_ref()),
            self.session.as_ref().and_then(|s| s.rename.as_ref()),
            self.session.as_ref().map(|s| &s.inspect),
            self.session.as_ref().map(|s| &s.resume),
            self.session.as_ref().map(|s| &s.interactive),
        ]
        .into_iter()
        .flatten()
        {
            ensure!(!c.command.trim().is_empty(), "command is required");
            ensure!(
                !c.command.contains("{{"),
                "command executable cannot be templated"
            );
            if let Some(t) = &c.timeout {
                duration(t)?;
            }
            let mut values = Vec::new();
            for arg in &c.args {
                match arg {
                    CommandArg::Literal(v) => values.push(v),
                    CommandArg::Group { when_set, args } => {
                        ensure!(
                            self.parameters.contains_key(when_set),
                            "unknown conditional parameter {when_set}"
                        );
                        values.extend(args);
                    }
                }
            }
            values.extend(c.stdin.iter().chain(c.cwd.iter()).chain(c.env.values()));
            for value in values {
                validate_template(value)?;
                let mut rest = value.as_str();
                while let Some((_, tail)) = rest.split_once("{{") {
                    let (key, next) = tail.split_once("}}").context("template")?;
                    if let Some(key) = key.trim().strip_prefix("parameters.") {
                        ensure!(
                            self.parameters.contains_key(key),
                            "undeclared parameter {key}"
                        );
                    }
                    rest = next;
                }
            }
        }
        Ok(())
    }
    pub fn digest(&self) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(
            serde_json::to_vec(self).expect("config serializes"),
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    pub every: Option<String>,
    pub cron: Option<String>,
    #[serde(default)]
    pub timezone: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Overlap {
    #[default]
    Skip,
    Allow,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Misfire {
    Skip,
    #[default]
    Latest,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    #[serde(default = "timeout")]
    pub timeout: String,
    #[serde(default = "timeout")]
    pub start_grace: String,
    #[serde(default)]
    pub overlap: Overlap,
    #[serde(default)]
    pub misfire: Misfire,
}
impl Default for Execution {
    fn default() -> Self {
        Self {
            timeout: timeout(),
            start_grace: timeout(),
            overlap: Overlap::Skip,
            misfire: Misfire::Latest,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = task_schema)]
pub struct TaskConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub runner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prompt: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionBinding>,
    pub schedule: Option<Schedule>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub execution: Execution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dag: Option<crate::dag::Dag>,
}
fn task_schema(schema: &mut schemars::Schema) {
    schema.insert("oneOf".into(), serde_json::json!([
        {"required":["runner","agent","cwd","prompt"],"properties":{"dag":{"type":"null"},"runner":{"minLength":1},"agent":{"minLength":1},"cwd":{"minLength":1},"prompt":{"minLength":1}}},
        {"required":["dag"],"properties":{"dag":{"type":"object"},"runner":{"maxLength":0},"agent":{"maxLength":0},"cwd":{"maxLength":0},"prompt":{"maxLength":0},"parameters":{"maxProperties":0},"session":{"type":"null"}}}
    ]));
}

impl TaskConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "unsupported task configuration version");
        validate_id(&self.id)?;
        if let Some(dag) = &self.dag {
            ensure!(
                self.runner.is_empty()
                    && self.agent.is_empty()
                    && self.cwd.is_empty()
                    && self.prompt.is_empty()
                    && self.parameters.is_empty()
                    && self.session.is_none(),
                "DAG and top-level node execution fields are mutually exclusive"
            );
            ensure!(!self.name.trim().is_empty(), "name is required");
            ensure!(
                serde_json::to_vec(self)?.len() <= 1000 * 1024,
                "encoded task configuration exceeds 1000 KiB"
            );
            duration(&self.execution.timeout)?;
            duration(&self.execution.start_grace)?;
            if let Some(s) = &self.schedule {
                crate::schedule::validate(s)?;
            }
            return dag.validate();
        }
        validate_id(&self.agent)?;
        ensure!(
            !self.name.trim().is_empty() && !self.runner.is_empty(),
            "name and runner are required"
        );
        ensure!(!self.cwd.trim().is_empty(), "cwd is required");
        ensure!(!self.prompt.trim().is_empty(), "prompt is required");
        ensure!(self.prompt.len() <= 1024 * 1024, "prompt exceeds 1 MiB");
        ensure!(
            serde_json::to_vec(self)?.len() <= 1000 * 1024,
            "encoded task configuration exceeds 1000 KiB"
        );
        if let Some(session) = &self.session {
            crate::sessions::validate_id(&session.id)?;
        }
        duration(&self.execution.timeout)?;
        duration(&self.execution.start_grace)?;
        if let Some(s) = &self.schedule {
            crate::schedule::validate(s)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default = "address")]
    pub listen: String,
    pub public_url: String,
    pub database_url_env: String,
    pub nats_url: String,
    pub nats_public_url: String,
    pub nats_auth_dir: PathBuf,
    #[serde(default = "replicas")]
    pub nats_replicas: usize,
    #[serde(default = "log_days")]
    pub log_retention_days: i32,
    #[serde(default = "run_days")]
    pub run_retention_days: i32,
    #[serde(default = "log_limit")]
    pub log_limit_bytes: usize,
    #[serde(default)]
    pub auth: GatewayAuth,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayAuth {
    #[serde(default = "session_cookie")]
    pub session_cookie: String,
    #[serde(default = "local_auth")]
    pub mode: String,
    #[serde(default)]
    pub allow_registration: bool,
    #[serde(default)]
    pub github: Option<GitHubAuth>,
    #[serde(default)]
    pub demo_workspace: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubAuth {
    pub client_id_env: String,
    pub client_secret_env: String,
}
fn local_auth() -> String {
    "local".into()
}
fn session_cookie() -> String {
    "wakeit_session".into()
}
impl Default for GatewayAuth {
    fn default() -> Self {
        Self {
            session_cookie: session_cookie(),
            mode: local_auth(),
            allow_registration: false,
            github: None,
            demo_workspace: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub gateway: String,
    pub id: uuid::Uuid,
    pub name: String,
    pub token_file: PathBuf,
    pub nats_url: String,
    pub credentials_file: PathBuf,
    pub data_dir: PathBuf,
    #[serde(default = "concurrency")]
    pub concurrency: usize,
    pub agents: Vec<PathBuf>,
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
}
pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    yaml_serde::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )
    .context("invalid YAML")
}
pub fn duration(value: &str) -> Result<Duration> {
    let d = humantime::parse_duration(value)?;
    ensure!(
        d.as_secs_f64() >= 1.0 && d.as_secs() <= 366 * 86400,
        "duration must be between 1s and 366d"
    );
    Ok(d)
}
pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 80
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "id must use 1–80 letters, numbers, hyphens or underscores"
    );
    Ok(())
}
pub fn validate_template(input: &str) -> Result<()> {
    let mut s = input;
    while let Some((_, rest)) = s.split_once("{{") {
        let (key, next) = rest
            .split_once("}}")
            .context("unclosed template variable")?;
        ensure!(
            [
                "task.prompt",
                "task.cwd",
                "task.id",
                "run.id",
                "job.id",
                "session.id",
                "session.title"
            ]
            .contains(&key.trim())
                || key
                    .trim()
                    .strip_prefix("parameters.")
                    .is_some_and(|k| validate_id(k).is_ok()),
            "unknown template variable: {key}"
        );
        s = next;
    }
    Ok(())
}
pub fn render(
    input: &str,
    task: &TaskConfig,
    run: uuid::Uuid,
    job: Option<&str>,
) -> Result<String> {
    render_named(input, task, run, job, None)
}
pub fn render_named(
    input: &str,
    task: &TaskConfig,
    run: uuid::Uuid,
    job: Option<&str>,
    title: Option<&str>,
) -> Result<String> {
    validate_template(input)?;
    let mut result = String::new();
    let mut rest = input;
    while let Some((prefix, tail)) = rest.split_once("{{") {
        result.push_str(prefix);
        let (key, next) = tail.split_once("}}").context("template")?;
        let run_id = run.to_string();
        let value = match key.trim() {
            "task.prompt" => task.prompt.as_str(),
            "task.cwd" => &task.cwd,
            "task.id" => &task.id,
            "run.id" => &run_id,
            "job.id" => job.context("job.id unavailable before submit")?,
            "session.id" => &task.session.as_ref().context("session.id unavailable")?.id,
            "session.title" => title.context("session.title unavailable before execution claim")?,
            key if key.starts_with("parameters.") => task
                .parameters
                .get(&key[11..])
                .context("parameter unavailable; use a conditional argument group")?,
            _ => bail!("unknown template"),
        };
        result.push_str(value);
        rest = next;
    }
    result.push_str(rest);
    Ok(result)
}
pub const BUILTIN_AGENTS: &[(&str, &str)] = &[
    ("codex", include_str!("../../../agents/codex.yaml")),
    (
        "claude-code",
        include_str!("../../../agents/claude-code.yaml"),
    ),
    ("pi", include_str!("../../../agents/pi.yaml")),
    ("opencode", include_str!("../../../agents/opencode.yaml")),
    ("kimi-code", include_str!("../../../agents/kimi-code.yaml")),
    ("agy", include_str!("../../../agents/agy.yaml")),
    (
        "gemini-cli",
        include_str!("../../../agents/gemini-cli.yaml"),
    ),
    ("qwen-code", include_str!("../../../agents/qwen-code.yaml")),
    ("cursor", include_str!("../../../agents/cursor.yaml")),
    ("copilot", include_str!("../../../agents/copilot.yaml")),
    ("cline", include_str!("../../../agents/cline.yaml")),
    ("goose", include_str!("../../../agents/goose.yaml")),
];
pub fn builtin(id: &str) -> Result<&'static str> {
    BUILTIN_AGENTS
        .iter()
        .find_map(|(name, yaml)| (*name == id).then_some(*yaml))
        .with_context(|| format!("unknown builtin agent: {id}"))
}
/// Convert npm's Windows batch launchers into explicit Node entry points.
/// This is generic package-bin discovery, not an agent-specific execution path.
pub fn normalize_commands(agent: &mut AgentConfig) -> Result<()> {
    let mut session = agent.session.take();
    for spec in [
        Some(&mut agent.submit),
        agent.status.as_mut(),
        agent.cancel.as_mut(),
        agent.detect.as_mut(),
        agent.models.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        if spec.command == "wakeit" {
            spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
        }
    }
    for spec in [
        Some(&mut agent.submit),
        agent.status.as_mut(),
        agent.cancel.as_mut(),
        agent.detect.as_mut(),
        agent.models.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        normalize_native_command(spec)?;
    }
    if let Some(session) = &mut session {
        for spec in [session.list.as_mut(), session.rename.as_mut()]
            .into_iter()
            .flatten()
        {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            normalize_native_command(spec)?;
        }
        for spec in [
            &mut session.inspect,
            &mut session.resume,
            &mut session.interactive,
        ] {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            normalize_native_command(spec)?;
        }
    }
    agent.session = session;
    Ok(())
}
/// Resolve npm launchers to an explicit Node interpreter without invoking a shell.
pub fn normalize_native_command(spec: &mut CommandSpec) -> Result<()> {
    #[cfg(windows)]
    {
        let Ok(executable) = which::which(&spec.command) else {
            return Ok(());
        };
        if !executable
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("cmd"))
        {
            return Ok(());
        }
        let parent = executable.parent().context("launcher directory")?;
        let root = if parent.file_name().is_some_and(|n| n == ".bin") {
            parent
                .parent()
                .context("node_modules directory")?
                .to_owned()
        } else {
            parent.join("node_modules")
        };
        let name = executable
            .file_stem()
            .context("command name")?
            .to_string_lossy();
        let mut packages = Vec::new();
        if root.exists() {
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().starts_with('@') {
                    for package in std::fs::read_dir(entry.path())? {
                        packages.push(package?.path());
                    }
                } else {
                    packages.push(entry.path());
                }
            }
        }
        let mut found = None;
        for package in packages {
            let Ok(text) = std::fs::read_to_string(package.join("package.json")) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let bin = metadata["bin"]
                .as_object()
                .and_then(|bins| {
                    bins.iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(&name))
                        .and_then(|(_, v)| v.as_str())
                })
                .or_else(|| {
                    if metadata["name"]
                        .as_str()
                        .and_then(|v| v.rsplit('/').next())
                        .is_some_and(|v| v.eq_ignore_ascii_case(&name))
                    {
                        metadata["bin"].as_str()
                    } else {
                        None
                    }
                });
            if let Some(bin) = bin {
                let path = package.join(bin);
                let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
                let mut prefix = [0u8; 160];
                use std::io::Read;
                let read = std::fs::File::open(&path)?.read(&mut prefix)?;
                let node = matches!(extension, "js" | "mjs" | "cjs")
                    || String::from_utf8_lossy(&prefix[..read])
                        .lines()
                        .next()
                        .is_some_and(|v| v.starts_with("#!") && v.contains("node"));
                if node {
                    found = Some(path);
                    break;
                }
            }
        }
        let path=found.context("npm launcher cannot be resolved; configure node.exe and the script entry point explicitly")?;
        spec.command = "node".into();
        spec.args
            .insert(0, path.to_string_lossy().into_owned().into());
    }
    #[cfg(not(windows))]
    let _ = spec;
    Ok(())
}

/// Native titles are display metadata; the complete native ID remains execution identity.
pub fn generated_session_title(
    name: &str,
    started: chrono::DateTime<chrono::Utc>,
    timezone: &str,
) -> Result<String> {
    let timezone: chrono_tz::Tz = timezone.parse().context("invalid workspace timezone")?;
    let suffix = format!(
        "-wakeit-{}",
        started.with_timezone(&timezone).format("%Y%m%d-%H%M%S%3f")
    );
    let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = cleaned.len().min(200 - suffix.len());
    while !cleaned.is_char_boundary(end) {
        end -= 1;
    }
    Ok(format!("{}{}", &cleaned[..end], suffix))
}
