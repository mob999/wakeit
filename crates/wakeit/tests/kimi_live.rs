use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use wakeit::{
    kimi_live::LiveNormalizer,
    trace::{RecordData, StreamRecord, TraceState},
};
fn event(kind: &str, p: Value) -> Value {
    json!({"type":kind,"payload":p,"timestamp":"2026-10-16T00:00:00Z"})
}
#[test]
fn public_deltas_are_snapshots_and_tools_do_not_become_the_report() {
    let mut n = LiveNormalizer::default();
    n.accept(&event("turn.started", json!({"turnId":0,"agentId":"main"})))
        .unwrap();
    for text in ["Hello ", "中文"] {
        n.accept(&event("assistant.delta", json!({"turnId":0,"delta":text})))
            .unwrap();
    }
    let r = n
        .accept(&event(
            "tool.call.started",
            json!({"turnId":0,"toolCallId":"one","name":"Bash","args":{"command":"pwd"}}),
        ))
        .unwrap();
    assert!(r.iter().any(|r|matches!(&r.record,RecordData::Trace(t) if t.status==TraceState::Running&&t.input["command"]=="pwd")));
    n.accept(&event(
        "tool.result",
        json!({"turnId":0,"toolCallId":"one","output":"tool-only"}),
    ))
    .unwrap();
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(n.result().is_empty());
}
#[test]
fn failure_and_incomplete_turns_cannot_publish_a_report() {
    for reason in ["failed", "cancelled", "blocked"] {
        let mut n = LiveNormalizer::default();
        n.accept(&event(
            "assistant.delta",
            json!({"turnId":0,"delta":"partial"}),
        ))
        .unwrap();
        assert!(n.result().is_empty());
        n.accept(&event("turn.ended", json!({"turnId":0,"reason":reason})))
            .unwrap();
        assert!(n.failed);
        assert!(n.result().is_empty());
    }
    let mut n = LiveNormalizer::default();
    n.accept(&event(
        "tool.call.started",
        json!({"turnId":0,"toolCallId":"one","name":"Bash","args":{}}),
    ))
    .unwrap();
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(n.failed);
}
#[test]
fn thinking_is_omitted_and_terminal_tools_do_not_regress_without_timestamps() {
    let mut n = LiveNormalizer::default();
    assert!(
        n.accept(&event("thinking.delta", json!({"delta":"private"})))
            .unwrap()
            .is_empty()
    );
    let started = json!({"type":"tool.call.started","payload":{"turnId":0,"toolCallId":"one","name":"Read","args":{}}});
    n.accept(&started).unwrap();
    let result = json!({"type":"tool.result","payload":{"turnId":0,"toolCallId":"one","output":"done","isError":true}});
    assert!(
        matches!(&n.accept(&result).unwrap()[0].record,RecordData::Trace(t) if t.status==TraceState::Failed&&t.started_at.is_none()&&t.finished_at.is_none())
    );
    assert!(n.accept(&result).unwrap().is_empty());
    assert!(n.accept(&started).unwrap().is_empty());
}
#[tokio::test]
async fn real_time_server_bridge_preserves_titles_and_fails_closed() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/kimi-code/live-server.cjs");
    for mode in [
        "ok",
        "fail",
        "disconnect",
        "reject",
        "wrong-cwd",
        "broken-title",
        "shutdown-fail",
    ] {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut p = Command::new(env!("CARGO_BIN_EXE_wakeit"));
        p.args([
            "adapter",
            "kimi",
            "--title",
            "中文 literal {{task.prompt}}-wakeit-20261016",
        ])
        .arg("--")
        .arg("node")
        .arg(&script)
        .env("KIMI_CODE_HOME", home.path())
        .env("WAKEIT_KIMI_FIXTURE", mode)
        .current_dir(cwd.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
        let mut child = p.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        input
            .write_all("literal \"中文\"\n$(not-shell)".as_bytes())
            .await
            .unwrap();
        drop(input);
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut records = vec![];
        let mut early = false;
        tokio::time::timeout(std::time::Duration::from_secs(15), async {
            while let Some(line) = lines.next_line().await.unwrap() {
                assert!(!line.contains("synthetic-fixture-only"));
                let r = StreamRecord::parse(line.as_bytes()).unwrap();
                if matches!(&r.record,RecordData::Trace(t) if t.status==TraceState::Running)
                    && !home.path().join("tool-completed").exists()
                {
                    early = true
                }
                records.push(r);
            }
        })
        .await
        .unwrap();
        let status = child.wait().await.unwrap();
        assert_eq!(status.success(), mode == "ok", "{mode}");
        assert_eq!(
            records
                .iter()
                .filter(|r| matches!(r.record, RecordData::Result(_)))
                .count(),
            usize::from(mode == "ok"),
            "{mode}"
        );
        if mode == "ok" {
            assert!(early, "events were buffered until tool completion");
            assert!(
                home.path().join("pong-received").exists(),
                "native heartbeat was not answered"
            );
            let state: Value =
                serde_json::from_slice(&std::fs::read(home.path().join("state.json")).unwrap())
                    .unwrap();
            assert_eq!(
                state["title"],
                "中文 literal {{task.prompt}}-wakeit-20261016"
            );
        }
        let requests = std::fs::read_to_string(home.path().join("requests.jsonl")).unwrap();
        assert!(requests.contains("/shutdown"));
        assert_eq!(
            requests.lines().filter(|l| l.contains("/prompts")).count(),
            usize::from(!["wrong-cwd", "broken-title"].contains(&mode)),
            "prompt must never be resubmitted: {mode}"
        );
    }
}

#[tokio::test]
async fn bound_session_is_activated_before_subscription_and_keeps_its_title_and_model() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    let cwd = std::fs::canonicalize(cwd.path()).unwrap();
    std::fs::write(
        home.path().join("state.json"),
        json!({"id":"session_fixture-live","cwd":cwd,"title":"Original native title"}).to_string(),
    )
    .unwrap();
    std::fs::write(
        home.path().join("session_index.jsonl"),
        json!({"sessionId":"session_fixture-live","workDir":cwd,"sessionDir":home.path()})
            .to_string()
            + "\n",
    )
    .unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/kimi-code/live-server.cjs");
    let mut child = Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "adapter",
            "kimi",
            "--session",
            "session_fixture-live",
            "--",
            "node",
        ])
        .arg(script)
        .env("KIMI_CODE_HOME", home.path())
        .env("WAKEIT_KIMI_FIXTURE", "cold")
        .current_dir(&cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"continue")
        .await
        .unwrap();
    let output = tokio::time::timeout(std::time::Duration::from_secs(15), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let state: Value =
        serde_json::from_slice(&std::fs::read(home.path().join("state.json")).unwrap()).unwrap();
    assert_eq!(state["title"], "Original native title");
    let requests = std::fs::read_to_string(home.path().join("requests.jsonl")).unwrap();
    let requests: Vec<Value> = requests
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert!(
        !requests
            .iter()
            .any(|r| r["path"] == "/api/v1/sessions" || r["path"] == "/api/v1/config")
    );
    assert!(requests.iter().any(
        |r| r["path"] == "/api/v1/sessions/session_fixture-live/profile"
            && r["body"] == json!({"agent_config":{}})
    ));
    let prompt = requests
        .iter()
        .find(|r| r["path"] == "/api/v1/sessions/session_fixture-live/prompts")
        .unwrap();
    assert!(prompt["body"].get("model").is_none());
}
