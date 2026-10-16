//! AAIF/Block Goose `run --output-format stream-json`, verified against v1.54.0.
//! Native messages contain incremental text and correlated tool requests/results.
//! https://github.com/aaif-goose/goose/blob/v1.54.0/crates/goose-cli/src/session/mod.rs
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct GooseNormalizer {
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

impl GooseNormalizer {
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        if self.completed || self.fatal {
            return self.diagnostic("event after terminal outcome");
        }
        match event["type"].as_str().unwrap_or("") {
            "message" => self.message(&event["message"]),
            "notification" => vec![],
            "error" => self.error(
                event["error"].as_str().unwrap_or("Agent execution failed"),
                Utc::now(),
            ),
            "complete" => {
                self.completed = true;
                self.close_message(Utc::now(), TraceState::Succeeded)
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

    fn message(&mut self, message: &Value) -> Vec<StreamRecord> {
        if message["metadata"]["userVisible"] == false {
            return vec![];
        }
        let at = message["created"]
            .as_i64()
            .and_then(|seconds| DateTime::from_timestamp(seconds, 0))
            .unwrap_or_else(Utc::now);
        if message["metadata"]["outputTokenLimitReached"] == true {
            return self.error("Agent response reached the output token limit", at);
        }
        let Some(content) = message["content"].as_array() else {
            return self.diagnostic("missing message content");
        };
        let mut records = vec![];
        for part in content {
            if self.fatal {
                break;
            }
            if !user_visible(part) {
                continue;
            }
            match part["type"].as_str().unwrap_or("") {
                "text" if message["role"] == "assistant" => {
                    if let Some(text) = part["text"].as_str() {
                        records.extend(self.text(text, at));
                    }
                }
                "toolRequest" => records.extend(self.tool_request(part, at)),
                "toolResponse" => records.extend(self.tool_response(part, at)),
                "error" => records.extend(self.error(
                    part["message"].as_str().unwrap_or("Agent execution failed"),
                    at,
                )),
                // Thoughts, hidden blocks, prompts, and notifications are not assistant answers.
                "text" | "thinking" | "redactedThinking" | "systemNotification" | "image"
                | "document" => {}
                typ => records.extend(self.diagnostic(typ)),
            }
        }
        records
    }

    fn text(&mut self, content: &str, at: DateTime<Utc>) -> Vec<StreamRecord> {
        if self.message.is_none() {
            self.message_omitted = false;
            self.next_message += 1;
            self.message = Some(TraceNode {
                id: format!("goose:message:{}", self.next_message),
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
        let mut text = node.output.as_str().unwrap_or("").to_owned();
        text.push_str(content);
        if serde_json::to_vec(&text).map_or(true, |bytes| bytes.len() > RECORD_LIMIT - 4096) {
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

    fn tool_request(&mut self, part: &Value, at: DateTime<Utc>) -> Vec<StreamRecord> {
        let Some(id) = tool_id(part) else {
            return self.diagnostic("invalid tool id");
        };
        if self.tools.contains_key(&id) {
            return vec![];
        }
        let call = &part["toolCall"];
        if call["status"] != "success" && call["status"] != "error" {
            return self.diagnostic("invalid tool call status");
        }
        let name = call["value"]["name"].as_str().unwrap_or("Tool");
        let mut records = self.close_message(at, TraceState::Succeeded);
        self.result = None;
        let failed = call["status"] == "error";
        let node = TraceNode {
            id: id.clone(),
            parent_id: None,
            kind: if name == "shell" || name.ends_with("__shell") {
                TraceKind::Command
            } else {
                TraceKind::Tool
            },
            title: name.chars().take(512).collect(),
            input: call["value"]["arguments"].clone(),
            output: call["error"].clone(),
            status: if failed {
                TraceState::Failed
            } else {
                TraceState::Running
            },
            occurred_at: at,
            started_at: Some(at),
            finished_at: failed.then_some(at),
        };
        self.tools.insert(id, node.clone());
        records.push(StreamRecord::new(RecordData::Trace(node)));
        records
    }

    fn tool_response(&mut self, part: &Value, at: DateTime<Utc>) -> Vec<StreamRecord> {
        let Some(id) = tool_id(part) else {
            return self.diagnostic("invalid tool result id");
        };
        let Some(node) = self.tools.get_mut(&id) else {
            return self.diagnostic("unmatched tool result");
        };
        if node.finished_at.is_some() {
            return vec![];
        }
        let result = &part["toolResult"];
        let failed = result["status"] != "success" || result["value"]["isError"] == true;
        node.output = if result["status"] == "error" {
            result["error"].clone()
        } else {
            let mut value = result["value"].clone();
            if let Some(content) = value.get_mut("content").and_then(Value::as_array_mut) {
                content.retain(user_visible);
            }
            value
        };
        node.status = if failed {
            TraceState::Failed
        } else {
            TraceState::Succeeded
        };
        node.occurred_at = at;
        node.finished_at = Some(at);
        vec![StreamRecord::new(RecordData::Trace(node.clone()))]
    }

    fn close_message(&mut self, at: DateTime<Utc>, status: TraceState) -> Vec<StreamRecord> {
        let Some(mut node) = self.message.take() else {
            return vec![];
        };
        self.result = (!self.message_omitted).then(|| node.output.clone());
        node.status = status;
        node.occurred_at = at;
        node.finished_at = Some(at);
        vec![StreamRecord::new(RecordData::Trace(node))]
    }

    fn error(&mut self, message: &str, at: DateTime<Utc>) -> Vec<StreamRecord> {
        let mut records = self.close_message(at, TraceState::Failed);
        self.fatal = true;
        self.result = None;
        self.next_error += 1;
        records.extend([
            StreamRecord::new(RecordData::Trace(TraceNode {
                id: format!("goose:error:{}", self.next_error),
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

fn user_visible(part: &Value) -> bool {
    part["annotations"]["audience"]
        .as_array()
        .is_none_or(|roles| roles.iter().any(|role| role == "user"))
}

fn tool_id(part: &Value) -> Option<String> {
    part["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 240)
        .map(|id| format!("goose:tool:{id}"))
}
