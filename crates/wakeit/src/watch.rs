//! Read-only observation of a single Run. No scheduling or execution requests live here.
use crate::{protocol, trace::TraceNode};
use anyhow::{Context, Result};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Write},
    time::Duration,
};
use tokio::time::Instant;
use uuid::Uuid;

pub fn timeout(value: &str) -> std::result::Result<Duration, String> {
    let duration = humantime::parse_duration(value).map_err(|e| e.to_string())?;
    if duration.is_zero() || Instant::now().checked_add(duration).is_none() {
        return Err("timeout must be a positive, representable duration".into());
    }
    Ok(duration)
}

#[derive(Clone, Copy)]
struct Timing {
    poll: Duration,
    request: Duration,
    reconnect: Duration,
    backoff: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(1),
            request: Duration::from_secs(5),
            reconnect: Duration::from_secs(30),
            backoff: Duration::from_secs(1),
        }
    }
}
#[derive(Debug)]
struct Failure {
    code: i32,
    message: String,
}
impl Failure {
    fn query(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
}
impl From<io::Error> for Failure {
    fn from(error: io::Error) -> Self {
        Self::query(format!("write observation output: {error}"))
    }
}
#[derive(Clone, Deserialize)]
struct RunView {
    id: Uuid,
    state: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    result: Value,
    #[serde(default)]
    log_truncated: bool,
}
impl RunView {
    fn parse(value: Value, id: Uuid) -> std::result::Result<Self, Failure> {
        let run: Self =
            serde_json::from_value(value).map_err(|_| Failure::query("invalid Run response"))?;
        if run.id != id
            || !(protocol::terminal(&run.state)
                || matches!(
                    run.state.as_str(),
                    "queued" | "starting" | "running" | "unknown"
                ))
        {
            return Err(Failure::query("invalid Run identity or state"));
        }
        Ok(run)
    }
}
#[derive(Deserialize)]
struct Event {
    id: i64,
    seq: i64,
    run_id: Uuid,
    kind: String,
    data: Value,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    execution_id: Option<Uuid>,
    #[serde(default)]
    original_seq: Option<i64>,
}

struct Reporter<W> {
    output: W,
    id: Uuid,
    json: bool,
    line_open: bool,
    status: Option<(String, Option<String>)>,
    notices: HashSet<String>,
    traces: HashMap<String, (i64, TraceNode)>,
}
impl<W: Write> Reporter<W> {
    fn new(output: W, id: Uuid, json: bool) -> Self {
        Self {
            output,
            id,
            json,
            line_open: false,
            status: None,
            notices: HashSet::new(),
            traces: HashMap::new(),
        }
    }
    fn raw(&mut self, text: &str) -> io::Result<()> {
        if !text.is_empty() {
            self.output.write_all(text.as_bytes())?;
            self.line_open = !text.ends_with('\n');
        }
        Ok(())
    }
    fn line(&mut self, text: &str) -> io::Result<()> {
        if self.line_open {
            self.raw("\n")?;
        }
        self.raw(text)?;
        if !text.ends_with('\n') {
            self.raw("\n")?;
        }
        Ok(())
    }
    fn emit(&mut self, kind: &str, data: Value, event: Option<&Event>) -> io::Result<()> {
        let mut record = json!({"type":kind,"run_id":self.id,"data":data});
        if let Some(event) = event {
            record["event_id"] = json!(event.id);
            record["seq"] = json!(event.seq);
            if let Some(node) = &event.node_id {
                record["node_id"] = json!(node);
            }
            if let Some(execution) = event.execution_id {
                record["execution_id"] = json!(execution);
            }
            if let Some(seq) = event.original_seq {
                record["original_seq"] = json!(seq);
            }
        }
        serde_json::to_writer(&mut self.output, &record)?;
        self.output.write_all(b"\n")
    }
    fn state(&mut self, run: &RunView) -> io::Result<()> {
        let status = (run.state.clone(), run.reason.clone());
        if self.status.as_ref() != Some(&status) {
            if self.json {
                self.emit(
                    "status",
                    json!({"state":run.state,"reason":run.reason}),
                    None,
                )?;
            } else {
                self.line(&format!(
                    "Run {}: {}{}",
                    self.id,
                    run.state,
                    run.reason
                        .as_ref()
                        .map(|r| format!(" — {r}"))
                        .unwrap_or_default()
                ))?;
            }
            self.status = Some(status);
        }
        if run.state == "unknown" {
            self.notice(
                "unknown",
                "Run status is uncertain; waiting for reconciliation.",
                None,
            )?;
        }
        if run.log_truncated {
            self.notice("truncated", "Execution output was truncated.", None)?;
        }
        Ok(())
    }
    fn notice(&mut self, status: &str, message: &str, event: Option<&Event>) -> io::Result<()> {
        if self.notices.insert(format!(
            "{}:{status}",
            event.and_then(|e| e.node_id.as_deref()).unwrap_or("")
        )) {
            if self.json {
                self.emit("notice", json!({"status":status,"message":message}), event)?;
            } else {
                self.line(message)?;
            }
        }
        Ok(())
    }
    fn delta(&mut self, previous: Option<&Value>, value: &Value, label: &str) -> io::Result<()> {
        if value.is_null() || previous == Some(value) {
            return Ok(());
        }
        if let Some(text) = value.as_str() {
            if let Some(old) = previous.and_then(Value::as_str)
                && let Some(suffix) = text.strip_prefix(old)
            {
                return self.raw(suffix);
            }
            if previous.is_some_and(|v| !v.is_null()) {
                self.line(&format!("{label} updated:"))?;
            } else {
                self.line(&format!("{label}:"))?;
            }
            self.raw(text)
        } else {
            self.line(&format!("{label}:"))?;
            self.line(&serde_json::to_string_pretty(value)?)
        }
    }
    fn trace(&mut self, event: &Event) -> std::result::Result<(), Failure> {
        let mut data = event.data.clone();
        if let Some(object) = data.as_object_mut() {
            object.remove("event_time");
        }
        let node: TraceNode =
            serde_json::from_value(data).map_err(|_| Failure::query("invalid Trace response"))?;
        let key = format!(
            "{}\0{}\0{}",
            event.execution_id.unwrap_or(event.run_id),
            event.node_id.as_deref().unwrap_or(""),
            node.id
        );
        let old = self.traces.get(&key).cloned();
        if old.as_ref().is_some_and(|(seq, old)| {
            *seq >= event.seq
                || (old.status != crate::trace::TraceState::Running
                    && node.status == crate::trace::TraceState::Running)
        }) {
            return Ok(());
        }
        if self.json {
            self.emit("trace", event.data.clone(), Some(event))?;
        } else {
            if old.as_ref().is_none_or(|(_, previous)| {
                previous.status != node.status || previous.title != node.title
            }) {
                let kind = match node.kind {
                    crate::trace::TraceKind::Message => "message",
                    crate::trace::TraceKind::Tool => "tool",
                    crate::trace::TraceKind::Command => "command",
                };
                let status = serde_json::to_value(&node.status).expect("Trace state");
                self.line(&format!(
                    "{}[{kind} {}] {}",
                    event
                        .node_id
                        .as_ref()
                        .map(|id| format!("[{id}] "))
                        .unwrap_or_default(),
                    status.as_str().unwrap_or("unknown"),
                    node.title
                ))?;
            }
            if let Some(id) = &event.node_id
                && old
                    .as_ref()
                    .is_some_and(|(_, n)| n.input != node.input || n.output != node.output)
            {
                self.line(&format!("[{id}]"))?;
            }
            self.delta(old.as_ref().map(|(_, n)| &n.input), &node.input, "Input")?;
            self.delta(old.as_ref().map(|(_, n)| &n.output), &node.output, "Output")?;
        }
        self.traces.insert(key, (event.seq, node));
        Ok(())
    }
    fn event(&mut self, event: &Event) -> std::result::Result<(), Failure> {
        match event.kind.as_str() {
            "log" => {
                let text = event.data["text"]
                    .as_str()
                    .ok_or_else(|| Failure::query("invalid log response"))?;
                if self.json {
                    self.emit("log", event.data.clone(), Some(event))?;
                } else {
                    if let Some(node) = &event.node_id {
                        self.line(&format!("[{node}]"))?;
                    }
                    self.raw(text)?;
                }
            }
            "trace" => self.trace(event)?,
            "state" | "node_state" | "node" => {
                let state = event.data["state"]
                    .as_str()
                    .ok_or_else(|| Failure::query("invalid state event"))?;
                if self.json {
                    self.emit(
                        "status",
                        json!({"state":state,"reason":event.data.get("reason")}),
                        Some(event),
                    )?;
                } else {
                    self.line(&format!(
                        "{}State: {state}",
                        event
                            .node_id
                            .as_deref()
                            .or_else(|| event.data["node_id"].as_str())
                            .map(|id| format!("[{id}] "))
                            .unwrap_or_default()
                    ))?;
                }
            }
            "progress" => {
                let message = event.data["message"]
                    .as_str()
                    .ok_or_else(|| Failure::query("invalid progress event"))?;
                if self.json {
                    self.emit(
                        "notice",
                        json!({"status":"progress","message":message}),
                        Some(event),
                    )?;
                } else {
                    self.line(message)?;
                }
            }
            "trace_status" | "truncated" => {
                let status = event.data["status"].as_str().unwrap_or("truncated");
                let message = match status {
                    "expired" => "Execution output has expired.",
                    "truncated" => "Execution output was truncated.",
                    "incomplete" => "Some execution events were not recorded.",
                    _ => "Execution content status changed.",
                };
                self.notice(status, message, Some(event))?;
            }
            // Run snapshots, credentials and session metadata are not observation output.
            _ => {}
        }
        Ok(())
    }
    fn finish(&mut self, run: Option<&RunView>, failure: Option<&Failure>) -> io::Result<i32> {
        let code = failure.map(|e| e.code).unwrap_or_else(|| {
            if run.is_some_and(|r| matches!(r.state.as_str(), "succeeded" | "warning")) {
                0
            } else {
                1
            }
        });
        if let Some(error) = failure {
            if self.json {
                self.emit(
                    "error",
                    json!({"message":error.message,"exit_code":code}),
                    None,
                )?;
            } else {
                self.line(&format!("Watch stopped: {}", error.message))?;
            }
        }
        if self.json {
            self.emit("summary", json!({"state":run.map(|r| &r.state),"terminal":run.is_some_and(|r| protocol::terminal(&r.state)),"reason":run.and_then(|r| r.reason.as_ref()),"result":run.map(|r| &r.result),"exit_code":code}), None)?;
        } else if let Some(run) = run {
            self.line(&format!("Run {}: {}", self.id, run.state))?;
            if let Some(reason) = &run.reason {
                self.line(reason)?;
            }
            if failure.is_none() {
                if run.result.is_null() || run.result.as_str() == Some("") {
                    self.line("No result returned.")?;
                } else if run.result["truncated"] == true && run.result["preview"].is_string() {
                    self.line("Result was truncated.")?;
                    self.line(run.result["preview"].as_str().unwrap_or_default())?;
                } else if let Some(text) = run.result.as_str() {
                    self.line(text)?;
                } else {
                    self.line(&serde_json::to_string_pretty(&run.result)?)?;
                }
            }
        }
        self.output.flush()?;
        Ok(code)
    }
}
struct Observer<W> {
    http: reqwest::Client,
    gateway: String,
    workspace: Option<Uuid>,
    token: String,
    reporter: Reporter<W>,
    last: Option<RunView>,
    cursor: i64,
    timing: Timing,
}
impl<W: Write> Observer<W> {
    async fn read(&mut self, path: &str) -> std::result::Result<Value, Failure> {
        let started = Instant::now();
        let mut attempt = 0u32;
        loop {
            let remaining = self.timing.reconnect.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(Failure::query(
                    "connection did not recover within 30 seconds",
                ));
            }
            let response = self
                .http
                .get(match self.workspace {
                    Some(workspace) => {
                        format!("{}/api/v1/workspaces/{workspace}{path}", self.gateway)
                    }
                    None => format!("{}/api/v1{path}", self.gateway),
                })
                .bearer_auth(self.token.trim())
                .timeout(self.timing.request.min(remaining))
                .send()
                .await;
            let message = match response {
                Ok(response) => {
                    let status = response.status();
                    if status.is_success() {
                        match response.json().await {
                            Ok(value) => return Ok(value),
                            Err(error) if error.is_timeout() || error.is_body() => {
                                "Gateway response interrupted".into()
                            }
                            Err(_) => return Err(Failure::query("invalid JSON response")),
                        }
                    } else {
                        if status != StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() {
                            return Err(Failure::query(format!("Gateway returned HTTP {status}")));
                        }
                        format!("Gateway returned HTTP {status}")
                    }
                }
                Err(error) if error.is_builder() => {
                    return Err(Failure::query("invalid Gateway request"));
                }
                Err(_) => "Gateway connection failed".into(),
            };
            self.reporter.notice(
                "reconnecting",
                "Connection interrupted; reconnecting.",
                None,
            )?;
            let delay = self
                .timing
                .backoff
                .saturating_mul(1u32 << attempt.min(3))
                .min(self.timing.backoff.saturating_mul(5));
            let remaining = self.timing.reconnect.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(Failure::query(format!(
                    "{message}; connection did not recover within 30 seconds"
                )));
            }
            tokio::time::sleep(delay.min(remaining)).await;
            attempt = attempt.saturating_add(1);
        }
    }
    async fn state(&mut self) -> std::result::Result<(), Failure> {
        let value = self.read(&format!("/runs/{}", self.reporter.id)).await?;
        let run = RunView::parse(value, self.reporter.id)?;
        self.reporter.state(&run)?;
        self.last = Some(run);
        Ok(())
    }
    async fn drain(&mut self, until_empty: bool) -> std::result::Result<(), Failure> {
        loop {
            let value = self
                .read(&format!(
                    "/runs/{}/events?after={}",
                    self.reporter.id, self.cursor
                ))
                .await?;
            let events: Vec<Event> = serde_json::from_value(value)
                .map_err(|_| Failure::query("invalid events response"))?;
            if events.len() > 500 {
                return Err(Failure::query("event batch exceeds 500 records"));
            }
            let count = events.len();
            let before = self.cursor;
            for event in events {
                if event.run_id != self.reporter.id
                    || event.id <= 0
                    || (event.seq <= 0
                        && !(event.seq == -1
                            && event.kind == "trace_status"
                            && event.data["status"] == "expired"))
                {
                    return Err(Failure::query("invalid event identity or cursor"));
                }
                if event.id <= before {
                    continue;
                }
                if event.id <= self.cursor {
                    return Err(Failure::query("event batch is not ordered"));
                }
                self.reporter.event(&event)?;
                self.reporter.output.flush()?;
                self.cursor = event.id;
            }
            if count == 0 || (!until_empty && count < 500) {
                break;
            }
            if self.cursor <= before {
                return Err(Failure::query("event cursor did not advance"));
            }
        }
        Ok(())
    }
    async fn observe(&mut self) -> std::result::Result<(), Failure> {
        self.state().await?;
        loop {
            self.drain(false).await?;
            self.state().await?;
            if self
                .last
                .as_ref()
                .is_some_and(|r| protocol::terminal(&r.state))
            {
                self.drain(true).await?;
                self.state().await?;
                if self
                    .last
                    .as_ref()
                    .is_some_and(|r| protocol::terminal(&r.state))
                {
                    return Ok(());
                }
            }
            tokio::time::sleep(self.timing.poll).await;
        }
    }
}

