//! Kimi Code 2.x local Server API. Own one loopback subprocess per execution;
//! never attach to a user's server or edit native session/config files.
use crate::trace::{
    LogRecord, RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState,
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};
use tokio_websockets::{ClientBuilder, Message};

const TEXT_LIMIT: usize = 256 * 1024;

struct Server {
    child: Child,
    client: reqwest::Client,
    base: String,
    token: String,
}
impl Server {
    async fn start(native: &[String]) -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let native = if native.is_empty() {
            vec!["kimi".into()]
        } else {
            native.to_vec()
        };
        let mut spec = crate::config::CommandSpec {
            command: native[0].clone(),
            args: native[1..].iter().cloned().map(Into::into).collect(),
            ..Default::default()
        };
        crate::config::normalize_native_command(&mut spec)?;
        let mut command = Command::new(&spec.command);
        for arg in spec.args {
            if let crate::config::CommandArg::Literal(arg) = arg {
                command.arg(arg);
            }
        }
        let child = command
            .args([
                "web",
                "--no-open",
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            // The native ready banner contains its bearer token. Never forward it.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("start Kimi local event server")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        let mut server = Self {
            child,
            client,
            base: String::new(),
            token: String::new(),
        };
        let pid = server.child.id().context("Kimi process ID missing")?;
        let home = crate::kimi_adapter::kimi_home()?;
        let ready = tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                ensure!(
                    server.child.try_wait()?.is_none(),
                    "Kimi server exited before becoming ready"
                );
                if let Ok(mut entries) = tokio::fs::read_dir(home.join("server/instances")).await {
                    while let Some(entry) = entries.next_entry().await? {
                        let Ok(bytes) = tokio::fs::read(entry.path()).await else {
                            continue;
                        };
                        if bytes.len() > 8192 {
                            continue;
                        }
                        let Ok(info) = serde_json::from_slice::<Value>(&bytes) else {
                            continue;
                        };
                        if info["pid"].as_u64() != Some(pid as u64) || info["host"] != "127.0.0.1" {
                            continue;
                        }
                        let Some(port) = info["port"].as_u64().filter(|v| *v > 0 && *v <= 65535)
                        else {
                            continue;
                        };
                        let token = tokio::fs::read_to_string(home.join("server.token")).await?;
                        ensure!(
                            !token.trim().is_empty() && token.len() < 4096,
                            "invalid Kimi server credential"
                        );
                        server.base = format!("http://127.0.0.1:{port}");
                        server.token = token.trim().into();
                        if let Ok(meta) = server.api("GET", "/meta", None).await {
                            ensure!(
                                meta["server_id"].as_str().is_some_and(|v| !v.is_empty())
                                    && meta["dangerous_bypass_auth"] == false,
                                "Kimi server identity/authentication mismatch"
                            );
                            return Ok::<(), anyhow::Error>(());
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .context("Kimi local server startup timed out")?;
        ready?;
        Ok(server)
    }
    async fn api(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self
            .client
            .request(method.parse()?, format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("Kimi local API connection failed")?;
        let status = response.status();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            bytes.extend(chunk?);
            ensure!(
                bytes.len() <= 2 * 1024 * 1024,
                "Kimi API response exceeded limit"
            );
        }
        let value: Value = serde_json::from_slice(&bytes).context("invalid Kimi API response")?;
        ensure!(
            status.is_success() && value["code"] == 0,
            "Kimi API {path} failed (HTTP {status}, code {})",
            value["code"]
        );
        Ok(value["data"].clone())
    }
    async fn close(&mut self) -> Result<()> {
        let _ = self.api("POST", "/shutdown", Some(json!({}))).await;
        match tokio::time::timeout(Duration::from_secs(8), self.child.wait()).await {
            Ok(status) => ensure!(status?.success(), "Kimi local server exited unsuccessfully"),
            Err(_) => {
                self.child.kill().await?;
                self.child.wait().await?;
                bail!("Kimi local server did not shut down cleanly");
            }
        }
        Ok(())
    }
}

async fn write(records: Vec<StreamRecord>) -> Result<()> {
    let mut output = tokio::io::stdout();
    for record in records {
        let mut bytes = serde_json::to_vec(&record)?;
        ensure!(
            bytes.len() <= RECORD_LIMIT,
            "Kimi stream record exceeded limit"
        );
        bytes.push(b'\n');
        output.write_all(&bytes).await?;
    }
    output.flush().await?;
    Ok(())
}

pub async fn run(
    native: &[String],
    model: Option<&str>,
    session: Option<&str>,
    title: Option<&str>,
) -> Result<i32> {
    let cwd = std::fs::canonicalize(std::env::current_dir()?)?;
    if let Some(id) = session {
        crate::native_sessions::inspect("kimi-code", id, &cwd).await?;
    }
    let mut prompt = Vec::new();
    tokio::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut prompt)
        .await?;
    ensure!(prompt.len() <= 1024 * 1024, "prompt exceeds 1 MiB");
    let prompt = String::from_utf8(prompt).context("prompt is not UTF-8")?;
    ensure!(!prompt.trim().is_empty(), "prompt is empty");
    if let Some(title) = title {
        ensure!(
            title.len() <= 200 && !title.chars().any(char::is_control),
            "invalid session title"
        );
    }
    let mut server = Server::start(native).await?;
    let mut actual_id = None;
    let outcome = async {
        let info = if let Some(id) = session { server.api("GET", &format!("/sessions/{id}"), None).await? } else {
            let mut body = json!({"metadata":{"cwd":cwd}});
            if let Some(title) = title.filter(|v| !v.is_empty()) { body["title"] = json!(title); }
            server.api("POST", "/sessions", Some(body)).await?
        };
        let id = info["id"].as_str().context("Kimi session ID missing")?.to_owned(); crate::sessions::validate_id(&id)?;
        ensure!(session.is_none_or(|expected| expected == id), "Kimi returned a different session ID");
        let reported = info["metadata"]["cwd"].as_str().context("Kimi session directory missing")?;
        ensure!(std::fs::canonicalize(reported)? == cwd, "Kimi session directory mismatch");
        if session.is_none() && let Some(title) = title.filter(|v| !v.is_empty()) { ensure!(info["title"] == title, "Kimi did not persist the requested session title"); }
        actual_id = Some(id.clone());
        if session.is_some() { server.api("POST", &format!("/sessions/{id}/profile"), Some(json!({"agent_config":{}}))).await?; }
        write(vec![StreamRecord::new(RecordData::Session(crate::sessions::SessionInfo { version:1,id:id.clone(),cwd:cwd.to_string_lossy().into_owned(),persisted:false,title:info["title"].as_str().filter(|v|!v.is_empty()).map(Into::into) }))]).await?;
        let ws_url = format!("{}/api/v1/ws", server.base.replace("http:", "ws:"));
        let (mut ws, _) = ClientBuilder::new().uri(&ws_url)?.add_header(axum::http::header::AUTHORIZATION, format!("Bearer {}",server.token).parse()?).connect().await.context("connect Kimi event stream")?;
        ws.send(Message::text(json!({"type":"subscribe","id":"wakeit","payload":{"session_ids":[id],"agent_filter":BTreeMap::from([(id.clone(), vec!["main"])])}}).to_string())).await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(message) = ws.next().await {
                let message = message?; let Some(text) = message.as_text() else { continue };
                let event: Value = serde_json::from_str(text)?;
                if event["type"] == "ack" && event["id"] == "wakeit" { ensure!(event["code"] == 0 && event["payload"]["accepted"].as_array().is_some_and(|v|v.iter().any(|item|item==&id)), "Kimi event subscription rejected"); return Ok::<(),anyhow::Error>(()); }
            }
            bail!("Kimi event stream closed before subscription")
        }).await.context("Kimi event subscription timed out")??;
        let model = match model { Some(model) => Some(model.to_owned()), None if session.is_some() => None, None => server.api("GET", "/config", None).await?["default_model"].as_str().map(Into::into) };
        let mut body = json!({"content":[{"type":"text","text":prompt}],"permission_mode":"auto"});
        if let Some(model) = model { body["model"] = json!(model); }
        // Subscribe before submitting and drain concurrently: a full WebSocket
        // must not block the HTTP response, and fast events must not be lost.
        let prompt_path = format!("/sessions/{id}/prompts");
        let submitted = server.api("POST", &prompt_path, Some(body));
        tokio::pin!(submitted);
        let mut accepted = false; let mut normalizer = LiveNormalizer::default();
        while !accepted || !normalizer.ended {
            tokio::select! {
                response = &mut submitted, if !accepted => { response?; accepted = true; }
                message = ws.next(), if !normalizer.ended => {
                    let message = message.context("Kimi event stream closed during execution")??;
                    let Some(text) = message.as_text() else { continue };
                    ensure!(text.len() <= RECORD_LIMIT, "Kimi event exceeded limit");
                    let event: Value = serde_json::from_str(text).context("invalid Kimi event JSON")?;
                    if event["type"] == "ping" {
                        ws.send(Message::text(json!({"type":"pong","payload":event["payload"]}).to_string())).await?;
                        continue;
                    }
                    if event["session_id"] != id { continue; }
                    write(normalizer.accept(&event)?).await?;
                }
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), ws.close()).await;
        Ok::<_, anyhow::Error>(normalizer)
    }.await;
    if outcome.as_ref().map_or(true, |n| n.failed)
        && let Some(id) = &actual_id
    {
        let _ = server
            .api("POST", &format!("/sessions/{id}:abort"), Some(json!({})))
            .await;
    }
    let closed = server.close().await;
    let normalizer = outcome?;
    closed?;
    // Publish the final report only after the native turn and process both end.
    if !normalizer.failed {
        write(normalizer.result()).await?;
    }
    Ok(if normalizer.failed { 1 } else { 0 })
}

