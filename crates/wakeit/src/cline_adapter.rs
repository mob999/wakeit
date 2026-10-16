//! Cline's current CLI JSONL contract, not the legacy VS Code extension wire format.
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct ClineNormalizer {
    tools: BTreeMap<String, TraceNode>,
    active_messages: BTreeMap<String, TraceNode>,
    omitted_messages: BTreeSet<String>,
    next_message: usize,
    retained_bytes: usize,
    result: Option<Value>,
    completed: bool,
    fatal: bool,
    diagnostics: BTreeSet<String>,
}

impl ClineNormalizer {
    fn diagnostic(&mut self, key: &str) -> Vec<StreamRecord> {
        if self.diagnostics.len() >= 128 || !self.diagnostics.insert(key.into()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!("Cline event omitted ({key}).\n"),
            stream: "stderr".into(),
            diagnostic: true,
        }))]
    }
    fn emit(&mut self, node: TraceNode) -> Vec<StreamRecord> {
        let record = StreamRecord::new(RecordData::Trace(node));
        if serde_json::to_vec(&record).map_or(true, |v| v.len() > RECORD_LIMIT) {
            return self.diagnostic("trace exceeds record limit");
        }
        vec![record]
    }
    pub fn accept(&mut self, value: Value) -> Vec<StreamRecord> {
        let typ = value["type"].as_str().unwrap_or("");
        if matches!(typ, "run_aborted" | "error") {
            self.fatal = true;
            self.result = None;
            return self.diagnostic("run failed or aborted");
        }
        if self.completed {
            return vec![];
        }
        if typ == "run_result" {
            self.completed = true;
            if value["finishReason"] != "completed" || !value["text"].is_string() {
                self.fatal = true;
                return self.diagnostic("unsuccessful run result");
            }
            let text = value["text"].as_str().unwrap();
            if serde_json::to_vec(&StreamRecord::new(RecordData::Result(json!(text))))
                .map_or(true, |v| v.len() > RECORD_LIMIT)
            {
                return self.diagnostic("result exceeds record limit");
            }
            self.result = Some(json!(text));
            return vec![];
        }
        if matches!(
            typ,
            "hook_event" | "team_event" | "run_start" | "run_abort_requested"
        ) {
            return vec![];
        }
        if typ != "agent_event" {
            return self.diagnostic("unsupported envelope");
        }
        let event = &value["event"];
        let typ = event["type"].as_str().unwrap_or("");
        if typ == "error" {
            // Recoverable native errors may be handled by the Agent itself.
            if event["recoverable"] != true {
                self.fatal = true;
                self.result = None;
            }
            return self.diagnostic("Agent error");
        }
        if matches!(
            typ,
            "iteration_start" | "iteration_end" | "usage" | "notice" | "done"
        ) {
            return vec![];
        }
        if !matches!(typ, "content_start" | "content_update" | "content_end") {
            return self.diagnostic("unsupported Agent event");
        }
        let content = event["contentType"].as_str().unwrap_or("");
        if matches!(content, "reasoning" | "media") {
            return vec![];
        }
        let at = value["ts"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        if content == "text" {
            let agent = event["agentId"].as_str().unwrap_or("main");
            if agent.len() > 256 {
                return self.diagnostic("invalid Agent ID");
            }
            if self.omitted_messages.contains(agent) {
                if typ == "content_end" {
                    self.omitted_messages.remove(agent);
                }
                return vec![];
            }
            let previous = self.active_messages.remove(agent);
            if typ == "content_update" {
                return self.diagnostic("unexpected text update");
            }
            let mut node = previous.unwrap_or_else(|| {
                self.next_message += 1;
                TraceNode {
                    id: format!("cline-message:{}", self.next_message),
                    parent_id: None,
                    kind: TraceKind::Message,
                    title: "Message".into(),
                    input: Value::Null,
                    output: json!(""),
                    status: TraceState::Running,
                    occurred_at: at,
                    started_at: None,
                    finished_at: None,
                }
            });
            let text = if typ == "content_start" {
                event["accumulated"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        format!(
                            "{}{}",
                            node.output.as_str().unwrap_or(""),
                            event["text"].as_str().unwrap_or("")
                        )
                    })
            } else {
                event["text"].as_str().unwrap_or("").to_owned()
            };
            if text.len() > RECORD_LIMIT / 8 {
                if typ != "content_end" && self.omitted_messages.len() < 128 {
                    self.omitted_messages.insert(agent.into());
                }
                return self.diagnostic("message exceeds limit");
            }
            node.output = json!(text);
            node.occurred_at = at;
            if typ == "content_end" {
                node.status = TraceState::Succeeded;
                node.finished_at = Some(at);
            } else if self.active_messages.len() < 64 {
                self.active_messages.insert(agent.into(), node.clone());
            } else {
                return self.diagnostic("too many simultaneous messages");
            }
            return self.emit(node);
        }
        if content != "tool" {
            return self.diagnostic("unsupported content type");
        }
        let Some(id) = event["toolCallId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 200)
        else {
            return self.diagnostic("missing tool call ID");
        };
        let mut node = if let Some(existing) = self.tools.get(id) {
            if existing.finished_at.is_some() {
                return vec![];
            }
            if typ == "content_start" {
                return vec![];
            }
            existing.clone()
        } else {
            let size = serde_json::to_vec(event).map_or(RECORD_LIMIT, |v| v.len());
            if self.tools.len() >= 10_000 || self.retained_bytes + size > 8 * 1024 * 1024 {
                return self.diagnostic("tool history exceeds limit");
            }
            self.retained_bytes += size;
            let name = event["toolName"].as_str().unwrap_or("Tool");
            TraceNode {
                id: format!("cline-tool:{id}"),
                parent_id: None,
                kind: if name == "run_commands" {
                    TraceKind::Command
                } else {
                    TraceKind::Tool
                },
                title: name.chars().take(512).collect(),
                input: event["input"].clone(),
                output: Value::Null,
                status: TraceState::Running,
                occurred_at: at,
                started_at: (typ == "content_start").then_some(at),
                finished_at: None,
            }
        };
        node.occurred_at = at;
        if typ == "content_end" {
            let failed = event["error"].as_str().is_some_and(|s| !s.is_empty());
            node.status = if failed {
                TraceState::Failed
            } else {
                TraceState::Succeeded
            };
            node.output = if failed {
                event["error"].clone()
            } else {
                event["output"].clone()
            };
            node.finished_at = Some(at);
        } else if typ == "content_update" {
            node.output = event["update"].clone();
        }
        // Retain inputs and timestamps, never retain potentially huge tool outputs.
        let mut retained = node.clone();
        retained.output = Value::Null;
        self.tools.insert(id.into(), retained);
        self.emit(node)
    }
    pub fn failed(&self) -> bool {
        self.fatal || !self.completed
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
