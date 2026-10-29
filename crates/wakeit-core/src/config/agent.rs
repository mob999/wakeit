use super::{TaskConfig, duration, validate_id, version};
use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn poll() -> String {
    "5s".into()
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
    ("codex", include_str!("../../../../agents/codex.yaml")),
    (
        "claude-code",
        include_str!("../../../../agents/claude-code.yaml"),
    ),
    ("pi", include_str!("../../../../agents/pi.yaml")),
    ("opencode", include_str!("../../../../agents/opencode.yaml")),
    (
        "kimi-code",
        include_str!("../../../../agents/kimi-code.yaml"),
    ),
    ("agy", include_str!("../../../../agents/agy.yaml")),
    (
        "gemini-cli",
        include_str!("../../../../agents/gemini-cli.yaml"),
    ),
    (
        "qwen-code",
        include_str!("../../../../agents/qwen-code.yaml"),
    ),
    ("cursor", include_str!("../../../../agents/cursor.yaml")),
    ("copilot", include_str!("../../../../agents/copilot.yaml")),
    ("cline", include_str!("../../../../agents/cline.yaml")),
    ("goose", include_str!("../../../../agents/goose.yaml")),
];
pub fn builtin(id: &str) -> Result<&'static str> {
    BUILTIN_AGENTS
        .iter()
        .find_map(|(name, yaml)| (*name == id).then_some(*yaml))
        .with_context(|| format!("unknown builtin agent: {id}"))
}
