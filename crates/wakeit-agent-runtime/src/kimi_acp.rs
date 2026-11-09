//! Native Kimi ACP over stdio. No HTTP listener or copied server credentials.
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet, VecDeque},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
use wakeit_plugin_sdk::{
    capture::bounded_value,
    sessions::SessionInfo,
    trace::{
        LogRecord, RECORD_LIMIT, RecordData, RecordingIssue, StreamRecord, TraceKind, TraceNode,
        TraceState,
    },
};

const NATIVE_LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Default)]
pub struct Normalizer {
    nodes: BTreeMap<String, TraceNode>,
    message: Option<String>,
    next_message: usize,
    answer: String,
    shortened: bool,
    notices: BTreeSet<RecordingIssue>,
    finished: HashSet<String>,
    finished_order: VecDeque<String>,
    retained: usize,
}
fn content(value: &Value) -> String {
    value
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    if block["type"] == "content" && block["content"]["type"] == "text" {
                        block["content"]["text"].as_str()
                    } else if block["type"] == "text" {
                        block["text"].as_str()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
impl Normalizer {
    fn notice(&mut self, issue: RecordingIssue, out: &mut Vec<StreamRecord>) {
        if self.notices.insert(issue) {
            out.push(StreamRecord::new(RecordData::Log(LogRecord {
                text: "Kimi trace preview shortened; native execution continues.\n".into(),
                stream: "stderr".into(),
                diagnostic: true,
                recording_issue: Some(issue),
            })));
        }
    }
    fn finish_message(&mut self, out: &mut Vec<StreamRecord>) {
        if let Some(id) = self.message.take()
            && let Some(node) = self.nodes.remove(&id)
        {
            out.push(StreamRecord::new(RecordData::Trace(TraceNode {
                status: TraceState::Succeeded,
                finished_at: Some(Utc::now()),
                ..node
            })));
        }
    }
    pub fn update(&mut self, update: &Value) -> Result<Vec<StreamRecord>> {
        let mut out = Vec::new();
        match update["sessionUpdate"].as_str().unwrap_or("") {
            "agent_message_chunk" if update["content"]["type"] == "text" => {
                let text = update["content"]["text"]
                    .as_str()
                    .context("ACP text chunk missing text")?;
                let remaining = (RECORD_LIMIT - 4096).saturating_sub(self.answer.len());
                let mut end = text.len().min(remaining);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                self.answer.push_str(&text[..end]);
                self.shortened |= end < text.len();
                let id = self
                    .message
                    .get_or_insert_with(|| {
                        self.next_message += 1;
                        format!("kimi-message:{}", self.next_message)
                    })
                    .clone();
                let node = self.nodes.entry(id.clone()).or_insert_with(|| TraceNode {
                    id,
                    parent_id: None,
                    kind: TraceKind::Message,
                    status: TraceState::Running,
                    title: "Assistant".into(),
                    input: Value::Null,
                    output: Value::Null,
                    occurred_at: Utc::now(),
                    started_at: Some(Utc::now()),
                    finished_at: None,
                });
                node.output = bounded_value(json!(&self.answer), RECORD_LIMIT / 4);
                out.push(StreamRecord::new(RecordData::Trace(node.clone())));
            }
            "tool_call" | "tool_call_update" => {
                if update["sessionUpdate"] == "tool_call" {
                    self.finish_message(&mut out);
                    self.answer.clear();
                    self.shortened = false;
                }
                let id = update["toolCallId"]
                    .as_str()
                    .context("ACP tool ID missing")?;
                ensure!(!id.is_empty() && id.len() <= 256, "invalid ACP tool ID");
                let id = format!("kimi-tool:{id}");
                if self.finished.contains(&id) {
                    return Ok(out);
                }
                if self.nodes.len() >= 10000 && !self.nodes.contains_key(&id) {
                    self.notice(RecordingIssue::EventLimit, &mut out);
                    return Ok(out);
                }
                let node = self.nodes.entry(id.clone()).or_insert_with(|| TraceNode {
                    id: id.clone(),
                    parent_id: None,
                    kind: TraceKind::Tool,
                    status: TraceState::Running,
                    title: "Tool".into(),
                    input: Value::Null,
                    output: Value::Null,
                    occurred_at: Utc::now(),
                    started_at: Some(Utc::now()),
                    finished_at: None,
                });
                let previous = serde_json::to_vec(&node.input)?.len()
                    + serde_json::to_vec(&node.output)?.len();
                if let Some(title) = update["title"].as_str() {
                    node.title = title.chars().take(4096).collect();
                }
                if update["kind"] == "execute" {
                    node.kind = TraceKind::Command;
                }
                if !update["rawInput"].is_null() {
                    node.input = bounded_value(update["rawInput"].clone(), RECORD_LIMIT / 4);
                }
                if !update["rawOutput"].is_null() {
                    node.output = bounded_value(update["rawOutput"].clone(), RECORD_LIMIT / 4);
                } else if update.get("content").is_some() {
                    node.output =
                        bounded_value(json!(content(&update["content"])), RECORD_LIMIT / 4);
                }
                let terminal = matches!(update["status"].as_str(), Some("completed" | "failed"));
                if terminal {
                    node.status = if update["status"] == "completed" {
                        TraceState::Succeeded
                    } else {
                        TraceState::Failed
                    };
                    node.finished_at = Some(Utc::now());
                }
                let size = serde_json::to_vec(&node.input)?.len()
                    + serde_json::to_vec(&node.output)?.len();
                self.retained = self.retained.saturating_sub(previous);
                let omitted = self.retained + size > 8 * 1024 * 1024;
                if omitted {
                    node.input = Value::Null;
                    node.output = Value::Null;
                } else {
                    self.retained += size;
                }
                node.occurred_at = Utc::now();
                out.push(StreamRecord::new(RecordData::Trace(node.clone())));
                if omitted {
                    self.notice(RecordingIssue::ContentLimit, &mut out);
                }
                if terminal {
                    if let Some(node) = self.nodes.remove(&id) {
                        self.retained = self.retained.saturating_sub(
                            serde_json::to_vec(&node.input)?.len()
                                + serde_json::to_vec(&node.output)?.len(),
                        );
                    }
                    if self.finished_order.len() >= 10000
                        && let Some(old) = self.finished_order.pop_front()
                    {
                        self.finished.remove(&old);
                    }
                    self.finished.insert(id.clone());
                    self.finished_order.push_back(id);
                }
            }
            // ACP may include private thought chunks. They are never forwarded.
            _ => {}
        }
        if self.shortened || out.iter().any(|r|matches!(&r.record,RecordData::Trace(n) if n.input["truncated"]==true || n.output["truncated"]==true)) {self.notice(RecordingIssue::ContentShortened,&mut out);}
        Ok(out)
    }
    pub fn finish(&mut self, stop: &str) -> Result<Vec<StreamRecord>> {
        let mut out = Vec::new();
        if stop != "end_turn" {
            for (_, mut node) in std::mem::take(&mut self.nodes) {
                node.status = if stop == "cancelled" {
                    TraceState::Cancelled
                } else {
                    TraceState::Failed
                };
                node.finished_at = Some(Utc::now());
                out.push(StreamRecord::new(RecordData::Trace(node)));
            }
            self.message = None;
            self.answer.clear();
            return Ok(out);
        }
        ensure!(
            self.nodes.values().all(|n| n.kind == TraceKind::Message),
            "Kimi ACP ended with incomplete tool calls"
        );
        self.finish_message(&mut out);
        if !self.answer.trim().is_empty() {
            out.push(StreamRecord::new(RecordData::Result(if self.shortened {
                wakeit_plugin_sdk::capture::preview(&self.answer)
            } else {
                json!(&self.answer)
            })));
        }
        Ok(out)
    }
}
struct Rpc {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next: u64,
    frame: Vec<u8>,
    session: Option<String>,
    normalizer: Normalizer,
    title: Option<String>,
    cwd: String,
}
async fn write(records: Vec<StreamRecord>) -> Result<()> {
    crate::stream::write_records(&mut tokio::io::stdout(), records).await
}
impl Rpc {
    async fn start(native: &[String]) -> Result<Self> {
        let executable = native.first().context("Kimi ACP command is required")?;
        let mut executable = executable.to_owned();
        let mut args = native[1..].to_vec();
        wakeit_plugin_sdk::native::normalize_native_command(&mut executable, &mut args)?;
        let native = Command::new(executable)
            .args(args)
            .env("PWD", std::env::current_dir()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        let mut child = match native {
            Ok(child) => child,
            Err(error) => {
                crate::official_io::start_failed(&error.to_string()).await?;
                return Err(error).context("start Kimi ACP");
            }
        };
        crate::official_io::started(child.id().context("native process ID unavailable")?).await?;
        let input = child.stdin.take().context("Kimi ACP stdin missing")?;
        let output = BufReader::new(child.stdout.take().context("Kimi ACP stdout missing")?);
        Ok(Self {
            child,
            input: Some(input),
            output,
            next: 0,
            frame: Vec::new(),
            session: None,
            normalizer: Normalizer::default(),
            title: crate::official_io::session_title(),
            cwd: wakeit_plugin_sdk::native::external_path(
                &std::env::current_dir()?.canonicalize()?,
            ),
        })
    }
    async fn send(&mut self, message: Value) -> Result<()> {
        let input = self.input.as_mut().context("Kimi ACP is closed")?;
        input.write_all(&serde_json::to_vec(&message)?).await?;
        input.write_all(b"\n").await?;
        input.flush().await?;
        Ok(())
    }
    async fn request(&mut self, method: &str, params: Value) -> Result<u64> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        Ok(id)
    }
    async fn receive(&mut self, id: u64) -> Result<Value> {
        loop {
            let remaining = NATIVE_LIMIT + 1 - self.frame.len() as u64;
            let read = (&mut self.output)
                .take(remaining)
                .read_until(b'\n', &mut self.frame)
                .await?;
            ensure!(read > 0, "Kimi ACP stream closed before request completed");
            ensure!(
                self.frame.len() as u64 <= NATIVE_LIMIT,
                "Kimi ACP record exceeds 64 MiB"
            );
            let message: Value =
                serde_json::from_slice(&self.frame).context("Kimi ACP returned invalid JSON")?;
            self.frame.clear();
            if let Some(method) = message["method"].as_str() {
                if !message["id"].is_null() {
                    if method == "session/request_permission" {
                        let option = message["params"]["options"]
                            .as_array()
                            .and_then(|options| options.iter().find(|o| o["kind"] == "reject_once"))
                            .and_then(|o| o["optionId"].as_str());
                        let outcome = option
                            .map(|o| json!({"outcome":"selected","optionId":o}))
                            .unwrap_or_else(|| json!({"outcome":"cancelled"}));
                        self.send(json!({"jsonrpc":"2.0","id":message["id"],"result":{"outcome":outcome}})).await?;
                    } else {
                        self.send(json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Client capability not advertised"}})).await?;
                    }
                } else if method == "session/update" {
                    let params = &message["params"];
                    if let Some(expected) = &self.session {
                        ensure!(
                            params["sessionId"] == *expected,
                            "Kimi ACP returned another session ID"
                        );
                    }
                    let update = &params["update"];
                    if update["sessionUpdate"] == "session_info_update" {
                        self.title = update["title"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned);
                    }
                    write(self.normalizer.update(update)?).await?;
                }
            } else if message["id"] == id {
                ensure!(
                    message.get("error").is_none(),
                    "Kimi ACP request failed: {}",
                    message["error"]
                );
                return Ok(message["result"].clone());
            }
        }
    }
    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.request(method, params).await?;
        tokio::time::timeout(Duration::from_secs(25), self.receive(id))
            .await
            .context("Kimi ACP setup request timed out")?
    }
    async fn stop(&mut self) -> Result<()> {
        self.input.take();
        match tokio::time::timeout_at(crate::official_io::cleanup_deadline(), self.child.wait())
            .await
        {
            Ok(status) => {
                let status = status?;
                crate::official_io::finished(status.code()).await?;
                ensure!(status.success(), "Kimi ACP exited unsuccessfully");
            }
            Err(_) => {
                self.child.kill().await?;
                let status = self.child.wait().await?;
                crate::official_io::finished(status.code()).await?;
                anyhow::bail!("Kimi ACP did not exit after closing stdin");
            }
        }
        Ok(())
    }
}
#[cfg(unix)]
async fn cancelled() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {_=term.recv()=>{},_=tokio::signal::ctrl_c()=>{},_=crate::official_io::cancelled()=>{}}
}
#[cfg(not(unix))]
async fn cancelled() {
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = crate::official_io::cancelled() => {} }
}