/// Report setup failures consistently when CLI connection resolution fails before observing.
pub fn setup_failure(id: Uuid, json: bool, message: &str) -> Result<i32> {
    Reporter::new(io::stdout(), id, json)
        .finish(None, Some(&Failure::query(message)))
        .context("write watch error")
}

pub async fn run(
    gateway: &str,
    token: &str,
    id: Uuid,
    timeout: Option<Duration>,
    json: bool,
) -> Result<i32> {
    run_with_workspace(gateway, None, token, id, timeout, json).await
}
pub async fn run_scoped(
    gateway: &str,
    workspace: Uuid,
    token: &str,
    id: Uuid,
    timeout: Option<Duration>,
    json: bool,
) -> Result<i32> {
    run_with_workspace(gateway, Some(workspace), token, id, timeout, json).await
}
async fn run_with_workspace(
    gateway: &str,
    workspace: Option<Uuid>,
    token: &str,
    id: Uuid,
    timeout: Option<Duration>,
    json: bool,
) -> Result<i32> {
    let mut observer = Observer {
        http: reqwest::Client::builder().build()?,
        gateway: gateway.trim_end_matches('/').into(),
        workspace,
        token: token.into(),
        reporter: Reporter::new(io::stdout(), id, json),
        last: None,
        cursor: 0,
        timing: Timing::default(),
    };
    let outcome = tokio::select! {
        result = observer.observe() => result,
        _ = async { match timeout { Some(d) => tokio::time::sleep(d).await, None => std::future::pending().await } } => Err(Failure { code:124, message:"observation timed out".into() }),
        signal = tokio::signal::ctrl_c() => Err(match signal { Ok(()) => Failure { code:130, message:"observation interrupted".into() }, Err(_) => Failure::query("could not listen for Ctrl+C") }),
    };
    observer
        .reporter
        .finish(observer.last.as_ref(), outcome.err().as_ref())
        .context("write watch summary")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, Response},
        response::IntoResponse,
    };
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };
    use tokio_util::sync::CancellationToken;

    struct Reply {
        path: String,
        status: StatusCode,
        body: String,
        delay: Duration,
    }
    impl Reply {
        fn json(path: &str, value: Value) -> Self {
            Self {
                path: path.into(),
                status: StatusCode::OK,
                body: value.to_string(),
                delay: Duration::ZERO,
            }
        }
    }
    struct Server {
        url: String,
        script: Arc<Mutex<VecDeque<Reply>>>,
        calls: Arc<Mutex<Vec<String>>>,
        stop: CancellationToken,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.cancel();
        }
    }
    impl Server {
        async fn new(replies: Vec<Reply>) -> Self {
            let script = Arc::new(Mutex::new(VecDeque::from(replies)));
            let calls = Arc::new(Mutex::new(vec![]));
            let (s, c) = (script.clone(), calls.clone());
            let router = Router::new().fallback(move |request: Request<Body>| {
                let (s, c) = (s.clone(), c.clone());
                async move {
                    let path = request.uri().path_and_query().unwrap().as_str().to_string();
                    c.lock()
                        .unwrap()
                        .push(format!("{} {path}", request.method()));
                    let reply = s.lock().unwrap().pop_front();
                    match reply {
                        Some(reply) => {
                            assert_eq!(path, reply.path);
                            assert_eq!(request.method(), reqwest::Method::GET);
                            assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
                            tokio::time::sleep(reply.delay).await;
                            Response::builder()
                                .status(reply.status)
                                .header("content-type", "application/json")
                                .body(Body::from(reply.body))
                                .unwrap()
                        }
                        None => (StatusCode::INTERNAL_SERVER_ERROR, "unexpected request")
                            .into_response(),
                    }
                }
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let stop = CancellationToken::new();
            let shutdown = stop.clone();
            tokio::spawn(async move {
                axum::serve(listener, router)
                    .with_graceful_shutdown(shutdown.cancelled_owned())
                    .await
                    .unwrap();
            });
            Self {
                url,
                script,
                calls,
                stop,
            }
        }
        fn observer(&self, id: Uuid, json: bool) -> Observer<Vec<u8>> {
            Observer {
                http: reqwest::Client::new(),
                gateway: self.url.clone(),
                workspace: None,
                token: "fixture-key".into(),
                reporter: Reporter::new(vec![], id, json),
                last: None,
                cursor: 0,
                timing: Timing {
                    poll: Duration::from_millis(1),
                    request: Duration::from_millis(200),
                    reconnect: Duration::from_secs(1),
                    backoff: Duration::from_millis(1),
                },
            }
        }
    }
    fn run_value(id: Uuid, state: &str, result: Value) -> Value {
        json!({"id":id,"state":state,"result":result,"config":{"prompt":"DO NOT PRINT CONFIG"},"session":{"id":"DO NOT PRINT SESSION"}})
    }
    fn event(id: Uuid, n: i64, kind: &str, data: Value) -> Value {
        json!({"run_id":id,"id":n,"seq":n,"kind":kind,"data":data})
    }
    fn lines(bytes: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect()
    }
    fn node(output: Value, status: &str) -> Value {
        json!({"id":"message","kind":"message","title":"Answer","status":status,"input":null,"output":output,"occurred_at":"2026-10-16T01:00:00Z"})
    }

    #[test]
    fn durations_and_snapshot_updates_preserve_only_new_message_text() {
        assert_eq!(timeout("1h").unwrap(), Duration::from_secs(3600));
        assert!(timeout("0s").is_err());
        assert!(timeout("-1s").is_err());
        assert!(timeout("never").is_err());
        let id = Uuid::new_v4();
        let mut reporter = Reporter::new(vec![], id, false);
        for (n, text, status) in [
            (1, "你", "running"),
            (2, "你好", "running"),
            (3, "你好", "succeeded"),
            (4, "late", "running"),
        ] {
            let e: Event =
                serde_json::from_value(event(id, n, "trace", node(json!(text), status))).unwrap();
            reporter.event(&e).unwrap();
        }
        let text = String::from_utf8(reporter.output).unwrap();
        assert!(text.contains("你好"));
        assert!(!text.contains("你你好"));
        assert!(!text.contains("late"));
    }
    #[test]
    fn dag_traces_do_not_collide_and_json_preserves_child_identity() {
        let id = Uuid::new_v4();
        let execution = Uuid::new_v4();
        for json_output in [false, true] {
            let mut reporter = Reporter::new(vec![], id, json_output);
            for (seq, node_id, text) in [(1, "a", "Alpha"), (2, "b", "Beta"), (3, "a", "Alpha!")] {
                let mut value = event(id, seq, "trace", node(json!(text), "running"));
                value["node_id"] = json!(node_id);
                value["execution_id"] = json!(execution);
                value["original_seq"] = json!(seq);
                reporter
                    .event(&serde_json::from_value(value).unwrap())
                    .unwrap();
            }
            if json_output {
                let records = lines(&reporter.output);
                assert_eq!(records.len(), 3);
                assert_eq!(records[1]["node_id"], "b");
                assert_eq!(records[2]["execution_id"], execution.to_string());
                assert_eq!(records[2]["original_seq"], 3);
            } else {
                let text = String::from_utf8(reporter.output).unwrap();
                assert!(text.contains("[a]"));
                assert!(text.contains("[b]"));
                assert!(text.contains("Alpha"));
                assert!(text.contains("Beta"));
                assert_eq!(text.matches("Alpha").count(), 1);
            }
        }
    }
    #[test]
    fn retry_trace_sequence_can_restart_with_same_native_id() {
        let id = Uuid::new_v4();
        let mut reporter = Reporter::new(vec![], id, true);
        for (seq, output) in [(50, "first attempt"), (1, "second attempt")] {
            let mut value = event(id, seq, "trace", node(json!(output), "running"));
            value["node_id"] = json!("same-node");
            value["execution_id"] = json!(Uuid::new_v4());
            reporter
                .event(&serde_json::from_value(value).unwrap())
                .unwrap();
        }
        assert_eq!(lines(&reporter.output).len(), 2);
    }
    #[tokio::test]
    async fn drains_more_than_500_and_terminal_tail_then_returns_final_result() {
        let id = Uuid::new_v4();
        let path = format!("/api/v1/runs/{id}");
        let batch: Vec<_> = (1..=500)
            .map(|n| event(id, n, "log", json!({"text":format!("line {n}\n")})))
            .collect();
        let server = Server::new(vec![
            Reply::json(&path, run_value(id, "running", Value::Null)),
            Reply::json(&format!("{path}/events?after=0"), json!(batch)),
            Reply::json(&format!("{path}/events?after=500"), json!([])),
            Reply::json(&path, run_value(id, "succeeded", json!("# 报告\n\nDone"))),
            Reply::json(
                &format!("{path}/events?after=500"),
                json!([
                    event(id, 500, "log", json!({"text":"duplicate must not print"})),
                    event(id, 501, "log", json!({"text":"tail\n"}))
                ]),
            ),
            Reply::json(&format!("{path}/events?after=501"), json!([])),
            Reply::json(&path, run_value(id, "succeeded", json!("# 报告\n\nDone"))),
        ])
        .await;
        let mut observer = server.observer(id, true);
        observer.observe().await.unwrap();
        assert_eq!(
            observer
                .reporter
                .finish(observer.last.as_ref(), None)
                .unwrap(),
            0
        );
        let records = lines(&observer.reporter.output);
        let logs: Vec<_> = records.iter().filter(|v| v["type"] == "log").collect();
        assert_eq!(logs.len(), 501);
        assert_eq!(logs.last().unwrap()["event_id"], 501);
        assert_eq!(records.last().unwrap()["data"]["result"], "# 报告\n\nDone");
        let text = String::from_utf8_lossy(&observer.reporter.output);
        assert!(!text.contains("DO NOT PRINT"));
        assert!(!text.contains("duplicate must not print"));
        assert!(server.script.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn every_terminal_state_finishes_without_fabricating_a_report() {
        for state in [
            "succeeded",
            "warning",
            "failed",
            "cancelled",
            "timed_out",
            "skipped",
            "interrupted",
        ] {
            let id = Uuid::new_v4();
            let path = format!("/api/v1/runs/{id}");
            let value = run_value(id, state, json!({"answer":42}));
            let server = Server::new(vec![
                Reply::json(&path, value.clone()),
                Reply::json(&format!("{path}/events?after=0"), json!([])),
                Reply::json(&path, value.clone()),
                Reply::json(&format!("{path}/events?after=0"), json!([])),
                Reply::json(&path, value),
            ])
            .await;
            let mut observer = server.observer(id, false);
            observer.observe().await.unwrap();
            let code = observer
                .reporter
                .finish(observer.last.as_ref(), None)
                .unwrap();
            assert_eq!(
                code,
                if matches!(state, "succeeded" | "warning") {
                    0
                } else {
                    1
                }
            );
            assert!(String::from_utf8_lossy(&observer.reporter.output).contains("\"answer\": 42"));
            assert!(server.script.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn unknown_keeps_waiting_progress_is_real_and_expiry_sequence_is_valid() {
        let id = Uuid::new_v4();
        let path = format!("/api/v1/runs/{id}");
        let expired =
            json!({"id":7,"seq":-1,"run_id":id,"kind":"trace_status","data":{"status":"expired"}});
        let server = Server::new(vec![
            Reply::json(&path, run_value(id, "unknown", Value::Null)),
            Reply::json(
                &format!("{path}/events?after=0"),
                json!([
                    expired,
                    event(id, 8, "progress", json!({"message":"remote progress"}))
                ]),
            ),
            Reply::json(&path, run_value(id, "running", Value::Null)),
            Reply::json(
                &format!("{path}/events?after=8"),
                json!([
                    event(id, 9, "trace", node(json!("hello"), "running")),
                    event(id, 10, "trace", node(json!("hello!"), "succeeded")),
                    event(id, 11, "trace_status", json!({"status":"incomplete"})),
                    event(id, 12, "trace_status", json!({"status":"truncated"}))
                ]),
            ),
            Reply::json(&path, run_value(id, "succeeded", Value::Null)),
            Reply::json(&format!("{path}/events?after=12"), json!([])),
            Reply::json(&path, run_value(id, "succeeded", Value::Null)),
        ])
        .await;
        let mut observer = server.observer(id, true);
        observer.observe().await.unwrap();
        observer
            .reporter
            .finish(observer.last.as_ref(), None)
            .unwrap();
        let records = lines(&observer.reporter.output);
        for status in ["unknown", "expired", "progress", "incomplete", "truncated"] {
            assert!(
                records
                    .iter()
                    .any(|v| v["type"] == "notice" && v["data"]["status"] == status),
                "{status}"
            );
        }
        let traces: Vec<_> = records.iter().filter(|v| v["type"] == "trace").collect();
        assert_eq!(traces.len(), 2);
        assert_eq!(traces[1]["data"]["output"], "hello!");
        assert!(records.last().unwrap()["data"]["result"].is_null());
    }
    #[tokio::test]
    async fn http_errors_invalid_json_and_wrong_identity_fail_without_retry() {
        for (status, body) in [
            (StatusCode::UNAUTHORIZED, "{}".into()),
            (StatusCode::FORBIDDEN, "{}".into()),
            (StatusCode::NOT_FOUND, "{}".into()),
            (StatusCode::OK, "not json".into()),
            (
                StatusCode::OK,
                run_value(Uuid::new_v4(), "running", Value::Null).to_string(),
            ),
        ] {
            let id = Uuid::new_v4();
            let server = Server::new(vec![Reply {
                path: format!("/api/v1/runs/{id}"),
                status,
                body,
                delay: Duration::ZERO,
            }])
            .await;
            let mut observer = server.observer(id, true);
            let error = observer.observe().await.unwrap_err();
            assert_eq!(error.code, 2);
            assert_eq!(server.calls.lock().unwrap().len(), 1);
            observer
                .reporter
                .finish(observer.last.as_ref(), Some(&error))
                .unwrap();
            let records = lines(&observer.reporter.output);
            assert_eq!(records.last().unwrap()["type"], "summary");
            assert_eq!(records.last().unwrap()["data"]["terminal"], false);
        }
    }
    #[tokio::test]
    async fn retries_rate_limits_and_server_errors_but_respects_the_failure_budget() {
        let id = Uuid::new_v4();
        let path = format!("/api/v1/runs/{id}");
        let server = Server::new(vec![
            Reply {
                path: path.clone(),
                status: StatusCode::TOO_MANY_REQUESTS,
                body: "{}".into(),
                delay: Duration::ZERO,
            },
            Reply {
                path: path.clone(),
                status: StatusCode::SERVICE_UNAVAILABLE,
                body: "{}".into(),
                delay: Duration::ZERO,
            },
            Reply::json(&path, run_value(id, "running", Value::Null)),
        ])
        .await;
        let mut observer = server.observer(id, true);
        observer.state().await.unwrap();
        assert_eq!(server.calls.lock().unwrap().len(), 3);
        assert_eq!(observer.last.unwrap().state, "running");
        let server = Server::new(
            (0..100)
                .map(|_| Reply {
                    path: path.clone(),
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: "{}".into(),
                    delay: Duration::ZERO,
                })
                .collect(),
        )
        .await;
        let mut observer = server.observer(id, true);
        let start = Instant::now();
        let error = observer.state().await.unwrap_err();
        assert_eq!(error.code, 2);
        assert!(start.elapsed() < Duration::from_secs(3));
        assert!(server.calls.lock().unwrap().len() > 2);
    }
    #[tokio::test]
    async fn request_timeout_retries_without_replaying_processed_events() {
        let id = Uuid::new_v4();
        let path = format!("/api/v1/runs/{id}");
        let mut slow = Reply::json(&path, run_value(id, "running", Value::Null));
        slow.delay = Duration::from_millis(300);
        let server = Server::new(vec![
            slow,
            Reply::json(&path, run_value(id, "running", Value::Null)),
        ])
        .await;
        let mut observer = server.observer(id, true);
        observer.timing.request = Duration::from_millis(100);
        observer.state().await.unwrap();
        assert_eq!(server.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn summary_distinguishes_wait_timeout_interruption_and_absent_results() {
        for code in [2, 124, 130] {
            let id = Uuid::new_v4();
            let mut reporter = Reporter::new(vec![], id, true);
            let run = RunView::parse(run_value(id, "unknown", Value::Null), id).unwrap();
            assert_eq!(
                reporter
                    .finish(
                        Some(&run),
                        Some(&Failure {
                            code,
                            message: "stopped".into()
                        })
                    )
                    .unwrap(),
                code
            );
            let records = lines(&reporter.output);
            let last = records.last().unwrap();
            assert_eq!(last["data"]["state"], "unknown");
            assert_eq!(last["data"]["terminal"], false);
            assert_eq!(last["data"]["exit_code"], code);
        }
        for result in [
            Value::Null,
            json!(""),
            json!({"truncated":true,"preview":"partial report"}),
        ] {
            let id = Uuid::new_v4();
            let mut reporter = Reporter::new(vec![], id, false);
            let run = RunView::parse(run_value(id, "succeeded", result.clone()), id).unwrap();
            reporter.finish(Some(&run), None).unwrap();
            let text = String::from_utf8(reporter.output).unwrap();
            assert!(text.contains(if result.is_object() {
                "Result was truncated."
            } else {
                "No result returned."
            }));
        }
    }
}
