use wakeit_agent_runtime::gemini::GeminiNormalizer;
mod support;
use support::assert_single_report;

use serde_json::{Value, json};
use wakeit_plugin_sdk::trace::{RecordData, TraceKind, TraceState};

#[test]
fn native_fixture_preserves_chunks_and_tool_lifecycle() {
    let mut normalizer = GeminiNormalizer::default();
    let records: Vec<_> = include_str!("fixtures/trace/gemini-cli.jsonl")
        .lines()
        .flat_map(|line| normalizer.accept(serde_json::from_str(line).unwrap()))
        .collect();
    assert!(!normalizer.failed());
    let nodes: Vec<_> = records
        .iter()
        .filter_map(|r| match &r.record {
            RecordData::Trace(node) => Some(node),
            _ => None,
        })
        .collect();
    let shell: Vec<_> = nodes
        .iter()
        .filter(|n| n.kind == TraceKind::Command)
        .collect();
    assert_eq!(shell.len(), 2);
    assert_eq!(shell[0].id, shell[1].id);
    assert_eq!(shell[0].status, TraceState::Running);
    assert_eq!(shell[1].status, TraceState::Succeeded);
    assert_eq!(shell[1].input["command"], "cargo test");
    assert_eq!(shell[1].output, "all tests passed");
    assert!(shell[1].started_at.is_some());
    assert!(shell[1].finished_at.is_some());
    assert_eq!(nodes.last().unwrap().output, "测试通过。");
    assert!(
        !records
            .iter()
            .any(|r| matches!(&r.record, RecordData::Result(_)))
    );
    assert_single_report(&normalizer.finish(), json!("测试通过。"));
    assert!(
        !records
            .iter()
            .any(|r| matches!(&r.record, RecordData::Trace(n) if n.output == "Run tests"))
    );
}

#[test]
fn warning_and_recoverable_tool_failure_do_not_fail_a_successful_turn() {
    let mut normalizer = GeminiNormalizer::default();
    for event in [
        json!({"type":"error","severity":"warning","message":"retrying"}),
        json!({"type":"tool_use","tool_name":"read_file","tool_id":"read-1","parameters":{"file_path":"missing"}}),
        json!({"type":"tool_result","tool_id":"read-1","status":"error","error":{"type":"FILE_NOT_FOUND","message":"missing"}}),
        json!({"type":"message","role":"assistant","content":"File unavailable","delta":true}),
        json!({"type":"result","status":"success"}),
    ] {
        normalizer.accept(event);
    }
    assert!(!normalizer.failed());
    assert_single_report(&normalizer.finish(), json!("File unavailable"));
}

#[test]
fn fatal_errors_and_incomplete_streams_never_emit_an_answer() {
    for terminal in [
        Some(json!({"type":"result","status":"error","error":{"message":"Quota exhausted"}})),
        None,
    ] {
        let mut normalizer = GeminiNormalizer::default();
        normalizer.accept(
            json!({"type":"message","role":"assistant","content":"Partial answer","delta":true}),
        );
        if let Some(event) = terminal {
            let records = normalizer.accept(event);
            assert!(records.iter().any(
                |r| matches!(&r.record, RecordData::Trace(n) if n.status == TraceState::Failed)
            ));
        }
        assert!(normalizer.failed());
        assert!(normalizer.finish().is_empty());
    }
    let mut normalizer = GeminiNormalizer::default();
    normalizer.accept(json!({"type":"error","severity":"error","message":"Invalid stream"}));
    let records = normalizer.accept(json!({"type":"result","status":"success"}));
    assert!(normalizer.failed());
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.record, RecordData::Result(_)))
    );
    assert!(normalizer.finish().is_empty());
}

#[test]
fn snapshot_messages_replace_text_and_unknown_events_are_diagnostic_only() {
    let mut normalizer = GeminiNormalizer::default();
    normalizer.accept(json!({"type":"message","role":"assistant","content":"old","delta":true}));
    normalizer.accept(json!({"type":"message","role":"assistant","content":"snapshot"}));
    normalizer
        .accept(json!({"type":"message","role":"assistant","content":" + delta","delta":true}));
    let omitted = normalizer.accept(json!({"type":"thought","content":"private reasoning"}));
    assert!(
        matches!(&omitted[0].record, RecordData::Log(log) if log.diagnostic && !log.text.contains("private reasoning"))
    );
    assert!(
        normalizer
            .accept(json!({"type":"thought","content":"private reasoning"}))
            .is_empty()
    );
    normalizer.accept(json!({"type":"result","status":"success"}));
    assert_single_report(&normalizer.finish(), json!("snapshot + delta"));
}

