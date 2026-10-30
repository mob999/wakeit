#[path = "../../../fixtures/official_agent.rs"]
mod support;
use chrono::Utc;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;
use wakeit::{
    config::*,
    process::{self, LogSink},
    protocol::Assignment,
    runner::journal::Journal,
    trace::{Lines, RecordData, StreamRecord, TraceState},
};
use wakeit_agent_runtime::stream::Normalizer;
fn task(cwd: &str) -> TaskConfig {
    TaskConfig {
        dag: None,
        version: 1,
        id: "trace-test".into(),
        name: "Trace".into(),
        runner: "fixture".into(),
        agent: "third-party".into(),
        cwd: cwd.into(),
        prompt: "中文\n\" literal".into(),
        parameters: Default::default(),
        session: None,
        schedule: None,
        enabled: false,
        execution: Default::default(),
    }
}
#[test]
fn native_fixtures_preserve_public_steps_and_separate_the_report() {
    for (protocol, fixture) in [
        (
            "codex",
            include_str!("../../wakeit-agent-runtime/tests/fixtures/trace/codex.jsonl"),
        ),
        (
            "claude-code",
            include_str!("../../wakeit-agent-runtime/tests/fixtures/trace/claude-code.jsonl"),
        ),
        (
            "pi",
            include_str!("../../wakeit-agent-runtime/tests/fixtures/trace/pi.jsonl"),
        ),
        (
            "opencode",
            include_str!("../../wakeit-agent-runtime/tests/fixtures/trace/opencode.jsonl"),
        ),
    ] {
        let mut normalizer = Normalizer::new(protocol).unwrap();
        let mut records = vec![];
        for line in fixture.lines() {
            records.extend(normalizer.accept(serde_json::from_str(line).unwrap()));
        }
        records.extend(normalizer.finish());
        let encoded = serde_json::to_string(&records).unwrap();
        assert!(
            !encoded.contains("private should never appear"),
            "{protocol}"
        );
        assert!(records.iter().any(|r| matches!(&r.record,RecordData::Trace(n) if n.id=="cmd-1" && n.status==TraceState::Succeeded && n.finished_at.is_some())),"{protocol}");
        assert!(records.iter().any(|r| matches!(&r.record,RecordData::Trace(n) if n.id=="tool-2" && n.status==TraceState::Running && n.finished_at.is_none())),"{protocol}");
        assert!(
            records
                .iter()
                .any(|r| matches!(&r.record,RecordData::Result(v) if v=="# 完成\n\n报告")),
            "{protocol}"
        );
        for record in &records {
            StreamRecord::parse(&serde_json::to_vec(record).unwrap()).unwrap();
        }
    }
}
#[test]
fn framing_handles_split_unicode_crlf_invalid_records_and_size_limits() {
    let value =
        json!({"version":1,"type":"log","data":{"text":"中文\u{2028}line"}}).to_string() + "\r\n";
    let mut lines = Lines::default();
    let mut records = vec![];
    for byte in value.as_bytes() {
        records.extend(lines.push(&[*byte]));
    }
    assert_eq!(records.len(), 1);
    assert!(records[0].is_ok());
    assert!(lines.push(b"{bad}\n")[0].is_err());
    assert!(lines.push(b"{\"version\":2,\"type\":\"result\",\"data\":1}\n")[0].is_err());
    assert!(lines.push(&vec![b'a'; 1024 * 1024 + 1])[0].is_err());
    assert!(lines.push(b"ignored suffix\n").is_empty());
    assert!(
        lines
            .push(b"{\"version\":1,\"type\":\"result\",\"data\":\"end\"}")
            .is_empty()
    );
    assert!(
        matches!(&lines.finish()[0].as_ref().unwrap().record,RecordData::Result(v) if v=="end")
    );
}
#[tokio::test]
async fn events_stream_live_to_durable_journal_and_result_survives_large_stdout() {
    let dir = tempfile::tempdir().unwrap();
    let t = task(dir.path().to_str().unwrap());
    let id = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let journal = Journal::open(&dir.path().join("journal"), runner)
        .await
        .unwrap();
    let a: AgentConfig = support::official_agent("codex");
    journal
        .insert(
            &Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: t.clone(),
                deadline: Utc::now() + chrono::Duration::hours(1),
            },
            &a,
        )
        .await
        .unwrap();
    let release = dir.path().join("release-agent");
    let code = r#"
