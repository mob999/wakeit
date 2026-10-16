//! GitHub Copilot CLI 1.0.x native --output-format json, not an SDK transport.
use crate::trace::{LogRecord, RecordData, StreamRecord, TraceKind, TraceNode, TraceState};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
const LIMIT: usize = 256 * 1024;
#[derive(Default)]
pub struct CopilotNormalizer {
    nodes: BTreeMap<String, TraceNode>,
    seen: BTreeSet<String>,
    unknown: BTreeSet<String>,
    result: Option<Value>,
    completed: bool,
    invalid: bool,
    retained: usize,
}
impl CopilotNormalizer {
    fn diagnostic(&mut self, kind: &str) -> Vec<StreamRecord> {
        let kind: String = kind.chars().take(100).collect();
        if self.unknown.len() >= 64 || !self.unknown.insert(kind.clone()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!("Copilot event omitted ({kind}).\n"),
            stream: "stderr".into(),
            diagnostic: true,
        }))]
    }
    fn malformed(&mut self, kind: &str) -> Vec<StreamRecord> {
        self.invalid = true;
        self.result = None;
        self.diagnostic(kind)
    }
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        if self.completed {
            return vec![];
        }
        if let Some(id) = event["id"].as_str() {
            if self.seen.len() >= 100_000 {
                return self.malformed("event count exceeded limit");
            };
            if !self.seen.insert(id.into()) {
                return vec![];
            }
        }
        let typ = event["type"].as_str().unwrap_or("");
        let data = &event["data"];
        let time = event["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok());
        let at = time.unwrap_or_else(Utc::now);
        match typ {
            "assistant.message_start" | "assistant.message_delta" | "assistant.message" => {
                let Some(raw_id) = data["messageId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 200)
                else {
                    return self.malformed("message ID missing");
                };
                let id = format!("copilot-message:{raw_id}");
                if self
                    .nodes
                    .get(&id)
                    .is_some_and(|n| n.status != TraceState::Running)
                {
                    return vec![];
                }
                if self.nodes.len() >= 10_000 {
                    return self.malformed("node count exceeded limit");
                }
                let node = self.nodes.entry(id.clone()).or_insert_with(|| TraceNode {
                    id,
                    parent_id: None,
                    kind: TraceKind::Message,
                    status: TraceState::Running,
                    title: "Message".into(),
                    input: Value::Null,
                    output: json!(""),
                    occurred_at: at,
                    started_at: if typ == "assistant.message" {
                        None
                    } else {
                        time
                    },
                    finished_at: None,
                });
                if typ == "assistant.message_delta" {
                    let Some(delta) = data["deltaContent"].as_str() else {
                        return self.malformed("message delta missing");
                    };
                    let before = node.output.as_str().unwrap_or_default();
                    if before.len() + delta.len() > LIMIT {
                        return self.malformed("message exceeded limit");
                    };
                    node.output = json!(format!("{before}{delta}"));
                } else if typ == "assistant.message" {
                    let Some(text) = data["content"].as_str() else {
                        return self.malformed("public message missing");
                    };
                    if text.len() > LIMIT {
                        return self.malformed("message exceeded limit");
                    };
                    node.output = json!(text);
                    node.status = TraceState::Succeeded;
                    node.finished_at = time;
                    self.result = if data["toolRequests"]
                        .as_array()
                        .is_some_and(|v| !v.is_empty())
                    {
                        None
                    } else {
                        Some(json!(text))
                    };
                }
                node.occurred_at = at;
                let record = StreamRecord::new(RecordData::Trace(node.clone()));
                if typ == "assistant.message" {
                    node.output = Value::Null;
                }
                vec![record]
            }
            "tool.execution_start" => {
                self.result = None;
                let Some(raw) = data["toolCallId"]
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 200)
                else {
                    return self.malformed("tool ID missing");
                };
                let id = format!("copilot-tool:{raw}");
                if self.nodes.contains_key(&id) {
                    return vec![];
                }
                let Some(name) = data["toolName"].as_str() else {
                    return self.malformed("tool name missing");
                };
                let size = serde_json::to_vec(&data["arguments"]).map_or(LIMIT, |v| v.len());
                if size >= LIMIT
                    || self.retained + size > 8 * 1024 * 1024
                    || self.nodes.len() >= 10_000
                {
                    return self.malformed("tool input exceeded limit");
                };
                self.retained += size;
                let node = TraceNode {
                    id: id.clone(),
                    parent_id: None,
                    kind: if matches!(
                        name.to_ascii_lowercase().as_str(),
                        "bash" | "shell" | "powershell" | "terminal"
                    ) {
                        TraceKind::Command
                    } else {
                        TraceKind::Tool
                    },
                    status: TraceState::Running,
                    title: name.chars().take(512).collect(),
                    input: data["arguments"].clone(),
                    output: Value::Null,
                    occurred_at: at,
                    started_at: time,
                    finished_at: None,
                };
                self.nodes.insert(id, node.clone());
                vec![StreamRecord::new(RecordData::Trace(node))]
            }
            "tool.execution_complete" => {
                let Some(raw) = data["toolCallId"].as_str() else {
                    return self.malformed("tool result ID missing");
                };
                let Some(success) = data["success"].as_bool() else {
                    return self.malformed("tool result status missing");
                };
                let id = format!("copilot-tool:{raw}");
                let Some(node) = self.nodes.get_mut(&id) else {
                    return self.malformed("tool result without start");
                };
                if node.status != TraceState::Running {
                    return vec![];
                }
                let result = if success {
                    data["result"].clone()
                } else {
                    data["error"].clone()
                };
                if serde_json::to_vec(&result).map_or(LIMIT, |v| v.len()) >= LIMIT {
                    return self.malformed("tool output exceeded limit");
                };
                node.status = if success {
                    TraceState::Succeeded
                } else {
                    TraceState::Failed
                };
                node.output = result;
                node.occurred_at = at;
                node.finished_at = time;
                let record = StreamRecord::new(RecordData::Trace(node.clone()));
                self.retained = self
                    .retained
                    .saturating_sub(serde_json::to_vec(&node.input).map_or(0, |v| v.len()));
                node.input = Value::Null;
                node.output = Value::Null;
                vec![record]
            }
            "session.error" => {
                self.invalid = true;
                self.result = None;
                self.diagnostic("native session error")
            }
            "result" => {
                self.completed = true;
                if event["exitCode"] != 0 {
                    self.invalid = true;
                    self.result = None;
                    self.diagnostic("unsuccessful native result")
                } else {
                    vec![]
                }
            }
            // Only the public content fields above are retained. In particular,
            // reasoningText/reasoningOpaque/encryptedContent never enter the trace.
            _ => vec![],
        }
    }
    pub fn failed(&self) -> bool {
        self.invalid
            || !self.completed
            || self.nodes.values().any(|n| n.status == TraceState::Running)
    }
    pub fn finish(&self) -> Vec<StreamRecord> {
        if self.failed() {
            return vec![];
        };
        self.result
            .clone()
            .filter(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
            .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
            .unwrap_or_default()
    }
}
