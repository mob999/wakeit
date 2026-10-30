use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use wakeit::trace::{RECORD_LIMIT, RecordData, StreamRecord, TraceState};
use wakeit_agent_runtime::kimi_server::LiveNormalizer;
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

#[test]
fn oversized_tools_keep_real_status_and_allow_a_complete_final_report() {
    for failed in [false, true] {
        let mut n = LiveNormalizer::default();
        let start = event(
            "tool.call.started",
            json!({"turnId":0,"toolCallId":"large","name":"Read","args":{"path":"中".repeat(200_000)}}),
        );
        let records = n.accept(&start).unwrap();
        assert!(records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.input["truncated"] == true)));
        let result = event(
            "tool.result",
            json!({"turnId":0,"toolCallId":"large","output":{"content":"中".repeat(500_000)},"isError":failed}),
        );
        let records = n.accept(&result).unwrap();
        let node = records
            .iter()
            .find_map(|record| match &record.record {
                RecordData::Trace(node) => Some(node),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            node.status,
            if failed {
                TraceState::Failed
            } else {
                TraceState::Succeeded
            }
        );
        assert_eq!(node.output["truncated"], true);
        for record in records {
            assert!(serde_json::to_vec(&record).unwrap().len() < RECORD_LIMIT);
        }
        assert!(n.accept(&result).unwrap().is_empty());
        assert!(n.accept(&start).unwrap().is_empty());
        n.accept(&event(
            "assistant.delta",
            json!({"turnId":0,"delta":"Final report after tool"}),
        ))
        .unwrap();
        assert!(n.accept(&start).unwrap().is_empty());
        assert!(n.accept(&result).unwrap().is_empty());
        n.accept(&event(
            "turn.ended",
            json!({"turnId":0,"reason":if failed { "failed" } else { "completed" }}),
        ))
        .unwrap();
        assert_eq!(n.failed, failed);
        if failed {
            assert!(n.result().is_empty());
        } else {
            assert!(
                matches!(&n.result()[0].record, RecordData::Result(value) if value == "Final report after tool")
            );
        }
    }
}

#[test]
fn oversized_messages_are_marked_incomplete_without_failing_the_native_turn() {
    let mut n = LiveNormalizer::default();
    for delta in ["中".repeat(500_000), "tail".into()] {
        let records = n
            .accept(&event("assistant.delta", json!({"turnId":0,"delta":delta})))
            .unwrap();
        assert!(records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.output["truncated"] == true)));
        for record in records {
            assert!(serde_json::to_vec(&record).unwrap().len() < RECORD_LIMIT);
        }
    }
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(!n.failed);
    assert!(
        matches!(&n.result()[0].record, RecordData::Result(value) if value["truncated"] == true && value["preview"].as_str().unwrap().starts_with("中"))
    );
}

#[test]
fn a_large_trace_preview_does_not_truncate_a_report_that_fits_the_transport() {
    let mut n = LiveNormalizer::default();
    let report = "中".repeat(100_000);
    let records = n
        .accept(&event(
            "assistant.delta",
            json!({"turnId":0,"delta":report}),
        ))
        .unwrap();
    assert!(records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.output["truncated"] == true)));
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(!n.failed);
    assert!(matches!(&n.result()[0].record, RecordData::Result(value) if value == &report));
}

#[test]
fn aggregate_tool_input_budget_only_truncates_the_trace() {
    let mut n = LiveNormalizer::default();
    let mut truncated = false;
    for id in 0..40 {
        let records = n.accept(&event("tool.call.started", json!({"turnId":0,"toolCallId":id.to_string(),"name":"Read","args":{"path":"x".repeat(250_000)}}))).unwrap();
        truncated |= records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.input["truncated"] == true));
    }
    assert!(truncated);
    for id in 0..40 {
        n.accept(&event(
            "tool.result",
            json!({"turnId":0,"toolCallId":id.to_string(),"output":"done"}),
        ))
        .unwrap();
    }
    n.accept(&event(
        "assistant.delta",
        json!({"turnId":0,"delta":"complete"}),
    ))
    .unwrap();
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(!n.failed);
    assert!(matches!(&n.result()[0].record, RecordData::Result(value) if value == "complete"));
}

#[test]
fn trace_count_and_dedup_budgets_do_not_interrupt_native_completion() {
    let mut n = LiveNormalizer::default();
    for seq in 1..=100_005 {
        let event = json!({"type":"housekeeping","seq":seq,"payload":{}});
        assert!(n.accept(&event).unwrap().is_empty());
    }
    // A replay older than the bounded dedup window must not be appended again.
    assert!(
        n.accept(
            &json!({"type":"assistant.delta","seq":1,"payload":{"turnId":0,"delta":"replayed"}})
        )
        .unwrap()
        .is_empty()
    );
    for id in 0..10_005 {
        n.accept(&event(
            "tool.call.started",
            json!({"turnId":0,"toolCallId":id.to_string(),"name":"Read","args":{}}),
        ))
        .unwrap();
    }
    for id in 0..10_005 {
        n.accept(&event(
            "tool.result",
            json!({"turnId":0,"toolCallId":id.to_string(),"output":"done"}),
        ))
        .unwrap();
    }
    n.accept(&event(
        "assistant.delta",
        json!({"turnId":0,"delta":"Final report"}),
    ))
    .unwrap();
    n.accept(&event(
        "turn.ended",
        json!({"turnId":0,"reason":"completed"}),
    ))
    .unwrap();
    assert!(!n.failed);
    assert!(matches!(&n.result()[0].record, RecordData::Result(value) if value == "Final report"));
}

#[tokio::test]
async fn real_time_server_bridge_preserves_titles_and_fails_closed() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../wakeit-agent-runtime/tests/fixtures/kimi-code/live-server.cjs");
    for mode in [
        "ok",
        "fail",
        "disconnect",
        "reject",
        "wrong-cwd",
        "broken-title",
        "shutdown-fail",
        "large-tools",
        "large-failed-tool",
        "large-message",
    ] {
        let home = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let mut p = Command::new(env!("CARGO_BIN_EXE_wakeit"));
        p.args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
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
                assert!(line.len() <= RECORD_LIMIT, "{mode}");
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
        let successful = matches!(mode, "ok" | "large-tools" | "large-message");
        assert_eq!(status.success(), successful, "{mode}");
        assert_eq!(
            records
                .iter()
                .filter(|r| matches!(r.record, RecordData::Result(_)))
                .count(),
            usize::from(successful),
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
        if successful {
            assert!(
                !requests.contains(":abort"),
                "large presentation content aborted native execution: {mode}"
            );
        }
        if mode == "large-tools" {
            assert!(records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.input["truncated"] == true && node.output["truncated"] == true && node.status == TraceState::Succeeded)));
            assert!(records.iter().any(|record| matches!(&record.record, RecordData::Result(value) if value == "Final report")));
        }
        if mode == "large-message" {
            assert!(records.iter().any(|record| matches!(&record.record, RecordData::Result(value) if value["truncated"] == true)));
        }
        if mode == "large-failed-tool" {
            assert!(records.iter().any(|record| matches!(&record.record, RecordData::Trace(node) if node.output["truncated"] == true && node.status == TraceState::Failed)));
        }
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
        .join("../wakeit-agent-runtime/tests/fixtures/kimi-code/live-server.cjs");
    let mut child = Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
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