pub async fn run(native: &[String], model: Option<&str>, session: Option<&str>) -> Result<i32> {
    let cwd = std::env::current_dir()?.canonicalize()?;
    if let Some(id) = session {
        crate::sessions::inspect("kimi-code", id, &cwd).await?;
    }
    let bytes = crate::official_io::input().await?;
    let prompt = String::from_utf8(bytes)?;
    ensure!(!prompt.trim().is_empty(), "prompt is empty");
    let mut rpc = Rpc::start(native).await?;
    let result: Result<(i32, Vec<StreamRecord>)> = async {
        let initialized = rpc.call("initialize", json!({
            "protocolVersion": 1,
            "clientInfo": {"name": "wakeit", "version": env!("CARGO_PKG_VERSION")},
            "clientCapabilities": {}
        })).await?;
        ensure!(initialized["protocolVersion"] == 1, "unsupported Kimi ACP version");
        let id = if let Some(id) = session {
            rpc.call("session/resume", json!({
                "sessionId": id, "cwd": rpc.cwd, "mcpServers": []
            })).await?;
            id.to_owned()
        } else {
            rpc.call("session/new", json!({"cwd": rpc.cwd, "mcpServers": []})).await?
                ["sessionId"].as_str().context("Kimi ACP session ID missing")?.to_owned()
        };
        wakeit_plugin_sdk::sessions::validate_id(&id)?;
        rpc.session = Some(id.clone());
        write(vec![StreamRecord::new(RecordData::Session(SessionInfo {
            version: 1, id: id.clone(), cwd: rpc.cwd.clone(), persisted: false, title: rpc.title.clone()
        }))]).await?;
        if let Some(model) = model {
            rpc.call("session/set_config_option", json!({
                "sessionId": id, "configId": "model", "value": model
            })).await?;
        }
        rpc.call("session/set_mode", json!({"sessionId": id, "modeId": "auto"})).await?;
        let request = rpc.request("session/prompt", json!({
            "sessionId": id, "prompt": [{"type": "text", "text": prompt}]
        })).await?;
        crate::official_io::input_submitted(crate::official_io::context()).await?;
        let response = tokio::select! {
            result = rpc.receive(request) => result?,
            _ = cancelled() => {
                rpc.send(json!({
                    "jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": id}
                })).await?;
                if let Ok(response) = tokio::time::timeout_at(crate::official_io::cleanup_deadline().min(tokio::time::Instant::now() + Duration::from_millis(500)), rpc.receive(request)).await {
                    response?;
                }
                write(rpc.normalizer.finish("cancelled")?).await?;
                return Ok((130, Vec::new()));
            }
        };
        let stop = response["stopReason"].as_str().context("Kimi ACP stop reason missing")?;
        let records = rpc.normalizer.finish(stop)?;
        // Confirm native persistence and cwd before publishing the final result.
        let info = crate::sessions::inspect("kimi-code", &id, &cwd).await?;
        write(vec![StreamRecord::new(RecordData::Session(serde_json::from_value(info)?))]).await?;
        rpc.call("session/close", json!({"sessionId": id})).await?;
        if stop != "end_turn" {
            write(vec![StreamRecord::new(RecordData::Log(LogRecord {
                text: format!("Kimi ACP stopped: {stop}\n"), stream: "stderr".into(),
                diagnostic: true, recording_issue: None
            }))]).await?;
        }
        Ok((if stop == "end_turn" {0} else {1}, records))
    }.await;
    let stopped = rpc.stop().await;
    if result.is_err() {
        write(rpc.normalizer.finish("error")?).await?;
    }
    let (code, records) = result?;
    stopped?;
    write(records).await?;
    Ok(code)
}