const fs = require('node:fs');
const send = (type, data) => console.log(JSON.stringify({version:1,type,data}));
send('trace',{id:'cmd',kind:'command',status:'running',title:'执行',input:'中文',output:null,occurred_at:'2026-10-14T01:00:00Z',started_at:'2026-10-14T01:00:00Z'});
const gate = setInterval(() => {
  if (!fs.existsSync(process.env.WAKEIT_TRACE_RELEASE)) return;
  clearInterval(gate);
  console.log('{malformed}');
  for (let i=0;i<128;i++) send('log',{text:'中文'.repeat(4000)+'\n'});
  send('trace',{id:'cmd',kind:'command',status:'succeeded',title:'执行',input:'中文',output:'done',occurred_at:'2026-10-14T01:00:02Z',started_at:'2026-10-14T01:00:00Z',finished_at:'2026-10-14T01:00:02Z'});
  send('result','# 完成\n\n报告');
},20);
"#;
    let spec = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), code.into()],
        env: [(
            "WAKEIT_TRACE_RELEASE".into(),
            release.to_string_lossy().into_owned(),
        )]
        .into(),
        ..Default::default()
    };
    let sink = LogSink {
        journal: journal.clone(),
        id,
        stdout: true,
    };
    let job = tokio::spawn(async move {
        process::execute_output_named_format(
            &spec,
            &t,
            id,
            None,
            // Keep stdout above 3 MB without turning this correctness test into
            // a benchmark of thousands of FULL SQLite fsyncs on CI disks.
            Duration::from_secs(60),
            tokio_util::sync::CancellationToken::new(),
            Some(sink),
            wakeit::config::OutputFormat::Events,
            None,
        )
        .await
        .unwrap()
    });
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let live: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pending_events WHERE json_extract(payload,'$.kind')='trace'",
            )
            .fetch_one(&journal.pool)
            .await
            .unwrap();
            if live > 0 {
                break;
            }
            assert!(
                !job.is_finished(),
                "Agent exited before a live Trace was recorded"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("first Trace must persist while the Agent is still running");
    assert!(!job.is_finished());
    std::fs::write(release, b"finish").unwrap();
    let out = job.await.unwrap();
    assert_eq!(out.code, Some(0));
    assert!(out.confirmed);
    assert_eq!(out.result, Some(json!("# 完成\n\n报告")));
    assert!(out.stdout.is_empty());
    let records: Vec<String> =
        sqlx::query_scalar("SELECT payload FROM pending_events ORDER BY seq")
            .fetch_all(&journal.pool)
            .await
            .unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|s| serde_json::from_str::<Value>(s).unwrap()["kind"] == "trace")
            .count(),
        2
    );
    assert!(
        records
            .iter()
            .any(|s| serde_json::from_str::<Value>(s).unwrap()["kind"] == "trace_status")
    );
    let log_bytes: usize = records
        .iter()
        .filter_map(|s| {
            let v: Value = serde_json::from_str(s).unwrap();
            (v["kind"] == "log").then(|| v["data"]["text"].as_str().unwrap_or("").len())
        })
        .sum();
    assert!(
        log_bytes > 3_000_000,
        "the final result must survive multi-megabyte stdout"
    );
    let count = records.len();
    journal.pool.close().await;
    let reopened = Journal::open(&dir.path().join("journal"), runner)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pending_events")
            .fetch_one(&reopened.pool)
            .await
            .unwrap(),
        count as i64
    );
    assert!(reopened.pending(id).await.unwrap());
    sqlx::query("UPDATE local_runs SET log_bytes=20971519")
        .execute(&reopened.pool)
        .await
        .unwrap();
    assert!(
        !reopened
            .content(id, "trace", json!({"id":"limit","output":"overflow"}))
            .await
            .unwrap()
    );
    reopened
        .emit(
            id,
            "state",
            json!({"state":"succeeded","result":"independent"}),
            Some("done"),
        )
        .await
        .unwrap();
    let final_state: String =
        sqlx::query_scalar("SELECT payload FROM pending_events ORDER BY seq DESC LIMIT 1")
            .fetch_one(&reopened.pool)
            .await
            .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&final_state).unwrap()["data"]["result"],
        "independent"
    );
}

