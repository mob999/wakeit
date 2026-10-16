//! Native wire formats live here; the Runner consumes only trace::StreamRecord.
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::Stdio,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

pub struct Normalizer {
    protocol: String,
    agy: Option<crate::agy_adapter::Normalizer>,
    gemini: Option<crate::gemini_adapter::GeminiNormalizer>,
    qwen: Option<crate::qwen_cursor_adapter::QwenNormalizer>,
    cursor: Option<crate::qwen_cursor_adapter::CursorNormalizer>,
    cline: Option<crate::cline_adapter::ClineNormalizer>,
    goose: Option<crate::goose_adapter::GooseNormalizer>,
    session_id: Option<String>,
    nodes: BTreeMap<String, TraceNode>,
    message_ids: BTreeMap<String, String>,
    next_message: usize,
    next_error: usize,
    pi_current: Option<String>,
    pi_blocks: BTreeMap<usize, String>,
    pi_last_emit: Option<std::time::Instant>,
    opencode_message: Option<String>,
    opencode_parts: Vec<(String, String)>,
    result: Option<Value>,
    failed: bool,
    unknown: BTreeSet<String>,
}
impl Normalizer {
    pub fn new(protocol: &str) -> Result<Self> {
        ensure!(
            matches!(
                protocol,
                "codex"
                    | "claude-code"
                    | "pi"
                    | "opencode"
                    | "agy"
                    | "gemini-cli"
                    | "qwen-code"
                    | "cursor"
                    | "cline"
                    | "goose"
            ),
            "unsupported stream protocol"
        );
        Ok(Self {
            protocol: protocol.into(),
            agy: (protocol == "agy").then(crate::agy_adapter::Normalizer::new),
            gemini: (protocol == "gemini-cli")
                .then(crate::gemini_adapter::GeminiNormalizer::default),
            qwen: (protocol == "qwen-code")
                .then(crate::qwen_cursor_adapter::QwenNormalizer::default),
            cursor: (protocol == "cursor")
                .then(crate::qwen_cursor_adapter::CursorNormalizer::default),
            cline: (protocol == "cline").then(crate::cline_adapter::ClineNormalizer::default),
            goose: (protocol == "goose").then(crate::goose_adapter::GooseNormalizer::default),
            session_id: None,
            nodes: BTreeMap::new(),
            message_ids: BTreeMap::new(),
            next_message: 0,
            next_error: 0,
            pi_current: None,
            pi_blocks: BTreeMap::new(),
            pi_last_emit: None,
            opencode_message: None,
            opencode_parts: vec![],
            result: None,
            failed: false,
            unknown: BTreeSet::new(),
        })
    }
    fn diagnostic(&mut self, key: &str) -> Vec<StreamRecord> {
        if !self.unknown.insert(key.chars().take(128).collect()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!(
                "Native Agent event omitted ({}).\n",
                key.chars().take(128).collect::<String>()
            ),
            stream: "stderr".into(),
            diagnostic: true,
        }))]
    }
    fn native_error(&mut self, event: &Value, at: DateTime<Utc>) -> Vec<StreamRecord> {
        self.failed = true;
        self.result = None;
        self.next_error += 1;
        let message = event["message"]
            .as_str()
            .or(event["error"]["message"].as_str())
            .or(event["error"]["data"]["message"].as_str())
            .map(str::to_owned)
            .or_else(|| {
                event["errors"].as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Agent execution failed".into());
        let id = format!("native-error:{}", self.next_error);
        vec![
            self.node(
                &id,
                TraceKind::Message,
                "错误",
                Value::Null,
                json!(message),
                TraceState::Failed,
                at,
                false,
                true,
            ),
            StreamRecord::new(RecordData::Log(LogRecord {
                text: format!("{message}\n"),
                stream: "stderr".into(),
                diagnostic: false,
            })),
        ]
    }
    #[allow(clippy::too_many_arguments)]
    fn node(
        &mut self,
        id: &str,
        kind: TraceKind,
        title: &str,
        input: Value,
        output: Value,
        status: TraceState,
        at: DateTime<Utc>,
        start: bool,
        end: bool,
    ) -> StreamRecord {
        let previous = self.nodes.get(id);
        let node = TraceNode {
            id: id.into(),
            parent_id: previous.and_then(|n| n.parent_id.clone()),
            kind,
            title: title.chars().take(512).collect(),
            input: if input.is_null() {
                previous.map(|n| n.input.clone()).unwrap_or(Value::Null)
            } else {
                input
            },
            output: if output.is_null() {
                previous.map(|n| n.output.clone()).unwrap_or(Value::Null)
            } else {
                output
            },
            status,
            occurred_at: at,
            started_at: previous
                .and_then(|n| n.started_at)
                .or(if start { Some(at) } else { None }),
            finished_at: if end {
                Some(at)
            } else {
                previous.and_then(|n| n.finished_at)
            },
        };
        self.nodes.insert(id.into(), node.clone());
        StreamRecord::new(RecordData::Trace(node))
    }
    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        let native_id = native_session_id(&self.protocol, &event).map(str::to_owned);
        let mut session_records = vec![];
        if let Some(id) = native_id {
            if self.session_id.as_ref().is_some_and(|old| old != &id) {
                self.failed = true;
                self.result = None;
                let mut records = self.diagnostic("session ID changed");
                records.push(StreamRecord::new(RecordData::Session(
                    crate::sessions::SessionInfo {
                        version: 1,
                        id,
                        cwd: std::env::current_dir()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        persisted: false,
                        title: None,
                    },
                )));
                return records;
            }
            if self.session_id.is_none() && crate::sessions::validate_id(&id).is_ok() {
                self.session_id = Some(id.clone());
                session_records.push(StreamRecord::new(RecordData::Session(
                    crate::sessions::SessionInfo {
                        version: 1,
                        id,
                        cwd: std::env::current_dir()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        persisted: false,
                        title: None,
                    },
                )));
            }
        }
        let parent = event["parent_tool_use_id"]
            .as_str()
            .or(event["parent_id"].as_str())
            .map(str::to_owned);
        let mut records = self.accept_inner(event);
        if let Some(parent) = parent {
            for record in &mut records {
                if let RecordData::Trace(node) = &mut record.record
                    && node.id != parent
                {
                    node.parent_id = Some(parent.clone());
                    self.nodes.insert(node.id.clone(), node.clone());
                }
            }
        }
        session_records.extend(records);
        session_records
    }
    fn accept_inner(&mut self, event: Value) -> Vec<StreamRecord> {
        if let Some(adapter) = &mut self.goose {
            return adapter.accept(event);
        }
        if let Some(adapter) = &mut self.qwen {
            return adapter.accept(event);
        }
        if let Some(adapter) = &mut self.cursor {
            return adapter.accept(event);
        }
        if let Some(adapter) = &mut self.cline {
            return adapter.accept(event);
        }
        if let Some(adapter) = &mut self.agy {
            return adapter.accept(event);
        }
        if let Some(adapter) = &mut self.gemini {
            return adapter.accept(event);
        }
        let typ = event["type"].as_str().unwrap_or("");
        let at = event["timestamp"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                event["timestamp"]
                    .as_i64()
                    .and_then(DateTime::from_timestamp_millis)
            })
            .unwrap_or_else(Utc::now);
        match self.protocol.as_str() {
            "codex" => {
                if typ.starts_with("item.") {
                    let item = &event["item"];
                    let Some(id) = item["id"].as_str() else {
                        return self.diagnostic("missing item id");
                    };
                    let kind = item["type"].as_str().unwrap_or("");
                    let start = typ == "item.started";
                    let end = typ == "item.completed";
                    let state = if item["status"] == "failed"
                        || item["exit_code"].as_i64().is_some_and(|n| n != 0)
                    {
                        TraceState::Failed
                    } else if end {
                        TraceState::Succeeded
                    } else {
                        TraceState::Running
                    };
                    let (kind, title, input, output) = match kind {
                        "reasoning" => return vec![],
                        "agent_message" => {
                            let text = item["text"].as_str().unwrap_or("");
                            if end && item["phase"].as_str() != Some("commentary") {
                                self.result = Some(json!(text));
                            }
                            (TraceKind::Message, "消息", Value::Null, json!(text))
                        }
                        "command_execution" => (
                            TraceKind::Command,
                            item["command"].as_str().unwrap_or("命令"),
                            json!({"command":item["command"]}),
                            item["aggregated_output"].clone(),
                        ),
                        "mcp_tool_call" | "dynamic_tool_call" => (
                            TraceKind::Tool,
                            item["tool"].as_str().unwrap_or("工具"),
                            item["arguments"].clone(),
                            item.get("result")
                                .filter(|v| !v.is_null())
                                .or(item.get("error"))
                                .cloned()
                                .unwrap_or(Value::Null),
                        ),
                        "web_search" => (
                            TraceKind::Tool,
                            "搜索",
                            item["query"].clone(),
                            item["action"].clone(),
                        ),
                        "file_change" => (
                            TraceKind::Tool,
                            "修改文件",
                            item["changes"].clone(),
                            item["status"].clone(),
                        ),
                        "todo_list" | "plan" => (
                            TraceKind::Message,
                            "计划",
                            Value::Null,
                            item["items"].clone(),
                        ),
                        _ => return self.diagnostic(kind),
                    };
                    vec![self.node(id, kind, title, input, output, state, at, start, end)]
                } else if typ == "turn.completed" {
                    self.failed = false;
                    self.result
                        .clone()
                        .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
                        .unwrap_or_default()
                } else if matches!(typ, "thread.started" | "turn.started") {
                    vec![]
                } else if matches!(typ, "error" | "turn.failed") {
                    self.native_error(&event, at)
                } else {
                    self.diagnostic(typ)
                }
            }
            "claude-code" => {
                if typ == "assistant" {
                    let message = &event["message"];
                    let Some(id) = message["id"].as_str() else {
                        return self.diagnostic("missing message id");
                    };
                    let mut records = vec![];
                    for (index, part) in message["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        match part["type"].as_str() {
                            Some("text") => records.push(self.node(
                                &format!("{id}:{index}"),
                                TraceKind::Message,
                                "消息",
                                Value::Null,
                                part["text"].clone(),
                                TraceState::Succeeded,
                                at,
                                false,
                                true,
                            )),
                            Some("tool_use") => {
                                let Some(tool_id) = part["id"].as_str() else {
                                    records.extend(self.diagnostic("missing tool id"));
                                    continue;
                                };
                                let name = part["name"].as_str().unwrap_or("工具");
                                records.push(self.node(
                                    tool_id,
                                    if is_command(name) {
                                        TraceKind::Command
                                    } else {
                                        TraceKind::Tool
                                    },
                                    name,
                                    part["input"].clone(),
                                    Value::Null,
                                    TraceState::Running,
                                    at,
                                    true,
                                    false,
                                ));
                            }
                            Some("thinking" | "redacted_thinking") => {}
                            _ => records.extend(self.diagnostic("assistant content")),
                        }
                    }
                    records
                } else if typ == "user" {
                    let mut records = vec![];
                    for part in event["message"]["content"].as_array().into_iter().flatten() {
                        if part["type"] != "tool_result" {
                            continue;
                        }
                        let Some(id) = part["tool_use_id"].as_str() else {
                            records.extend(self.diagnostic("missing tool result id"));
                            continue;
                        };
                        let prior = self.nodes.get(id).cloned();
                        let name = prior.as_ref().map(|n| n.title.as_str()).unwrap_or("工具");
                        let kind = prior
                            .as_ref()
                            .map(|n| n.kind.clone())
                            .unwrap_or(TraceKind::Tool);
                        records.push(self.node(
                            id,
                            kind,
                            name,
                            Value::Null,
                            part["content"].clone(),
                            if part["is_error"] == true {
                                TraceState::Failed
                            } else {
                                TraceState::Succeeded
                            },
                            at,
                            false,
                            true,
                        ));
                    }
                    records
                } else if typ == "result" {
                    self.failed = event["is_error"] == true
                        || event["subtype"]
                            .as_str()
                            .is_some_and(|s| s.starts_with("error"));
                    if self.failed {
                        return self.native_error(&event, at);
                    }
                    if let Some(result) = event.get("result") {
                        self.result = Some(result.clone());
                        vec![StreamRecord::new(RecordData::Result(result.clone()))]
                    } else {
                        vec![]
                    }
                } else if matches!(typ, "system" | "rate_limit_event") {
                    vec![]
                } else {
                    self.diagnostic(typ)
                }
            }
            "pi" => {
                if typ == "message_update" && event.get("message").is_none() {
                    let update = &event["assistantMessageEvent"];
                    let Some(id) = self.pi_current.clone() else {
                        return vec![];
                    };
                    let Some(index) = update["contentIndex"].as_u64().map(|n| n as usize) else {
                        return vec![];
                    };
                    match update["type"].as_str() {
                        Some("text_start") => {
                            self.pi_blocks.entry(index).or_default();
                        }
                        Some("text_delta") => {
                            self.pi_blocks
                                .entry(index)
                                .or_default()
                                .push_str(update["delta"].as_str().unwrap_or(""));
                        }
                        Some("text_end") => {
                            self.pi_blocks
                                .insert(index, update["content"].as_str().unwrap_or("").into());
                        }
                        _ => return vec![],
                    }
                    // Full snapshots stay idempotent without publishing once per token.
                    if self
                        .pi_last_emit
                        .is_some_and(|last| last.elapsed() < std::time::Duration::from_millis(100))
                    {
                        return vec![];
                    }
                    self.pi_last_emit = Some(std::time::Instant::now());
                    let text = self
                        .pi_blocks
                        .values()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n");
                    return vec![self.node(
                        &id,
                        TraceKind::Message,
                        "消息",
                        Value::Null,
                        json!(text),
                        TraceState::Running,
                        at,
                        false,
                        false,
                    )];
                }
                if matches!(
                    typ,
                    "tool_execution_start" | "tool_execution_update" | "tool_execution_end"
                ) {
                    let Some(id) = event["toolCallId"].as_str() else {
                        return self.diagnostic("missing tool id");
                    };
                    let previous = self.nodes.get(id).cloned();
                    let name = event["toolName"]
                        .as_str()
                        .or(previous.as_ref().map(|n| n.title.as_str()))
                        .unwrap_or("工具");
                    self.result = None;
                    let end = typ == "tool_execution_end";
                    vec![
                        self.node(
                            id,
                            if is_command(name) {
                                TraceKind::Command
                            } else {
                                TraceKind::Tool
                            },
                            name,
                            event["args"].clone(),
                            event
                                .get("result")
                                .or(event.get("partialResult"))
                                .cloned()
                                .unwrap_or(Value::Null),
                            if end {
                                if event["isError"] == true {
                                    TraceState::Failed
                                } else {
                                    TraceState::Succeeded
                                }
                            } else {
                                TraceState::Running
                            },
                            at,
                            typ == "tool_execution_start",
                            end,
                        ),
                    ]
                } else if matches!(typ, "message_start" | "message_update" | "message_end") {
                    let message = &event["message"];
                    if message["role"] != "assistant" {
                        return vec![];
                    }
                    let key = message["id"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| message["timestamp"].as_i64().map(|n| n.to_string()))
                        .unwrap_or_else(|| "current".into());
                    if typ == "message_start" && !self.message_ids.contains_key(&key) {
                        self.next_message += 1;
                        self.message_ids
                            .insert(key.clone(), format!("message:{}", self.next_message));
                        self.result = None;
                    }
                    let id = self
                        .message_ids
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| format!("message:{key}"));
                    if typ == "message_start" {
                        self.pi_current = Some(id.clone());
                        self.pi_blocks.clear();
                        self.pi_last_emit = None;
                    }
                    let mut text = content_text(&message["content"]);
                    let end = typ == "message_end";
                    if end {
                        self.failed = matches!(
                            message["stopReason"].as_str(),
                            Some("error" | "aborted" | "length")
                        );
                        if self.failed && text.is_empty() {
                            text = message["errorMessage"]
                                .as_str()
                                .unwrap_or("Agent request failed")
                                .into();
                        }
                    }
                    if end
                        && !message["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|p| p["type"] == "toolCall")
                        && !matches!(message["stopReason"].as_str(), Some("error" | "aborted"))
                    {
                        self.result = Some(json!(text));
                    }
                    vec![self.node(
                        &id,
                        TraceKind::Message,
                        "消息",
                        Value::Null,
                        json!(text),
                        if end {
                            if matches!(message["stopReason"].as_str(), Some("error" | "aborted")) {
                                TraceState::Failed
                            } else {
                                TraceState::Succeeded
                            }
                        } else {
                            TraceState::Running
                        },
                        at,
                        typ == "message_start",
                        end,
                    )]
                } else if typ == "agent_settled" && event["aborted"] == true {
                    self.failed = true;
                    self.result = None;
                    vec![]
                } else if matches!(
                    typ,
                    "session"
                        | "entry_appended"
                        | "agent_start"
                        | "agent_end"
                        | "agent_settled"
                        | "turn_start"
                        | "turn_end"
                        | "auto_retry_start"
                        | "auto_retry_end"
                        | "auto_compaction_start"
                        | "auto_compaction_end"
                ) {
                    vec![]
                } else {
                    self.diagnostic(typ)
                }
            }
            "opencode" => {
                let part = &event["part"];
                if matches!(typ, "text" | "tool_use") {
                    let Some(id) = part["id"].as_str() else {
                        return self.diagnostic("missing part id");
                    };
                    if typ == "text" {
                        let text = part["text"].clone();
                        let message = part["messageID"].as_str().unwrap_or(id);
                        if self.opencode_message.as_deref() != Some(message) {
                            self.opencode_message = Some(message.into());
                            self.opencode_parts.clear();
                        }
                        let value = text.as_str().unwrap_or("").to_owned();
                        if let Some((_, old)) =
                            self.opencode_parts.iter_mut().find(|(key, _)| key == id)
                        {
                            *old = value;
                        } else {
                            self.opencode_parts.push((id.into(), value));
                        }
                        self.result = Some(json!(
                            self.opencode_parts
                                .iter()
                                .map(|(_, text)| text.as_str())
                                .collect::<Vec<_>>()
                                .join("\n")
                        ));
                        vec![self.node(
                            id,
                            TraceKind::Message,
                            "消息",
                            Value::Null,
                            text,
                            TraceState::Succeeded,
                            at,
                            false,
                            true,
                        )]
                    } else {
                        self.result = None;
                        self.opencode_parts.clear();
                        let name = part["tool"].as_str().unwrap_or("工具");
                        let state = &part["state"];
                        let status = state["status"].as_str().unwrap_or("running");
                        let start_at = state["time"]["start"]
                            .as_i64()
                            .and_then(DateTime::from_timestamp_millis);
                        let end_at = state["time"]["end"]
                            .as_i64()
                            .and_then(DateTime::from_timestamp_millis);
                        let record = self.node(
                            id,
                            if is_command(name) {
                                TraceKind::Command
                            } else {
                                TraceKind::Tool
                            },
                            name,
                            state["input"].clone(),
                            state
                                .get("output")
                                .filter(|v| !v.is_null())
                                .or(state.get("error"))
                                .cloned()
                                .unwrap_or(Value::Null),
                            match status {
                                "completed" => TraceState::Succeeded,
                                "error" => TraceState::Failed,
                                _ => TraceState::Running,
                            },
                            end_at.or(start_at).unwrap_or(at),
                            false,
                            false,
                        );
                        if let RecordData::Trace(mut n) = record.record {
                            n.started_at = start_at;
                            n.finished_at = end_at;
                            self.nodes.insert(id.into(), n.clone());
                            vec![StreamRecord::new(RecordData::Trace(n))]
                        } else {
                            unreachable!()
                        }
                    }
                } else if matches!(typ, "step_start" | "step_finish" | "reasoning") {
                    vec![]
                } else if typ == "error" {
                    self.native_error(&event, at)
                } else {
                    self.diagnostic(typ)
                }
            }
            _ => unreachable!(),
        }
    }
    pub fn failed(&self) -> bool {
        self.failed
            || self.agy.as_ref().is_some_and(|adapter| adapter.failed())
            || self.gemini.as_ref().is_some_and(|adapter| adapter.failed())
            || self.qwen.as_ref().is_some_and(|adapter| adapter.failed())
            || self.cursor.as_ref().is_some_and(|adapter| adapter.failed())
            || self.cline.as_ref().is_some_and(|adapter| adapter.failed())
            || self.goose.as_ref().is_some_and(|adapter| adapter.failed())
    }
    pub fn finish(&self) -> Vec<StreamRecord> {
        if self.failed() {
            return vec![];
        }
        if let Some(adapter) = &self.goose {
            return adapter.finish();
        }
        if let Some(adapter) = &self.qwen {
            return adapter.finish();
        }
        if let Some(adapter) = &self.cursor {
            return adapter.finish();
        }
        if let Some(adapter) = &self.cline {
            return adapter.finish();
        }
        if let Some(adapter) = &self.agy {
            return adapter.finish();
        }
        if let Some(adapter) = &self.gemini {
            return adapter.finish();
        }
        self.result
            .clone()
            .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
            .unwrap_or_default()
    }
}
fn is_command(name: &str) -> bool {
    matches!(name, "bash" | "Bash" | "shell" | "exec_command")
}
fn content_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "text")
        .filter_map(|p| p["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
async fn write_records(
    writer: &mut (impl AsyncWriteExt + Unpin),
    records: Vec<StreamRecord>,
) -> Result<()> {
    for record in records {
        let bytes = serde_json::to_vec(&record)?;
        writer.write_all(&bytes).await?;
        writer.write_all(b"\n").await?;
    }
    writer.flush().await?;
    Ok(())
}
pub fn native_session_id<'a>(protocol: &str, event: &'a Value) -> Option<&'a str> {
    match protocol {
        "codex" if event["type"] == "thread.started" => event["thread_id"].as_str(),
        "claude-code" => event["session_id"].as_str(),
        "pi" if event["type"] == "session" => event["id"].as_str(),
        "opencode" => event["sessionID"].as_str(),
        "agy" => crate::agy_adapter::native_session_id(event),
        "gemini-cli" if event["type"] == "init" => event["session_id"].as_str(),
        "qwen-code" | "cursor" => crate::qwen_cursor_adapter::native_session_id(event),
        _ => None,
    }
}
pub async fn stream(protocol: &str, args: &[String]) -> Result<i32> {
    stream_checked(protocol, args, None).await
}
pub async fn stream_checked(
    protocol: &str,
    args: &[String],
    expected_session: Option<&str>,
) -> Result<i32> {
    let mut normalizer = Normalizer::new(protocol)?;
    let executable = args.first().context("provide a native command after --")?;
    let mut spec = crate::config::CommandSpec {
        command: executable.clone(),
        args: args[1..].iter().cloned().map(Into::into).collect(),
        ..Default::default()
    };
    crate::config::normalize_native_command(&mut spec)?;
    // Inherit the Runner's process group / Windows Job Object; never detach the child.
    let mut command = Command::new(&spec.command);
    #[cfg(windows)]
    ensure!(
        !spec.command.to_lowercase().ends_with(".cmd")
            && !spec.command.to_lowercase().ends_with(".bat"),
        "use an explicit interpreter and script entry point on Windows"
    );
    let arguments = spec.args.iter().filter_map(|arg| match arg {
        crate::config::CommandArg::Literal(v) => Some(v),
        _ => None,
    });
    // AGY's documented stdin is NDJSON, not a raw prompt. Other native CLIs
    // retain their configured stdin exactly as supplied by the adapter.
    let prompt_input = if protocol == "agy" {
        let mut input = Vec::new();
        tokio::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut input)
            .await?;
        ensure!(input.len() <= 1024 * 1024, "prompt exceeds 1 MiB");
        Some(crate::agy_adapter::encode_prompt(
            std::str::from_utf8(&input).context("prompt must be UTF-8")?,
        )?)
    } else {
        None
    };
    command.env("PWD", std::env::current_dir()?);
    command
        .args(arguments)
        .stdin(if prompt_input.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().context("start native Agent")?;
    let input_task = if let Some(bytes) = prompt_input {
        let mut stdin = child.stdin.take().context("native stdin")?;
        Some(tokio::spawn(async move {
            stdin.write_all(&bytes).await?;
            stdin.shutdown().await
        }))
    } else {
        None
    };
    let mut reader = child.stdout.take().context("native stdout")?;
    let mut writer = tokio::io::stdout();
    let mut bytes = vec![];
    let mut dropping = false;
    let mut buffer = [0u8; 8192];
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        for &b in &buffer[..n] {
            if b == b'\n' {
                if !dropping && !bytes.iter().all(u8::is_ascii_whitespace) {
                    let records = match serde_json::from_slice(&bytes) {
                        Ok(v) => {
                            if expected_session
                                .zip(native_session_id(protocol, &v))
                                .is_some_and(|(a, b)| a != b)
                            {
                                write_records(
                                    &mut writer,
                                    vec![StreamRecord::new(RecordData::Session(
                                        crate::sessions::SessionInfo {
                                            version: 1,
                                            id: native_session_id(protocol, &v).unwrap().into(),
                                            cwd: std::env::current_dir()?
                                                .to_string_lossy()
                                                .into_owned(),
                                            persisted: false,
                                            title: None,
                                        },
                                    ))],
                                )
                                .await?;
                                child.kill().await?;
                                child.wait().await?;
                                anyhow::bail!("Agent returned a different session ID");
                            }
                            normalizer.accept(v)
                        }
                        Err(_) => normalizer.diagnostic("invalid json"),
                    };
                    write_records(&mut writer, records).await?;
                }
                bytes.clear();
                dropping = false;
            } else if !dropping {
                if bytes.len() == RECORD_LIMIT {
                    bytes.clear();
                    dropping = true;
                    write_records(
                        &mut writer,
                        normalizer.diagnostic("record exceeds 1000 KiB"),
                    )
                    .await?;
                } else {
                    bytes.push(b);
                }
            }
        }
    }
    if !dropping && !bytes.iter().all(u8::is_ascii_whitespace) {
        let records = match serde_json::from_slice(&bytes) {
            Ok(v) => {
                if expected_session
                    .zip(native_session_id(protocol, &v))
                    .is_some_and(|(a, b)| a != b)
                {
                    write_records(
                        &mut writer,
                        vec![StreamRecord::new(RecordData::Session(
                            crate::sessions::SessionInfo {
                                version: 1,
                                id: native_session_id(protocol, &v).unwrap().into(),
                                cwd: std::env::current_dir()?.to_string_lossy().into_owned(),
                                persisted: false,
                                title: None,
                            },
                        ))],
                    )
                    .await?;
                    child.kill().await?;
                    child.wait().await?;
                    anyhow::bail!("Agent returned a different session ID");
                }
                normalizer.accept(v)
            }
            Err(_) => normalizer.diagnostic("invalid json"),
        };
        write_records(&mut writer, records).await?;
    }
    if expected_session.is_some() && normalizer.session_id.is_none() {
        normalizer.failed = true;
        normalizer.result = None;
        write_records(&mut writer, normalizer.diagnostic("session ID missing")).await?;
    }
    let status = child.wait().await?;
    if let Some(input) = input_task {
        let written = input.await.context("native stdin writer stopped")?;
        // A native failure can close stdin early. Preserve that exit code.
        if status.success() {
            written.context("write native prompt")?;
        }
    }
    // A native nonzero exit never publishes an earlier partial answer as a report.
    if status.success() {
        write_records(&mut writer, normalizer.finish()).await?;
    }
    let code = status.code().unwrap_or(1);
    Ok(if code == 0 && normalizer.failed() {
        1
    } else {
        code
    })
}
