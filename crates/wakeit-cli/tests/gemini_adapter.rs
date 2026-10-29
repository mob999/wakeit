mod support;
use support::assert_single_report;

use serde_json::{Value, json};
use wakeit::{
    adapters::gemini::GeminiNormalizer,
    trace::{RecordData, TraceKind, TraceState},
};

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
fn yaml_has_explicit_headless_execution_without_invented_models_or_sessions() {
    let config: Value =
        yaml_serde::from_str(include_str!("../../../agents/gemini-cli.yaml")).unwrap();
    assert_eq!(config["id"], "gemini-cli");
    assert!(config.get("models").is_none());
    assert!(config.get("session").is_none());
    let args = config["submit"]["args"].as_array().unwrap();
    assert!(args.windows(2).any(|w| w == [json!("--prompt"), json!("")]));
    assert!(
        args.windows(2)
            .any(|w| w == [json!("--output-format"), json!("stream-json")])
    );
    assert_eq!(config["submit"]["stdin"], "{{task.prompt}}");
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
            .all(|r| serde_json::to_vec(r).unwrap().len() < wakeit::trace::RECORD_LIMIT)
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
