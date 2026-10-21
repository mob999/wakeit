//! Finalized headless envelopes for Qwen Code and Cursor Agent.
//! See tests/fixtures/additional-cli/README.md for pinned official contracts.
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct State {
    pending: BTreeMap<String, TraceNode>,
    seen_tools: BTreeSet<String>,
    seen_messages: BTreeSet<String>,
    unknown: BTreeSet<String>,
    pending_bytes: usize,
    next_message: usize,
    completed: bool,
    fatal: bool,
    trace_omitted: bool,
    result: Option<Value>,
}
impl State {
    fn diagnostic(&mut self, key: &str) -> Vec<StreamRecord> {
        let key: String = key.chars().take(128).collect();
        if self.unknown.len() >= 128 || !self.unknown.insert(key.clone()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!("Native Agent event omitted ({key}).\n"),
            stream: "stderr".into(),
            diagnostic: true,
        }))]
    }
    fn invalid(&mut self, key: &str) -> Vec<StreamRecord> {
        self.fatal = true;
        self.result = None;
        self.diagnostic(key)
    }
    fn omit_trace(&mut self, key: &str) -> Vec<StreamRecord> {
        // Retain terminal/session parsing after the observability budget is spent.
        self.trace_omitted = true;
        self.pending.clear();
        self.seen_tools.clear();
        self.seen_messages.clear();
        self.pending_bytes = 0;
        self.diagnostic(key)
    }
    fn bounded(value: Value) -> Value {
        if serde_json::to_vec(&value).map_or(true, |v| v.len() > RECORD_LIMIT - 8192) {
            json!("[Native content omitted: exceeds 1000 KiB]")
        } else {
            value
        }
    }
    fn message(&mut self, event: &Value, prefix: &str) -> Vec<StreamRecord> {
        if self.trace_omitted {
            return vec![];
        }
        let key = event["message"]["id"].as_str().or(event["uuid"].as_str());
        if let Some(key) = key {
            if key.len() > 256 {
                return self.invalid("invalid message id");
            }
            if self.seen_messages.contains(key) {
                return vec![];
            }
            if self.seen_messages.len() >= 10_000 {
                return self.omit_trace("trace omitted: message count budget reached");
            }
            self.seen_messages.insert(key.into());
        }
        let Some(parts) = event["message"]["content"].as_array() else {
            return self.invalid("invalid assistant content");
        };
        let text = parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return vec![];
        }
        self.next_message += 1;
        let at = Utc::now();
        vec![StreamRecord::new(RecordData::Trace(TraceNode {
            id: format!("{prefix}:message:{}", self.next_message),
            parent_id: parent(event, ""),
            kind: TraceKind::Message,
            title: "Message".into(),
            input: Value::Null,
            output: Self::bounded(json!(text)),
            status: TraceState::Succeeded,
            occurred_at: at,
            started_at: None,
            finished_at: Some(at),
        }))]
    }
    fn tool_start(
        &mut self,
        id: &str,
        name: &str,
        input: Value,
        parent_id: Option<String>,
        command: bool,
    ) -> Vec<StreamRecord> {
        if self.trace_omitted {
            return vec![];
        }
        if !valid_trace_id(id) || name.is_empty() {
            return self.invalid("invalid tool identity");
        }
        if self.seen_tools.contains(id) {
            return vec![];
        }
        if self.seen_tools.len() >= 10_000 {
            return self.omit_trace("trace omitted: tool count budget reached");
        }
        let input = Self::bounded(input);
        let bytes = serde_json::to_vec(&input).map_or(RECORD_LIMIT, |v| v.len());
        if self.pending_bytes + bytes > 8 * 1024 * 1024 {
            return self.omit_trace("trace omitted: pending tool input budget reached");
        }
        self.pending_bytes += bytes;
        self.seen_tools.insert(id.into());
        let at = Utc::now();
        let node = TraceNode {
            id: id.into(),
            parent_id,
            kind: if command {
                TraceKind::Command
            } else {
                TraceKind::Tool
            },
            title: name.chars().take(512).collect(),
            input,
            output: Value::Null,
            status: TraceState::Running,
            occurred_at: at,
            started_at: Some(at),
            finished_at: None,
        };
        self.pending.insert(id.into(), node.clone());
        vec![StreamRecord::new(RecordData::Trace(node))]
    }
    fn tool_end(&mut self, id: &str, output: Value, failed: bool) -> Vec<StreamRecord> {
        if self.trace_omitted {
            return vec![];
        }
        let Some(mut node) = self.pending.remove(id) else {
            return if self.seen_tools.contains(id) {
                vec![]
            } else {
                self.invalid("unmatched tool result")
            };
        };
        self.pending_bytes = self
            .pending_bytes
            .saturating_sub(serde_json::to_vec(&node.input).map_or(0, |v| v.len()));
        node.output = Self::bounded(output);
        if serde_json::to_vec(&node).map_or(true, |v| v.len() > RECORD_LIMIT - 1024) {
            node.output = json!("[Native content omitted: combined tool record exceeds 1000 KiB]");
        }
        node.status = if failed {
            TraceState::Failed
        } else {
            TraceState::Succeeded
        };
        node.occurred_at = Utc::now();
        node.finished_at = Some(node.occurred_at);
        vec![StreamRecord::new(RecordData::Trace(node))]
    }
    fn terminal(&mut self, event: &Value) -> Vec<StreamRecord> {
        self.completed = true;
        if event["subtype"] != "success" || event["is_error"] != false {
            self.fatal = true;
            self.result = None;
            let message = event["error"]["message"]
                .as_str()
                .or(event["error"].as_str())
                .or(event["result"].as_str())
                .unwrap_or("Agent execution failed");
            let message: String = message.chars().take(8192).collect();
            let at = Utc::now();
            return vec![
                StreamRecord::new(RecordData::Trace(TraceNode {
                    id: "native:terminal-error".into(),
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
            ];
        }
        if !self.pending.is_empty() {
            return self.invalid("unfinished tool calls at result");
        }
        let Some(result) = event["result"].as_str() else {
            return self.invalid("missing terminal result");
        };
        if !self.fatal {
            let encoded = serde_json::to_vec(result).unwrap_or_default();
            self.result = Some(if encoded.len() > RECORD_LIMIT - 8192 {
                let mut end = result.len().min(32 * 1024);
                while !result.is_char_boundary(end) {
                    end -= 1;
                }
                json!({"truncated":true,"preview":&result[..end]})
            } else {
                json!(result)
            });
        }
        vec![]
    }
    fn failed(&self) -> bool {
        self.fatal || !self.completed
    }
    fn finish(&self) -> Vec<StreamRecord> {
        if self.failed() {
            return vec![];
        }
        self.result
            .clone()
            .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
            .unwrap_or_default()
    }
}
fn valid_trace_id(id: &str) -> bool {
    !id.trim().is_empty() && id.len() <= 256
}
fn parent(event: &Value, id: &str) -> Option<String> {
    event["parent_tool_use_id"]
        .as_str()
        .filter(|p| valid_trace_id(p) && *p != id)
        .map(str::to_owned)
}

fn envelope(state: &mut State, event: Value, prefix: &str) -> Vec<StreamRecord> {
    if state.completed {
        return state.diagnostic("event after result");
    }
    if state.fatal && event["type"] != "result" {
        return vec![];
    }
    match event["type"].as_str().unwrap_or("") {
        "system" => vec![],
        "assistant" => {
            let mut records = state.message(&event, prefix);
            for part in event["message"]["content"].as_array().into_iter().flatten() {
                if part["type"] != "tool_use" {
                    continue;
                }
                let (Some(id), Some(name)) = (part["id"].as_str(), part["name"].as_str()) else {
                    records.extend(state.invalid("missing tool identity"));
                    continue;
                };
                records.extend(state.tool_start(
                    id,
                    name,
                    part["input"].clone(),
                    parent(&event, id),
                    matches!(name, "Bash" | "bash" | "shell" | "run_shell_command"),
                ));
            }
            records
        }
        "user" => {
            let mut records = vec![];
            for part in event["message"]["content"].as_array().into_iter().flatten() {
                if part["type"] != "tool_result" {
                    continue;
                }
                let Some(id) = part["tool_use_id"].as_str() else {
                    records.extend(state.invalid("missing tool result id"));
                    continue;
                };
                records.extend(state.tool_end(
                    id,
                    part["content"].clone(),
                    part["is_error"] == true,
                ));
            }
            records
        }
        "result" => state.terminal(&event),
        // Qwen emits canonical Goal/progress metadata here even when partial
        // assistant streaming is disabled. Never expose thinking payloads.
        "stream_event" => vec![],
        other => state.diagnostic(other),
    }
}

#[derive(Default)]
pub struct QwenNormalizer(State);
impl QwenNormalizer {
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        envelope(&mut self.0, event, "qwen")
    }
    pub fn failed(&self) -> bool {
        self.0.failed()
    }
    pub fn finish(&self) -> Vec<StreamRecord> {
        self.0.finish()
    }
}
#[derive(Default)]
pub struct CursorNormalizer(State);
impl CursorNormalizer {
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        let state = &mut self.0;
        if state.completed {
            return state.diagnostic("event after result");
        }
        match event["type"].as_str().unwrap_or("") {
            "system" | "user" => vec![],
            "assistant" => {
                // YAML selects finalized messages. Ignore optional partial-mode
                // deltas/pre-tool flushes; terminal result remains authoritative.
                if event.get("timestamp_ms").is_some() {
                    return vec![];
                }
                state.message(&event, "cursor")
            }
            "tool_call" => {
                let Some(id) = event["call_id"].as_str().filter(|s| valid_trace_id(s)) else {
                    return state.invalid("missing Cursor call id");
                };
                let Some(calls) = event["tool_call"].as_object() else {
                    return state.invalid("missing Cursor tool call");
                };
                let Some((name, call)) = calls
                    .iter()
                    .find(|(name, _)| name.ends_with("ToolCall") || name.as_str() == "function")
                else {
                    return state.diagnostic("unsupported Cursor tool call");
                };
                let title = if name == "function" {
                    call["name"].as_str().unwrap_or("Tool")
                } else {
                    name.as_str()
                };
                let input = if name == "function" {
                    call["arguments"]
                        .as_str()
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| call["arguments"].clone())
                } else {
                    call["args"].clone()
                };
                match event["subtype"].as_str().unwrap_or("") {
                    "started" => state.tool_start(
                        id,
                        title,
                        input,
                        None,
                        name == "shellToolCall"
                            || matches!(title, "shell" | "Bash" | "run_terminal_cmd"),
                    ),
                    "completed" => {
                        let result = &call["result"];
                        if result.is_null() {
                            return state.invalid("missing Cursor tool result");
                        }
                        let failed = result.get("error").is_some_and(|v| !v.is_null())
                            || result.get("failure").is_some_and(|v| !v.is_null());
                        state.tool_end(id, result.clone(), failed)
                    }
                    other => state.diagnostic(other),
                }
            }
            "result" => state.terminal(&event),
            other => state.diagnostic(other),
        }
    }
    pub fn failed(&self) -> bool {
        self.0.failed()
    }
    pub fn finish(&self) -> Vec<StreamRecord> {
        self.0.finish()
    }
}

/// Identity only: never claims native persistence or resumability.
pub fn native_session_id(event: &Value) -> Option<&str> {
    event["session_id"]
        .as_str()
        .filter(|id| crate::sessions::validate_id(id).is_ok())
}