#[test]
fn native_explicit_failures_are_not_successful_reports_and_retries_can_recover() {
    let mut pi = Normalizer::new("pi").unwrap();
    let failed = pi.accept(json!({"type":"message_end","message":{"role":"assistant","timestamp":1,"content":[],"stopReason":"error","errorMessage":"Connection error."}}));
    assert!(pi.failed());
    assert!(pi.finish().is_empty());
    assert!(failed.iter().any(|r| matches!(&r.record,RecordData::Trace(n) if n.status==TraceState::Failed && n.output=="Connection error.")));
    pi.accept(
        json!({"type":"message_start","message":{"role":"assistant","timestamp":2,"content":[]}}),
    );
    pi.accept(json!({"type":"message_end","message":{"role":"assistant","timestamp":2,"content":[{"type":"text","text":"recovered"}],"stopReason":"stop"}}));
    assert!(!pi.failed());
    assert!(
        pi.finish()
            .iter()
            .any(|r| matches!(&r.record,RecordData::Result(v) if v=="recovered"))
    );
    assert!(pi.accept(json!({"type":"entry_appended"})).is_empty());
    let mut claude = Normalizer::new("claude-code").unwrap();
    claude.accept(json!({"type":"result","subtype":"error_during_execution","is_error":true}));
    assert!(claude.failed());
    let mut codex = Normalizer::new("codex").unwrap();
    let errors = codex.accept(json!({"type":"error","message":"temporary failure"}));
    assert!(codex.failed());
    assert!(
        errors
            .iter()
            .any(|r| matches!(&r.record, RecordData::Trace(n) if n.output == "temporary failure"))
    );
    codex.accept(json!({"type":"item.completed","item":{"id":"final","type":"agent_message","text":"recovered"}}));
    codex.accept(json!({"type":"turn.completed"}));
    assert!(!codex.failed());
    let mut opencode = Normalizer::new("opencode").unwrap();
    opencode.accept(json!({"type":"error","error":{"message":"failed"}}));
    assert!(opencode.failed());
}
#[tokio::test]
async fn stream_helper_keeps_native_children_in_the_runner_process_tree() {
    let dir = tempfile::tempdir().unwrap();
    let t = task(dir.path().to_str().unwrap());
    let code = r#"const fs=require('fs');const child=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});fs.writeFileSync('child.pid',String(child.pid));console.log(JSON.stringify({type:'item.started',item:{id:'command',type:'command_execution',command:'fixture'}}));setInterval(()=>{},1000)"#;
    let spec = CommandSpec {
        command: env!("CARGO_BIN_EXE_wakeit").into(),
        args: vec![
            "plugin".into(),
            "exec".into(),
            "git:github.com/mob999/wakeit".into(),
            "adapter".into(),
            "--".into(),
            "stream".into(),
            "codex".into(),
            "--".into(),
            "node".into(),
            "-e".into(),
            code.into(),
        ],
        ..Default::default()
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let token = cancel.clone();
    let job = tokio::spawn(async move {
        process::execute_output_named_format(
            &spec,
            &t,
            Uuid::new_v4(),
            None,
            Duration::from_secs(30),
            token,
            None,
            wakeit::config::OutputFormat::Events,
            None,
        )
        .await
        .unwrap()
    });
    for _ in 0..200 {
        if dir.path().join("child.pid").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(dir.path().join("child.pid").exists());
    cancel.cancel();
    let out = job.await.unwrap();
    assert_eq!(out.stopped, Some("cancelled"));
    #[cfg(unix)]
    {
        let pid = std::fs::read_to_string(dir.path().join("child.pid"))
            .unwrap()
            .parse::<i32>()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let exists = unsafe { libc::kill(pid, 0) } == 0;
        #[cfg(target_os = "linux")]
        let zombie = std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|s| {
            s.lines()
                .any(|l| l.starts_with("State:") && l.contains("Z (zombie)"))
        });
        #[cfg(not(target_os = "linux"))]
        let zombie = false;
        assert!(!exists || zombie, "native descendant survived cancellation");
    }
    assert!(out.confirmed);
}

#[test]
fn opencode_report_combines_final_message_parts_without_tool_output_or_previous_turns() {
    let mut n = Normalizer::new("opencode").unwrap();
    n.accept(json!({"type":"text","part":{"id":"before","messageID":"before","text":"working"}}));
    n.accept(json!({"type":"tool_use","part":{"id":"shell","tool":"shell","state":{"status":"completed","output":"tool-only"}}}));
    assert!(n.finish().is_empty());
    n.accept(json!({"type":"text","part":{"id":"p1","messageID":"final","text":"# 报告"}}));
    n.accept(json!({"type":"text","part":{"id":"p2","messageID":"final","text":"完成"}}));
    n.accept(json!({"type":"text","part":{"id":"p2","messageID":"final","text":"完成"}}));
    assert!(
        n.finish()
            .iter()
            .any(|r| matches!(&r.record,RecordData::Result(v) if v=="# 报告\n完成"))
    );
}

async fn recording_sample(script: &str) -> (process::Output, Vec<Value>) {
    let dir = tempfile::tempdir().unwrap();
    let task = task(dir.path().to_str().unwrap());
    let id = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let journal = Journal::open(&dir.path().join("journal"), runner)
        .await
        .unwrap();
    let agent: AgentConfig = support::official_agent("codex");
    journal
        .insert(
            &Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: task.clone(),
                deadline: Utc::now() + chrono::Duration::hours(1),
            },
            &agent,
        )
        .await
        .unwrap();
    let command = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), script.into()],
        ..Default::default()
    };
    let out = process::execute_output_named_format(
        &command,
        &task,
        id,
        None,
        Duration::from_secs(30),
        tokio_util::sync::CancellationToken::new(),
        Some(LogSink {
            journal: journal.clone(),
            id,
            stdout: true,
        }),
        wakeit::config::OutputFormat::Events,
        None,
    )
    .await
    .unwrap();
    let records: Vec<String> =
        sqlx::query_scalar("SELECT payload FROM pending_events ORDER BY seq")
            .fetch_all(&journal.pool)
            .await
            .unwrap();
    (
        out,
        records
            .iter()
            .map(|record| serde_json::from_str(record).unwrap())
            .collect(),
    )
}