#[test]
fn sdk_native_arguments_keep_instructions_off_argv_and_advertise_only_supported_queries() {
    use wakeit_plugin_sdk::adapter::{AgentDescriptor, AgentOperation, AgentRequest};
    let manifest: Value =
        serde_json::from_str(include_str!("../../../wakeit-plugin.json")).unwrap();
    let descriptor: AgentDescriptor = serde_json::from_value(
        manifest["wakeit"]["plugins"]["official"]["agents"]["gemini-cli"].clone(),
    )
    .unwrap();
    let prompt = "Synthetic instructions\n--resume $(not-a-shell)";
    let model = "synthetic model with spaces";
    let request: AgentRequest = serde_json::from_value(json!({
        "id": uuid::Uuid::new_v4(), "agent": "gemini-cli", "operation": "execute",
        "context": {"cwd":"/synthetic", "deadline":chrono::Utc::now() + chrono::Duration::seconds(60),
            "run_id":uuid::Uuid::new_v4(), "task_id":"synthetic", "prompt":prompt,
            "parameters":json!({"model": model})}
    })).unwrap();
    request.validate(&descriptor).unwrap();
    let args = wakeit_agent_runtime::official_args::native_args(&request, false).unwrap();
    let (input_flag, input_value) = ("--prompt", "");
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == input_flag && pair[1] == input_value)
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--output-format" && pair[1] == "stream-json")
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "--model" && pair[1] == model)
    );
    assert!(!args.iter().any(|arg| arg.contains(prompt)));
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--resume" || arg == "--no-session")
    );
    assert!(!descriptor.operations.contains(&AgentOperation::Models));
    for operation in [
        AgentOperation::SessionList,
        AgentOperation::SessionInspect,
        AgentOperation::SessionResume,
        AgentOperation::SessionRename,
        AgentOperation::SessionInteractive,
    ] {
        assert!(!descriptor.operations.contains(&operation));
    }
    assert!(wakeit_agent_runtime::official_args::native_args(&request, true).is_err());
}

#[test]
fn duplicate_tool_events_do_not_regress_a_completed_call() {
    let mut normalizer = GeminiNormalizer::default();
    let start = json!({"type":"tool_use","tool_name":"run_shell_command","tool_id":"shell","parameters":{"command":"echo done"}});
    normalizer.accept(start.clone());
    let records = normalizer
        .accept(json!({"type":"tool_result","tool_id":"shell","status":"success","output":"done"}));
    assert!(
        matches!(&records[0].record, RecordData::Trace(n) if n.status == TraceState::Succeeded)
    );
    assert!(normalizer.accept(start).is_empty());
    assert!(normalizer.accept(json!({"type":"tool_result","tool_id":"shell","status":"error","error":{"message":"late duplicate"}})).is_empty());
    normalizer.accept(json!({"type":"message","role":"assistant","content":"done","delta":true}));
    let terminal = normalizer.accept(json!({"type":"result","status":"success"}));
    assert!(
        !terminal
            .iter()
            .any(|r| matches!(r.record, RecordData::Result(_)))
    );
    normalizer.accept(json!({"type":"tool_use","tool_name":"read_file","tool_id":"late"}));
    normalizer.accept(json!({"type":"result","status":"error"}));
    assert!(!normalizer.failed());
    assert_single_report(&normalizer.finish(), json!("done"));
}

#[test]
fn oversized_delta_response_is_bounded_and_never_published_as_complete_answer() {
    let mut normalizer = GeminiNormalizer::default();
    let chunk = "\u{1}".repeat(100_000);
    normalizer.accept(json!({"type":"message","role":"assistant","content":chunk,"delta":true}));
    let overflow = normalizer
        .accept(json!({"type":"message","role":"assistant","content":chunk,"delta":true}));
    assert!(
        overflow
            .iter()
            .any(|r| matches!(&r.record, RecordData::Log(log) if log.diagnostic))
    );
    assert!(
        overflow
            .iter()
            .all(|r| serde_json::to_vec(r).unwrap().len() < wakeit_plugin_sdk::trace::RECORD_LIMIT)
    );
    for _ in 0..10 {
        assert!(
            normalizer
                .accept(json!({"type":"message","role":"assistant","content":chunk,"delta":true}))
                .is_empty()
        );
    }
    normalizer.accept(json!({"type":"result","status":"success"}));
    assert!(!normalizer.failed());
    assert!(normalizer.finish().is_empty());
}
