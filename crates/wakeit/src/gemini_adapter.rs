//! Gemini CLI's documented stream-json format, verified against v0.64.0.
//! https://github.com/google-gemini/gemini-cli/blob/v0.64.0/packages/core/src/output/types.ts
//! An init ID does not establish that a native session was persisted.
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct GeminiNormalizer {
    tools: BTreeMap<String, TraceNode>,
    message: Option<TraceNode>,
    message_omitted: bool,
    next_message: usize,
    next_error: usize,
    completed: bool,
    fatal: bool,
    result: Option<Value>,
    unknown: BTreeSet<String>,
}

impl GeminiNormalizer {
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        if self.completed {
            return self.diagnostic("event after result");
        }
        if self.fatal && event["type"] != "result" {
            return self.diagnostic("event after fatal error");
        }
        let at = event["timestamp"]
            .as_str()
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);
        match event["type"].as_str().unwrap_or("") {
            "init" => vec![],
            "message" if event["role"] == "user" => vec![],
            "message" if event["role"] == "assistant" => {
                let Some(content) = event["content"].as_str() else {
                    return self.diagnostic("missing message content");
                };
                if self.message.is_none() {
                    self.message_omitted = false;
                    self.next_message += 1;
                    self.message = Some(TraceNode {
                        id: format!("gemini:message:{}", self.next_message),
                        parent_id: None,
                        kind: TraceKind::Message,
                        title: "Message".into(),
                        input: Value::Null,
                        output: json!(""),
                        status: TraceState::Running,
                        occurred_at: at,
                        started_at: Some(at),
                        finished_at: None,
                    });
                }
                if self.message_omitted {
                    return vec![];
                }
                let node = self.message.as_mut().unwrap();
                let mut text = if event["delta"] == true {
                    node.output.as_str().unwrap_or("").to_owned()
                } else {
                    String::new()
                };
                text.push_str(content);
                // Reserve envelope space and count JSON escaping, not only UTF-8 bytes.
                if serde_json::to_vec(&text).map_or(true, |bytes| bytes.len() > RECORD_LIMIT - 4096)
                {
                    self.message_omitted = true;
                    node.output = json!("[Response omitted: exceeds 1000 KiB]");
                    let record = StreamRecord::new(RecordData::Trace(node.clone()));
                    let mut records = self.diagnostic("response exceeds 1000 KiB");
                    records.push(record);
                    return records;
                }
                node.output = json!(text);
                node.occurred_at = at;
                vec![StreamRecord::new(RecordData::Trace(node.clone()))]
            }
            "tool_use" => {
                let Some(id) = tool_id(&event) else {
                    return self.diagnostic("invalid tool id");
                };
                let Some(name) = event["tool_name"].as_str() else {
                    return self.diagnostic("missing tool name");
                };
                // Native IDs identify a single call, including retransmitted events.
                if self.tools.contains_key(&id) {
                    return vec![];
                }
                let mut records = self.close_message(at, TraceState::Succeeded);
                // Text preceding a tool call is commentary, not the final answer.
                self.result = None;
                let node = TraceNode {
                    id: id.clone(),
                    parent_id: None,
                    kind: if name == "run_shell_command" {
                        TraceKind::Command
                    } else {
                        TraceKind::Tool
                    },
                    title: name.chars().take(512).collect(),
                    input: event["parameters"].clone(),
                    output: Value::Null,
                    status: TraceState::Running,
                    occurred_at: at,
                    started_at: Some(at),
                    finished_at: None,
                };
                self.tools.insert(id, node.clone());
                records.push(StreamRecord::new(RecordData::Trace(node)));
                records
            }
            "tool_result" => {
                let Some(id) = tool_id(&event) else {
                    return self.diagnostic("invalid tool result id");
                };
                let Some(node) = self.tools.get_mut(&id) else {
                    return self.diagnostic("unmatched tool result");
                };
                if node.finished_at.is_some() {
                    return vec![];
                }
                node.output = event
                    .get("output")
                    .filter(|v| !v.is_null())
                    .or(event.get("error"))
                    .cloned()
                    .unwrap_or(Value::Null);
                node.status = if event["status"] == "success" {
                    TraceState::Succeeded
                } else {
                    TraceState::Failed
                };
                node.occurred_at = at;
                node.finished_at = Some(at);
                vec![StreamRecord::new(RecordData::Trace(node.clone()))]
            }
            "error" if event["severity"] == "warning" => {
                vec![StreamRecord::new(RecordData::Log(LogRecord {
                    text: format!("{}\n", event["message"].as_str().unwrap_or("Agent warning")),
                    stream: "stderr".into(),
                    diagnostic: false,
                }))]
            }
            "error" => self.error(&event, at),
            "result" => {
                self.completed = true;
                if event["status"] != "success" {
                    let mut records = self.close_message(at, TraceState::Failed);
                    records.extend(self.error(&event, at));
                    return records;
                }
                let mut records = self.close_message(at, TraceState::Succeeded);
                if self.fatal {
                    self.result = None;
                    for record in &mut records {
                        if let RecordData::Trace(node) = &mut record.record {
                            node.status = TraceState::Failed;
                        }
                    }
                }
                records
            }
            typ => self.diagnostic(typ),
        }
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
            .map(|result| vec![StreamRecord::new(RecordData::Result(result))])
            .unwrap_or_default()
    }

    fn close_message(&mut self, at: DateTime<Utc>, state: TraceState) -> Vec<StreamRecord> {
        let Some(mut node) = self.message.take() else {
            return vec![];
        };
        self.result = (!self.message_omitted).then(|| node.output.clone());
        node.status = state;
        node.occurred_at = at;
        node.finished_at = Some(at);
        vec![StreamRecord::new(RecordData::Trace(node))]
    }

    fn error(&mut self, event: &Value, at: DateTime<Utc>) -> Vec<StreamRecord> {
        let mut records = self.close_message(at, TraceState::Failed);
        self.fatal = true;
        self.result = None;
        self.next_error += 1;
        let message = event["message"]
            .as_str()
            .or(event["error"]["message"].as_str())
            .unwrap_or("Agent execution failed");
        records.extend([
            StreamRecord::new(RecordData::Trace(TraceNode {
                id: format!("gemini:error:{}", self.next_error),
                parent_id: None,
                kind: TraceKind::Message,
                title: "Error".into(),
                input: Value::Null,
                output: json!(message),
                status: TraceState::Failed,
                occurred_at: at,
                started_at: None,
                finished_at: Some(at),
            })),
            StreamRecord::new(RecordData::Log(LogRecord {
                text: format!("{message}\n"),
                stream: "stderr".into(),
                diagnostic: false,
            })),
        ]);
        records
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
        }))]
    }
}

fn tool_id(event: &Value) -> Option<String> {
    event["tool_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 240)
        .map(|id| format!("gemini:tool:{id}"))
}
