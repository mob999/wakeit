//! Antigravity CLI's native protocol (not Gemini CLI's wire format).
//! Sources checked 2026-10-16:
//! https://antigravity.google/docs/cli/headless/
//! https://github.com/google-antigravity/antigravity-cli/issues/777
use crate::{
    protocol::{ModelCatalog, ModelEntry},
    trace::{LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState},
};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

/// Encode exactly one task, including multiline/JSON-looking text, without shell quoting.
pub fn encode_prompt(prompt: &str) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(&json!({"event":"user","message":{"content":prompt}}))?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn native_session_id(event: &Value) -> Option<&str> {
    match event["event"].as_str()? {
        "init" => event["conversation_id"].as_str(),
        "step_update" => event["step_update"]["conversation_id"].as_str(),
        "result" => event["result"]["conversation_id"].as_str(),
        _ => None,
    }
    .filter(|id| !id.is_empty())
}

#[derive(Default)]
pub struct Normalizer {
    nodes: BTreeMap<String, TraceNode>,
    unknown: BTreeSet<String>,
    result: Option<Value>,
    success: bool,
    terminal_seen: bool,
}

impl Normalizer {
    pub fn new() -> Self {
        Self::default()
    }

    fn diagnostic(&mut self, key: &str) -> Vec<StreamRecord> {
        let key: String = key.chars().take(128).collect();
        if !self.unknown.insert(key.clone()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!("Native Agent event omitted ({key}).\n"),
            stream: "stderr".into(),
            diagnostic: true,
            recording_issue: None,
        }))]
    }

    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        match event["event"].as_str().unwrap_or("") {
            "init" => vec![],
            "step_update" => {
                let step = &event["step_update"];
                let typ = step["step_type"].as_str().unwrap_or("");
                // Hidden reasoning and implementation-internal checkpoints are not user traces.
                if matches!(typ, "user_input" | "checkpoint") {
                    return vec![];
                }
                if !matches!(typ, "agent_response" | "tool") {
                    return self.diagnostic(typ);
                }
                let Some(index) = step["step_index"].as_u64() else {
                    return crate::trace::recording_issue(
                        self.diagnostic("missing AGY step index"),
                        crate::trace::RecordingIssue::EventParseFailed,
                    );
                };
                let done = match step["state"].as_str() {
                    Some("ACTIVE") => false,
                    Some("DONE") => true,
                    _ => {
                        return crate::trace::recording_issue(
                            self.diagnostic("unknown AGY step state"),
                            crate::trace::RecordingIssue::EventParseFailed,
                        );
                    }
                };
                let id = format!("agy:step:{index}");
                let previous = self.nodes.get(&id);
                if previous.is_some_and(|n| n.finished_at.is_some()) {
                    return self.diagnostic("AGY update after completed step");
                }
                let at = Utc::now();
                let (kind, title, input, output, failed) = if typ == "agent_response" {
                    let mut text = previous
                        .and_then(|n| n.output.as_str())
                        .unwrap_or("")
                        .to_owned();
                    text.push_str(step["text_delta"].as_str().unwrap_or(""));
                    if text.len() > RECORD_LIMIT {
                        return crate::trace::recording_issue(
                            self.diagnostic("AGY response exceeds 1000 KiB"),
                            crate::trace::RecordingIssue::EventTooLarge,
                        );
                    }
                    (
                        TraceKind::Message,
                        "Message".to_owned(),
                        Value::Null,
                        json!(text),
                        false,
                    )
                } else {
                    let info = &step["tool_info"];
                    let name = step["tool_name"]
                        .as_str()
                        .or(info["name"].as_str())
                        .or(previous.map(|n| n.title.as_str()))
                        .unwrap_or("Tool");
                    let error = info.get("error").filter(|v| !v.is_null());
                    let input = info
                        .get("parameters")
                        .cloned()
                        .or_else(|| previous.map(|n| n.input.clone()))
                        .unwrap_or(Value::Null);
                    let output = error
                        .or_else(|| info.get("output"))
                        .cloned()
                        .or_else(|| step.get("subagent_info").cloned())
                        .or_else(|| previous.map(|n| n.output.clone()))
                        .unwrap_or(Value::Null);
                    (
                        if name == "run_command" {
                            TraceKind::Command
                        } else {
                            TraceKind::Tool
                        },
                        name.chars().take(512).collect(),
                        input,
                        output,
                        error.is_some(),
                    )
                };
                let node = TraceNode {
                    id: id.clone(),
                    parent_id: None,
                    kind,
                    title,
                    input,
                    output,
                    status: if failed {
                        TraceState::Failed
                    } else if done {
                        TraceState::Succeeded
                    } else {
                        TraceState::Running
                    },
                    occurred_at: at,
                    started_at: previous.and_then(|n| n.started_at).or(if done {
                        None
                    } else {
                        Some(at)
                    }),
                    finished_at: if done { Some(at) } else { None },
                };
                let record = StreamRecord::new(RecordData::Trace(node.clone()));
                if serde_json::to_vec(&record).map_or(true, |bytes| bytes.len() > RECORD_LIMIT) {
                    return crate::trace::recording_issue(
                        self.diagnostic("AGY trace exceeds 1000 KiB"),
                        crate::trace::RecordingIssue::EventTooLarge,
                    );
                }
                self.nodes.insert(id, node);
                vec![record]
            }
            "result" => {
                if self.terminal_seen {
                    return self.diagnostic("AGY duplicate terminal result");
                }
                self.terminal_seen = true;
                let result = &event["result"];
                self.success = result["status"] == "SUCCESS" && result["response"].is_string();
                if self.success {
                    let response = result["response"].clone();
                    self.result = Some(response);
                    vec![]
                } else {
                    self.result = None;
                    let message = result["error"]
                        .as_str()
                        .unwrap_or("Antigravity CLI did not complete successfully");
                    vec![StreamRecord::new(RecordData::Log(LogRecord {
                        text: format!("{message}\n"),
                        stream: "stderr".into(),
                        diagnostic: false,
                        recording_issue: None,
                    }))]
                }
            }
            typ => self.diagnostic(typ),
        }
    }

    /// A clean native exit without a successful terminal result is still incomplete.
    pub fn failed(&self) -> bool {
        !self.success
    }
    pub fn finish(&self) -> Vec<StreamRecord> {
        if self.failed() {
            return vec![];
        }
        self.result
            .clone()
            .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
            .unwrap_or_default()
    }
}