#[tokio::test]
async fn ordinary_diagnostics_only_enter_logs_and_do_not_mark_activity_incomplete() {
    let (out, events) = recording_sample(
        r#"
const send = (type,data) => console.log(JSON.stringify({version:1,type,data}));
send('log',{text:'Ordinary adapter diagnostic',stream:'stderr',diagnostic:true});
send('result','Native result retained');
"#,
    )
    .await;
    assert_eq!(out.code, Some(0));
    assert_eq!(out.result, Some(json!("Native result retained")));
    assert!(
        events.iter().any(|event| event["kind"] == "log"
            && event["data"]["text"] == "Ordinary adapter diagnostic")
    );
    assert!(events.iter().all(|event| event["kind"] != "trace_status"));
}

#[tokio::test]
async fn recording_causes_are_explicit_deduplicated_and_never_abort_native_execution() {
    let (out, events) = recording_sample(r#"
const send = (type,data) => console.log(JSON.stringify({version:1,type,data}));
for (let i=0;i<2;i++) {
  send('log',{text:'Event budget exhausted',diagnostic:true,recording_issue:'event_limit'});
  console.log('{malformed}');
  console.log('x'.repeat(1024*1024));
  send('trace',{id:'tool',kind:'tool',status:'succeeded',title:'Read',input:null,output:{truncated:true,preview:'Saved excerpt'},occurred_at:'2026-10-20T00:00:00Z'});
}
send('result','The native Agent still finishes');
"#).await;
    assert_eq!(out.code, Some(0));
    assert!(out.confirmed);
    assert_eq!(out.result, Some(json!("The native Agent still finishes")));
    let reasons: Vec<&str> = events
        .iter()
        .filter(|event| event["kind"] == "trace_status")
        .map(|event| event["data"]["reason"].as_str().unwrap())
        .collect();
    for cause in [
        "event_limit",
        "event_parse_failed",
        "event_too_large",
        "content_shortened",
    ] {
        assert_eq!(
            reasons.iter().filter(|reason| **reason == cause).count(),
            1,
            "{cause}"
        );
    }
    assert_eq!(reasons.len(), 4);
}

#[test]
fn version_one_diagnostics_remain_valid_without_optional_recording_metadata() {
    let record = StreamRecord::parse(
        br#"{"version":1,"type":"log","data":{"text":"diagnostic","diagnostic":true}}"#,
    )
    .unwrap();
    assert!(matches!(record.record, RecordData::Log(log) if log.recording_issue.is_none()));
}
