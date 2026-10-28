use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
};
use serde_json::{Value, json};
use std::{
    io::Read,
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const WORKSPACE: &str = "bf14b225-f7b8-4327-8c9e-05818754d48d";

struct Server {
    url: String,
    calls: Arc<Mutex<Vec<String>>>,
    stop: CancellationToken,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
async fn server<F>(respond: F) -> Server
where
    F: Fn(&str) -> (StatusCode, String, Duration) + Send + Sync + 'static,
{
    let calls = Arc::new(Mutex::new(vec![]));
    let recorded = calls.clone();
    let respond = Arc::new(respond);
    let router = Router::new().fallback(move |request: Request<Body>| {
        let calls = recorded.clone();
        let respond = respond.clone();
        async move {
            let path = request.uri().path_and_query().unwrap().as_str().to_string();
            assert!(path.starts_with(&format!("/api/v1/workspaces/{WORKSPACE}/runs/")));
            calls
                .lock()
                .unwrap()
                .push(format!("{} {path}", request.method()));
            assert_eq!(request.method(), reqwest::Method::GET);
            assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
            let (status, body, delay) = respond(&path);
            tokio::time::sleep(delay).await;
            (status, [("content-type", "application/json")], body).into_response()
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
    Server { url, calls, stop }
}
fn start(server: &Server, id: Uuid, args: &[&str], home: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .env_remove("WAKEIT_GATEWAY")
        .env_remove("WAKEIT_TOKEN")
        .env_remove("WAKEIT_TOKEN_FILE")
        .env_remove("WAKEIT_WORKSPACE")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home)
        .env("APPDATA", home)
        .args([
            "--gateway",
            &server.url,
            "--workspace",
            WORKSPACE,
            "--token",
            "fixture-key",
            "--json",
            "run",
            "watch",
            &id.to_string(),
        ])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}
async fn finish(mut child: Child) -> Output {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stdout = vec![];
            let mut stderr = vec![];
            child
                .stdout
                .take()
                .unwrap()
                .read_to_end(&mut stdout)
                .unwrap();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_end(&mut stderr)
                .unwrap();
            return Output {
                status,
                stdout,
                stderr,
            };
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("watch did not exit");
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
fn records(output: &Output, code: i32) -> Vec<Value> {
    assert_eq!(
        output.status.code(),
        Some(code),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let records: Vec<Value> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(records.last().unwrap()["type"], "summary");
    assert_eq!(records.last().unwrap()["data"]["exit_code"], code);
    records
}
fn ok(value: Value) -> (StatusCode, String, Duration) {
    (StatusCode::OK, value.to_string(), Duration::ZERO)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_streams_read_only_events_and_final_report() {
    let id = Uuid::new_v4();
    let states = AtomicUsize::new(0);
    let server=server(move|path|{
        if path.contains("/events?") {
            if path.ends_with("after=0") {return ok(json!([{"id":4,"seq":1,"run_id":id,"kind":"log","data":{"text":"中文 log\n"}},{"id":5,"seq":2,"run_id":id,"kind":"state","data":{"state":"succeeded","result":"DO NOT PRINT TWICE"}}]));}
            return ok(json!([]));
        }
        let n=states.fetch_add(1,Ordering::SeqCst);ok(json!({"id":id,"state":if n==0 {"running"}else{"succeeded"},"result":if n==0 {Value::Null}else{json!("# Final\n\n报告")},"config":{"prompt":"private config"},"session":{"id":"private session"}}))
    }).await;
    let home = tempfile::tempdir().unwrap();
    let output = finish(start(&server, id, &[], home.path())).await;
    let records = records(&output, 0);
    assert_eq!(records.iter().filter(|v| v["type"] == "log").count(), 1);
    assert_eq!(records.last().unwrap()["data"]["result"], "# Final\n\n报告");
    assert!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|s| s.starts_with("GET "))
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("private config"));
    assert!(!text.contains("private session"));
    assert!(!text.contains("DO NOT PRINT TWICE"));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_timeout_covers_stalled_requests_reconnect_and_unknown() {
    for mode in ["stalled", "reconnect", "unknown"] {
        let id = Uuid::new_v4();
        let server = server(move |path| match mode {
            "stalled" => (
                StatusCode::OK,
                json!({"id":id,"state":"running"}).to_string(),
                Duration::from_secs(5),
            ),
            "reconnect" => (StatusCode::SERVICE_UNAVAILABLE, "{}".into(), Duration::ZERO),
            _ if path.contains("/events?") => ok(json!([])),
            _ => ok(json!({"id":id,"state":"unknown","reason":"heartbeat lost"})),
        })
        .await;
        let home = tempfile::tempdir().unwrap();
        let start_time = tokio::time::Instant::now();
        let output = finish(start(&server, id, &["--timeout", "1s"], home.path())).await;
        let records = records(&output, 124);
        assert!(start_time.elapsed() < Duration::from_secs(4));
        assert_eq!(records.last().unwrap()["data"]["terminal"], false);
        if mode == "unknown" {
            assert_eq!(records.last().unwrap()["data"]["state"], "unknown");
            assert!(
                records
                    .iter()
                    .any(|v| v["type"] == "notice" && v["data"]["status"] == "unknown")
            );
        }
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cli_reports_failure_permission_loss_missing_and_malformed_responses() {
    for mode in ["failed", "revoked", "missing", "malformed"] {
        let id = Uuid::new_v4();
        let server=server(move|path|match mode {
            "missing"=>(StatusCode::NOT_FOUND,"{}".into(),Duration::ZERO),
            "malformed"=>(StatusCode::OK,"not JSON".into(),Duration::ZERO),
            "revoked" if path.contains("/events?")=>(StatusCode::UNAUTHORIZED,"{}".into(),Duration::ZERO),
            _ if path.contains("/events?")=>ok(json!([])),
            _=>ok(json!({"id":id,"state":if mode=="failed"{"failed"}else{"running"},"reason":if mode=="failed"{Some("Agent failed")}else{None::<&str>}})),
        }).await;
        let home = tempfile::tempdir().unwrap();
        let output = finish(start(&server, id, &[], home.path())).await;
        let records = records(&output, if mode == "failed" { 1 } else { 2 });
        assert_eq!(
            records.last().unwrap()["data"]["terminal"],
            mode == "failed"
        );
    }
}
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_c_exits_observation_without_cancelling_execution() {
    let id = Uuid::new_v4();
    let server = server(move |path| {
        if path.contains("/events?") {
            ok(json!([]))
        } else {
            ok(json!({"id":id,"state":"running"}))
        }
    })
    .await;
    let home = tempfile::tempdir().unwrap();
    let child = start(&server, id, &[], home.path());
    for _ in 0..200 {
        if server.calls.lock().unwrap().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(server.calls.lock().unwrap().len() >= 2);
    // Signal only this CLI child; no process group or Runner process is touched.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
    let output = finish(child).await;
    let records = records(&output, 130);
    assert_eq!(records.last().unwrap()["data"]["state"], "running");
    assert!(
        server
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|s| s.starts_with("GET "))
    );
}
#[test]
fn cli_rejects_zero_timeout_and_has_no_task_watch() {
    let id = Uuid::new_v4().to_string();
    for args in [
        vec!["run", "watch", &id, "--timeout", "0s"],
        vec!["task", "watch", "task-id"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_wakeit"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
    }
}