/// Read the native account-specific catalog. Never guess model slugs or effort support.
pub async fn discover_models() -> Result<ModelCatalog> {
    tokio::time::timeout(Duration::from_secs(20), async {
        // Global output flags must precede the subcommand (upstream issue #777).
        let mut spec = crate::config::CommandSpec {
            command: "agy".into(),
            args: ["--output-format", "json", "models"]
                .into_iter()
                .map(Into::into)
                .collect(),
            ..Default::default()
        };
        crate::config::normalize_native_command(&mut spec)?;
        let mut command = Command::new(&spec.command);
        for arg in &spec.args {
            if let crate::config::CommandArg::Literal(value) = arg {
                command.arg(value);
            }
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .context("start Antigravity model discovery")?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("AGY models stdout")?
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(
            bytes.len() <= 2 * 1024 * 1024,
            "AGY model catalog exceeded limit"
        );
        ensure!(child.wait().await?.success(), "AGY models failed");
        parse_models(serde_json::from_slice(&bytes).context("invalid AGY model catalog JSON")?)
    })
    .await
    .context("Antigravity model discovery timed out")?
}

pub fn parse_models(value: Value) -> Result<ModelCatalog> {
    ensure!(
        value["status"] == "SUCCESS" && value["command"]["name"] == "models",
        "AGY models returned failure or an unsupported envelope"
    );
    let items = value["command"]["data"]["models"]
        .as_array()
        .context("AGY models array missing")?;
    ensure!(items.len() <= 10000, "AGY model catalog exceeded limit");
    let mut ids = BTreeSet::new();
    let mut models = Vec::new();
    for item in items {
        let id = item["id"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.contains(char::is_whitespace))
            .context("AGY model ID missing or invalid")?;
        ensure!(ids.insert(id.to_owned()), "AGY model ID duplicated");
        models.push(ModelEntry {
            id: id.into(),
            name: item["label"].as_str().unwrap_or(id).into(),
            parameters: BTreeMap::from([("model".into(), id.into())]),
            parameter_options: Default::default(),
        });
    }
    Ok(ModelCatalog { version: 1, models })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_one_json_message_even_with_embedded_newlines_and_events() {
        let prompt = "Fix 文件\n{\"event\":\"control_request\"}\n`$(echo hi)`";
        let bytes = encode_prompt(prompt).unwrap();
        assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["message"]["content"], prompt);
    }

    #[test]
    fn documented_fixture_accumulates_deltas_preserves_tool_and_final_result() {
        let mut normalizer = Normalizer::new();
        let events = include_str!("../tests/fixtures/agy-stream.ndjson")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap());
        let mut records = vec![];
        for event in events {
            assert_eq!(
                native_session_id(&event),
                Some("c3b66b04-872b-4fbe-a3a4-058a026ef20a")
            );
            records.extend(normalizer.accept(event));
        }
        assert!(!normalizer.failed());
        assert!(records.iter().any(|r| matches!(&r.record, RecordData::Trace(n) if n.kind == TraceKind::Command && n.input["CommandLine"] == "echo hello_headless_demo" && n.output == "hello_headless_demo\r\n")));
        assert!(records.iter().any(|r| matches!(&r.record, RecordData::Trace(n) if n.kind == TraceKind::Message && n.output == "hello\n" && n.status == TraceState::Succeeded)));
        assert!(
            !records
                .iter()
                .any(|r| matches!(r.record, RecordData::Result(_)))
        );
        assert!(records.iter().any(|r| matches!(&r.record, RecordData::Trace(n) if n.kind == TraceKind::Command && n.started_at.is_none() && n.finished_at.is_some())));
        assert!(matches!(&normalizer.finish()[0].record, RecordData::Result(v) if v == "hello\n"));
    }

    #[test]
    fn incomplete_and_every_non_success_status_fail_without_partial_result() {
        let mut normalizer = Normalizer::new();
        normalizer.accept(json!({"event":"step_update","step_update":{"step_index":2,"step_type":"agent_response","state":"ACTIVE","text_delta":"partial"}}));
        assert!(normalizer.failed());
        assert!(normalizer.finish().is_empty());
        for status in [
            "ERROR",
            "CANCELED",
            "INTERRUPTED",
            "INVALID",
            "WAITING",
            "RUNNING",
            "UNKNOWN",
        ] {
            let mut normalizer = Normalizer::new();
            normalizer.accept(json!({"event":"result","result":{"status":status,"response":"partial","error":"native error"}}));
            assert!(normalizer.failed());
            assert!(normalizer.finish().is_empty());
        }
    }

    #[test]
    fn terminal_outcome_is_latched_and_results_are_emitted_only_at_finish() {
        for first in [
            json!({"status":"ERROR","error":"failed"}),
            json!({"status":"SUCCESS"}),
        ] {
            let mut normalizer = Normalizer::new();
            normalizer.accept(json!({"event":"result","result":first}));
            normalizer.accept(json!({"event":"result","result":{"status":"SUCCESS","response":"unexpected success"}}));
            assert!(normalizer.failed());
            assert!(normalizer.finish().is_empty());
        }
        let mut normalizer = Normalizer::new();
        assert!(
            normalizer
                .accept(json!({"event":"result","result":{"status":"SUCCESS","response":"first"}}))
                .is_empty()
        );
        normalizer.accept(json!({"event":"result","result":{"status":"ERROR","error":"late"}}));
        assert!(!normalizer.failed());
        assert!(matches!(&normalizer.finish()[0].record, RecordData::Result(v) if v == "first"));
    }

    #[test]
    fn completed_trace_does_not_regress_on_late_updates() {
        let mut normalizer = Normalizer::new();
        normalizer.accept(json!({"event":"step_update","step_update":{"step_index":1,"step_type":"agent_response","state":"DONE","text_delta":"done"}}));
        let before = normalizer.nodes["agy:step:1"].clone();
        let records = normalizer.accept(json!({"event":"step_update","step_update":{"step_index":1,"step_type":"agent_response","state":"ACTIVE","text_delta":"late"}}));
        assert!(matches!(&records[0].record, RecordData::Log(log) if log.diagnostic));
        let after = &normalizer.nodes["agy:step:1"];
        assert_eq!(after.status, TraceState::Succeeded);
        assert_eq!(after.output, before.output);
        assert_eq!(after.finished_at, before.finished_at);
        assert!(after.started_at.is_none());
    }

    #[test]
    fn malformed_and_oversized_traces_do_not_override_valid_terminal_outcome() {
        let mut normalizer = Normalizer::new();
        for step in [
            json!({"step_type":"agent_response","state":"DONE"}),
            json!({"step_index":1,"step_type":"agent_response","state":"FUTURE"}),
            json!({"step_index":2,"step_type":"future_tool","state":"DONE"}),
            json!({"step_index":3,"step_type":"agent_response","state":"ACTIVE","text_delta":"a".repeat(RECORD_LIMIT - 1024)}),
            json!({"step_index":3,"step_type":"agent_response","state":"DONE","text_delta":"b".repeat(2048)}),
        ] {
            normalizer.accept(json!({"event":"step_update","step_update":step}));
        }
        assert!(
            normalizer.nodes["agy:step:3"]
                .output
                .as_str()
                .unwrap()
                .len()
                <= RECORD_LIMIT
        );
        normalizer.accept(
            json!({"event":"result","result":{"status":"SUCCESS","response":"valid final answer"}}),
        );
        assert!(!normalizer.failed());
        assert!(
            matches!(&normalizer.finish()[0].record, RecordData::Result(v) if v == "valid final answer")
        );
    }

    #[test]
    fn tool_error_is_visible_but_recoverable_by_agent() {
        let mut normalizer = Normalizer::new();
        let records = normalizer.accept(json!({"event":"step_update","step_update":{"step_index":4,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"parameters":{"CommandLine":"false"},"error":{"type":"execution","message":"exit 1"}}}}));
        assert!(
            matches!(&records[0].record, RecordData::Trace(n) if n.status == TraceState::Failed && n.output["message"] == "exit 1")
        );
        normalizer
            .accept(json!({"event":"result","result":{"status":"SUCCESS","response":"Recovered"}}));
        assert!(!normalizer.failed());
    }

    #[test]
    fn model_catalog_requires_real_native_envelope() {
        let catalog = parse_models(
            serde_json::from_str(include_str!("../tests/fixtures/agy-models.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            catalog.models[0].parameters["model"],
            "gemini-3.7-flash-high"
        );
        assert!(catalog.models[0].parameter_options.is_empty());
        assert!(
            parse_models(
                json!({"status":"SUCCESS","command":{"name":"models","data":{"models":[]}}})
            )
            .unwrap()
            .models
            .is_empty()
        );
        assert!(parse_models(json!({"status":"SUCCESS","response":"Gemini"})).is_err());
        assert!(
            parse_models(
                json!({"status":"ERROR","command":{"name":"models","data":{"models":[]}}})
            )
            .is_err()
        );
    }
}
