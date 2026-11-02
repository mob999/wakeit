use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
};
use wakeit::trace::{RecordData, StreamRecord};
fn command(root: &Path, mode: &str, session: bool, model: Option<&str>) -> Command {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../wakeit-agent-runtime/tests/fixtures/kimi-code/acp-agent.cjs");
    let mut command = Command::new(env!("CARGO_BIN_EXE_wakeit"));
    command.args([
        "plugin",
        "exec",
        "git:github.com/mob999/wakeit",
        "adapter",
        "--",
        "run",
        "kimi-code",
    ]);
    if session {
        command.args(["--session", "kimi-acp-fixture"]);
    }
    if let Some(model) = model {
        command.args(["--model", model]);
    }
    command
        .arg("--")
        .arg("node")
        .arg(fixture)
        .arg(mode)
        .current_dir(root)
        .env("KIMI_CODE_HOME", root)
        .env("WAKEIT_PLUGIN_HOME", root.join("plugins"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
async fn execute(
    root: &Path,
    mode: &str,
    session: bool,
    model: Option<&str>,
) -> std::process::Output {
    let mut child = command(root, mode, session, model).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("literal 中文\n\"quotes\" $(not-a-shell)".as_bytes())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(12), child.wait_with_output())
        .await
        .unwrap()
        .unwrap()
}
fn records(bytes: &[u8]) -> Vec<StreamRecord> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| StreamRecord::parse(l.as_bytes()).unwrap())
        .collect()
}
fn calls(root: &Path) -> Vec<Value> {
    std::fs::read_to_string(root.join("calls.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
#[tokio::test]
async fn native_acp_streams_tools_then_report_and_preserves_bound_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = command(dir.path(), "normal", false, Some("chosen-model"))
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("literal 中文\n\"quotes\" $(not-a-shell)".as_bytes())
        .await
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut observed = Vec::new();
    let mut pending = false;
    while let Some(line) = output.next_line().await.unwrap() {
        let record = StreamRecord::parse(line.as_bytes()).unwrap();
        if matches!(&record.record,RecordData::Trace(n) if n.kind==wakeit::trace::TraceKind::Command && n.status==wakeit::trace::TraceState::Running)
        {
            pending = true;
            assert!(child.try_wait().unwrap().is_none());
        }
        observed.push(record);
    }
    assert!(child.wait().await.unwrap().success());
    assert!(pending);
    assert_eq!(
        observed
            .iter()
            .filter(|r| matches!(r.record, RecordData::Result(_)))
            .count(),
        1
    );
    assert!(
        observed
            .iter()
            .any(|r| matches!(&r.record,RecordData::Result(v) if v=="ACP_REPORT"))
    );
    assert!(
        observed
            .iter()
            .any(|r| matches!(&r.record,RecordData::Trace(n) if n.output=="ACP_TOOL_OUTPUT"))
    );
    assert!(
        !serde_json::to_string(&observed)
            .unwrap()
            .contains("PRIVATE_REASONING")
    );
    let state = dir.path().join("fixture-session/state.json");
    let mut metadata: Value = serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(metadata["model"], "chosen-model");
    metadata["title"] = "Existing title".into();
    std::fs::write(&state, metadata.to_string()).unwrap();
    let before = calls(dir.path()).len();
    let resumed = execute(dir.path(), "normal", true, None).await;
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let after = calls(dir.path());
    let new = &after[before..];
    assert!(new.iter().any(|c| c["method"] == "session/resume"));
    assert!(
        !new.iter()
            .any(|c| c["method"] == "session/new" || c["method"] == "session/set_config_option")
    );
    assert!(records(&resumed.stdout).iter().any(|r|matches!(&r.record,RecordData::Session(s) if s.persisted && s.title.as_deref()==Some("Existing title"))));
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(state).unwrap()).unwrap()["model"],
        "chosen-model"
    );
}
#[tokio::test]
async fn incomplete_failed_disconnected_or_invalid_acp_cannot_publish_success() {
    for mode in [
        "rpc-error",
        "disconnect",
        "wrong-session",
        "incomplete",
        "failed-stop",
        "shutdown-failure",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let output = execute(dir.path(), mode, false, None).await;
        assert!(!output.status.success(), "{mode} unexpectedly succeeded");
        assert!(
            !records(&output.stdout)
                .iter()
                .any(|r| matches!(r.record, RecordData::Result(_))),
            "{mode} published a final report"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let invalid = execute(dir.path(), "normal", false, Some("invalid")).await;
    assert!(!invalid.status.success());
    assert!(
        !calls(dir.path())
            .iter()
            .any(|c| c["method"] == "session/prompt")
    );
    let dir = tempfile::tempdir().unwrap();
    let missing = execute(dir.path(), "normal", true, None).await;
    assert!(!missing.status.success());
    assert!(!dir.path().join("calls.jsonl").exists());
}
#[tokio::test]
async fn unexpected_permission_requests_do_not_expand_native_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let output = execute(dir.path(), "permission", false, None).await;
    assert!(output.status.success());
    let response: Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("permission.json")).unwrap())
            .unwrap();
    assert_eq!(
        response["outcome"],
        json!({"outcome":"selected","optionId":"no"})
    );
}
#[cfg(unix)]
#[tokio::test]
async fn cancellation_notifies_native_acp_before_stopping_the_peer() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = command(dir.path(), "wait", false, None).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"wait")
        .await
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    while let Some(line) = lines.next_line().await.unwrap() {
        if matches!(StreamRecord::parse(line.as_bytes()).unwrap().record,RecordData::Trace(n) if n.kind==wakeit::trace::TraceKind::Command)
        {
            break;
        }
    }
    // Signal only the helper, letting the synthetic peer receive the ACP cancellation.
    unsafe {
        libc::kill(child.id().unwrap() as i32, libc::SIGTERM);
    }
    let mut results = Vec::new();
    while let Some(line) = lines.next_line().await.unwrap() {
        results.push(line);
    }
    let status = tokio::time::timeout(Duration::from_secs(8), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.code(), Some(130));
    assert!(
        calls(dir.path())
            .iter()
            .any(|c| c["method"] == "session/cancel")
    );
    assert!(!results.iter().any(|l| l.contains("\"type\":\"result\"")));
}
