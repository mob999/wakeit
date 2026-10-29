use super::{duration, validate_id, version};
use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn yes() -> bool {
    true
}
fn timeout() -> String {
    "1h".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionBinding {
    pub id: String,
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
    #[serde(default)]
    pub timeout: Option<String>,
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
            timeout: None,
            start_grace: timeout(),
            overlap: Overlap::Skip,
            misfire: Misfire::Latest,
        }
    }
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
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
// Normalize ordinary tasks at the input boundary; DAG omission means no total cap.
#[derive(Deserialize)]
#[serde(remote = "TaskConfig", deny_unknown_fields)]
struct TaskConfigWire {
    #[serde(default = "version")]
    version: u32,
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    runner: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    agent: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    cwd: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    prompt: String,
    #[serde(default)]
    parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<SessionBinding>,
    schedule: Option<Schedule>,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    execution: Execution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dag: Option<crate::dag::Dag>,
}
impl<'de> Deserialize<'de> for TaskConfig {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let mut config = TaskConfigWire::deserialize(deserializer)?;
        if config.dag.is_none() && config.execution.timeout.is_none() {
            config.execution.timeout = Some(timeout());
        }
        Ok(config)
    }
}

fn task_schema(schema: &mut schemars::Schema) {
    schema.insert("oneOf".into(), serde_json::json!([
        {"required":["runner","agent","cwd","prompt"],"properties":{"dag":{"type":"null"},"runner":{"minLength":1},"agent":{"minLength":1},"cwd":{"minLength":1},"prompt":{"minLength":1}}},
        {"required":["dag"],"properties":{"dag":{"type":"object"},"runner":{"maxLength":0},"agent":{"maxLength":0},"cwd":{"maxLength":0},"prompt":{"maxLength":0},"parameters":{"maxProperties":0},"session":{"type":"null"}}}
    ]));
}

impl TaskConfig {
    pub fn issues(&self) -> Vec<crate::dag::ValidationIssue> {
        let mut issues = self.validate_fields().err().into_iter().collect::<Vec<_>>();
        if let Some(dag) = &self.dag {
            issues.extend(dag.issues());
        }
        issues
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(issue) = self.issues().first() {
            if issue.path.starts_with("dag.") {
                bail!("{}: {}", issue.path, issue.message);
            }
            bail!("{}", issue.message);
        }
        Ok(())
    }

    fn validate_fields(&self) -> std::result::Result<(), crate::dag::ValidationIssue> {
        let issue = |path: &str, message: String| crate::dag::ValidationIssue {
            path: path.into(),
            node_id: None,
            message,
            cycle: vec![],
        };
        if self.version != 1 {
            return Err(issue(
                "version",
                "unsupported task configuration version".into(),
            ));
        }
        validate_id(&self.id).map_err(|e| issue("id", e.to_string()))?;
        if self.name.trim().is_empty() {
            return Err(issue(
                "name",
                if self.dag.is_some() {
                    "name is required"
                } else {
                    "name and runner are required"
                }
                .into(),
            ));
        }
        if self.dag.is_some() {
            if !self.runner.is_empty()
                || !self.agent.is_empty()
                || !self.cwd.is_empty()
                || !self.prompt.is_empty()
                || !self.parameters.is_empty()
                || self.session.is_some()
            {
                return Err(issue(
                    "dag",
                    "DAG and top-level node execution fields are mutually exclusive".into(),
                ));
            }
        } else {
            validate_id(&self.agent).map_err(|e| issue("agent", e.to_string()))?;
            if self.runner.is_empty() {
                return Err(issue("runner", "name and runner are required".into()));
            }
            if self.cwd.trim().is_empty() {
                return Err(issue("cwd", "cwd is required".into()));
            }
            if self.prompt.trim().is_empty() {
                return Err(issue("prompt", "prompt is required".into()));
            }
            if self.prompt.len() > 1024 * 1024 {
                return Err(issue("prompt", "prompt exceeds 1 MiB".into()));
            }
            if let Some(session) = &self.session {
                crate::sessions::validate_id(&session.id)
                    .map_err(|e| issue("session.id", e.to_string()))?;
            }
        }
        if serde_json::to_vec(self)
            .map_err(|e| issue("config", e.to_string()))?
            .len()
            > 1000 * 1024
        {
            return Err(issue(
                "config",
                "encoded task configuration exceeds 1000 KiB".into(),
            ));
        }
        if let Some(timeout) = &self.execution.timeout {
            duration(timeout).map_err(|e| issue("execution.timeout", e.to_string()))?;
        }
        duration(&self.execution.start_grace)
            .map_err(|e| issue("execution.start_grace", e.to_string()))?;
        if let Some(schedule) = &self.schedule {
            crate::schedule::validate(schedule).map_err(|e| issue("schedule", e.to_string()))?;
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_issues_identify_the_invalid_field_without_reparsing() {
        let task = json!({
            "id": "daily", "name": "Daily", "runner": "device", "agent": "pi",
            "cwd": "/work", "prompt": "Summarize", "schedule": null
        });
        for (path, value) in [
            ("/execution/timeout", json!("invalid")),
            ("/execution/start_grace", json!("invalid")),
            ("/schedule", json!({"cron": "invalid", "timezone": "UTC"})),
            ("/prompt", json!("")),
        ] {
            let mut input = task.clone();
            input["execution"] = json!({"timeout": "1h", "start_grace": "1m"});
            *input.pointer_mut(path).unwrap() = value;
            let config: TaskConfig = serde_json::from_value(input).unwrap();
            let issues = config.issues();
            assert_eq!(issues.len(), 1, "{path}: {issues:?}");
            assert_eq!(
                issues[0].path,
                path.trim_start_matches('/').replace('/', ".")
            );
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn task_issues_preserve_graph_details_alongside_execution_errors() {
        let config: TaskConfig = serde_json::from_value(json!({
            "id": "daily", "name": "Daily", "schedule": null,
            "execution": {"timeout": "invalid"},
            "dag": {"nodes": [
                {"id": "a", "name": "A", "pre": ["b"], "runner": "device", "agent": "pi", "cwd": "/work", "prompt": "Collect"},
                {"id": "b", "name": "B", "pre": ["a"], "runner": "device", "agent": "pi", "cwd": "/work", "prompt": "Summarize"}
            ]}
        })).unwrap();
        let issues = config.issues();
        assert_eq!(issues[0].path, "execution.timeout");
        assert!(issues.iter().any(|issue| !issue.cycle.is_empty()));
        assert_eq!(
            issues.len(),
            config.dag.as_ref().unwrap().issues().len() + 1
        );
    }
}