#[derive(Default)]
pub struct LiveNormalizer {
    nodes: BTreeMap<String, TraceNode>,
    message: Option<String>,
    next_message: u64,
    retained_input: usize,
    turn: Option<u64>,
    final_text: Option<String>,
    seen: BTreeSet<(u64, String)>,
    pub ended: bool,
    pub failed: bool,
}
impl LiveNormalizer {
    pub fn accept(&mut self, event: &Value) -> Result<Vec<StreamRecord>> {
        let kind = event["type"].as_str().context("Kimi event type missing")?;
        let p = &event["payload"];
        if p["agentId"].as_str().is_some_and(|id| id != "main") || self.ended {
            return Ok(vec![]);
        }
        if event["volatile"] != true
            && let Some(seq) = event["seq"].as_u64().filter(|v| *v > 0)
        {
            ensure!(self.seen.len() < 100_000, "Kimi event count exceeded limit");
            if !self.seen.insert((seq, kind.into())) {
                return Ok(vec![]);
            }
        }
        let source_time = event["timestamp"]
            .as_str()
            .and_then(|v| v.parse::<DateTime<Utc>>().ok())
            .or_else(|| p["time"].as_i64().and_then(DateTime::from_timestamp_millis));
        let at = source_time.unwrap_or_else(Utc::now);
        let mut output = vec![];
        match kind {
            "turn.started" => {
                ensure!(self.turn.is_none(), "unexpected extra Kimi turn");
                self.turn = p["turnId"].as_u64();
            }
            "turn.step.started" => {
                self.finish_message(TraceState::Succeeded, source_time, &mut output);
            }
            "assistant.delta" => {
                let delta = p["delta"].as_str().context("Kimi message delta missing")?;
                if self.message.is_none() {
                    ensure!(
                        self.nodes.len() < 10_000,
                        "Kimi trace node count exceeded limit"
                    );
                    self.next_message += 1;
                    self.message = Some(format!(
                        "kimi-message:{}:{}",
                        p["turnId"], self.next_message
                    ));
                }
                let id = self.message.clone().unwrap();
                let node = self.nodes.entry(id.clone()).or_insert_with(|| TraceNode {
                    id,
                    parent_id: None,
                    kind: TraceKind::Message,
                    status: TraceState::Running,
                    title: "Message".into(),
                    input: Value::Null,
                    output: json!(""),
                    occurred_at: at,
                    started_at: source_time,
                    finished_at: None,
                });
                let text = node.output.as_str().unwrap_or_default();
                ensure!(
                    text.len() + delta.len() <= TEXT_LIMIT,
                    "Kimi message exceeded limit"
                );
                node.output = json!(format!("{text}{delta}"));
                node.occurred_at = at;
                self.final_text = node.output.as_str().map(Into::into);
                output.push(StreamRecord::new(RecordData::Trace(node.clone())));
            }
            "tool.call.started" => {
                self.finish_message(TraceState::Succeeded, source_time, &mut output);
                self.final_text = None;
                let id = tool_id(p)?;
                if self
                    .nodes
                    .get(&id)
                    .is_some_and(|n| n.status != TraceState::Running)
                {
                    return Ok(output);
                }
                let name = p["name"].as_str().context("Kimi tool name missing")?;
                ensure!(
                    self.nodes.len() < 10_000 && serde_json::to_vec(&p["args"])?.len() < TEXT_LIMIT,
                    "Kimi tool input exceeded limit"
                );
                if self.nodes.contains_key(&id) {
                    return Ok(output);
                }
                let retained = serde_json::to_vec(&p["args"])?.len();
                ensure!(
                    self.retained_input + retained <= 8 * 1024 * 1024,
                    "Kimi active tool input exceeded limit"
                );
                self.retained_input += retained;
                let node = TraceNode {
                    id: id.clone(),
                    parent_id: None,
                    kind: if matches!(name, "Bash" | "Shell" | "Terminal") {
                        TraceKind::Command
                    } else {
                        TraceKind::Tool
                    },
                    status: TraceState::Running,
                    title: name.chars().take(512).collect(),
                    input: p["args"].clone(),
                    output: Value::Null,
                    occurred_at: at,
                    started_at: source_time,
                    finished_at: None,
                };
                self.nodes.insert(id, node.clone());
                output.push(StreamRecord::new(RecordData::Trace(node)));
            }
            "tool.result" => {
                let id = tool_id(p)?;
                let node = self
                    .nodes
                    .get_mut(&id)
                    .context("Kimi tool result without start")?;
                if node.status != TraceState::Running {
                    return Ok(output);
                }
                ensure!(
                    serde_json::to_vec(&p["output"])?.len() < TEXT_LIMIT,
                    "Kimi tool output exceeded limit"
                );
                node.output = p["output"].clone();
                node.status = if p["isError"] == true {
                    TraceState::Failed
                } else {
                    TraceState::Succeeded
                };
                node.occurred_at = at;
                node.finished_at = source_time;
                output.push(StreamRecord::new(RecordData::Trace(node.clone())));
                self.retained_input = self
                    .retained_input
                    .saturating_sub(serde_json::to_vec(&node.input)?.len());
                node.input = Value::Null;
                node.output = Value::Null;
            }
            "turn.step.completed" => {
                self.finish_message(TraceState::Succeeded, source_time, &mut output)
            }
            "turn.ended" => {
                self.ended = true;
                self.failed |= p["reason"] != "completed"
                    || self
                        .nodes
                        .values()
                        .any(|n| n.kind != TraceKind::Message && n.status == TraceState::Running);
                self.finish_message(
                    if self.failed {
                        TraceState::Failed
                    } else {
                        TraceState::Succeeded
                    },
                    source_time,
                    &mut output,
                );
                if self.failed {
                    self.final_text = None;
                    output.push(StreamRecord::new(RecordData::Log(LogRecord {
                        text: format!(
                            "Kimi execution ended: {} ({})\n",
                            p["reason"], p["error"]["code"]
                        ),
                        stream: "stderr".into(),
                        diagnostic: true,
                    })));
                }
            }
            "error" => {
                self.failed = true;
                self.final_text = None;
            }
            // Hidden thinking, native configuration and unrelated housekeeping
            // are not user-visible Agent messages or fabricated tool steps.
            _ => {}
        }
        Ok(output)
    }
    fn finish_message(
        &mut self,
        status: TraceState,
        time: Option<DateTime<Utc>>,
        output: &mut Vec<StreamRecord>,
    ) {
        if let Some(id) = self.message.take()
            && let Some(node) = self.nodes.get_mut(&id)
        {
            node.status = status;
            node.finished_at = time;
            if let Some(time) = time {
                node.occurred_at = time
            };
            output.push(StreamRecord::new(RecordData::Trace(node.clone())));
            node.output = Value::Null;
        }
    }
    pub fn result(&self) -> Vec<StreamRecord> {
        if !self.ended || self.failed {
            return vec![];
        }
        self.final_text
            .as_ref()
            .filter(|v| !v.trim().is_empty())
            .map(|v| vec![StreamRecord::new(RecordData::Result(json!(v)))])
            .unwrap_or_default()
    }
}
fn tool_id(p: &Value) -> Result<String> {
    let id = p["toolCallId"].as_str().context("Kimi tool ID missing")?;
    ensure!(!id.is_empty() && id.len() <= 200, "invalid Kimi tool ID");
    Ok(format!("kimi-tool:{}:{id}", p["turnId"]))
}

pub async fn rename(id: &str, title: &str, cwd: &Path) -> Result<()> {
    crate::native_sessions::inspect("kimi-code", id, cwd).await?;
    let mut server = Server::start(&[]).await?;
    let result = async {
        let v = server
            .api(
                "POST",
                &format!("/sessions/{id}/profile"),
                Some(json!({"title":title})),
            )
            .await?;
        ensure!(
            v["id"] == id && v["title"] == title,
            "Kimi session title was not persisted"
        );
        Ok::<(), anyhow::Error>(())
    }
    .await;
    let closed = server.close().await;
    result?;
    closed?;
    ensure!(
        crate::native_sessions::inspect("kimi-code", id, cwd).await?["title"] == title,
        "Kimi saved title does not match"
    );
    Ok(())
}
