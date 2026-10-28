mod support;
use support::assert_single_report;

use serde_json::{Value, json};
use wakeit::{
    goose_adapter::GooseNormalizer,
    trace::{RecordData, TraceKind, TraceState},
};

fn message(content: Value) -> Value {
    json!({"type":"message","message":{"id":"native-1","role":"assistant","created":1792108800,"content":content,"metadata":{"userVisible":true,"agentVisible":true}}})
}

fn text(text: &str) -> Value {
    message(json!([{"type":"text","text":text}]))
}

fn tool_request(id: &str) -> Value {
    message(
        json!([{"type":"toolRequest","id":id,"toolCall":{"status":"success","value":{"name":"developer__shell","arguments":{"command":"echo done"}}}}]),
    )
}

#[test]
fn official_stream_shape_produces_incremental_messages_and_tool_lifecycle() {
    let mut normalizer = GooseNormalizer::default();
    let records: Vec<_> = include_str!("fixtures/trace/goose.jsonl")
        .lines()
        .flat_map(|line| normalizer.accept(serde_json::from_str(line).unwrap()))
        .collect();
    assert!(!normalizer.failed());
    let nodes: Vec<_> = records
        .iter()
        .filter_map(|record| match &record.record {
            RecordData::Trace(node) => Some(node),
            _ => None,
        })
        .collect();
    let command: Vec<_> = nodes
        .iter()
        .filter(|n| n.kind == TraceKind::Command)
        .collect();
    assert_eq!(command.len(), 2);
    assert_eq!(command[0].id, command[1].id);
    assert_eq!(command[0].title, "developer__shell");
    assert_eq!(command[0].status, TraceState::Running);
    assert_eq!(command[1].status, TraceState::Succeeded);
    assert_eq!(command[1].input["command"], "cargo test");
    assert_eq!(command[1].output["content"].as_array().unwrap().len(), 1);
    assert!(command[1].started_at.is_some() && command[1].finished_at.is_some());
    assert_eq!(nodes[1].output, "Checking tests.");
    assert_eq!(nodes.last().unwrap().output, "测试通过。");
    assert!(
        !records
            .iter()
            .any(|r| matches!(r.record, RecordData::Result(_)))
    );
    assert_single_report(&normalizer.finish(), json!("测试通过。"));
    let serialized = serde_json::to_string(&records).unwrap();
    for hidden in [
        "private reasoning",
        "hidden tool detail",
        "internal summary",
        "hidden text",
    ] {
        assert!(!serialized.contains(hidden));
    }
}

#[test]
fn error_and_incomplete_streams_cannot_report_success() {
    for error in [
        Some(json!({"type":"error","error":"authentication failed"})),
        Some(message(
            json!([{"type":"error","kind":"contextLengthExceeded","message":"context too large"}]),
        )),
        None,
    ] {
        let mut normalizer = GooseNormalizer::default();
        normalizer.accept(text("Partial answer"));
        if let Some(error) = error {
            let records = normalizer.accept(error);
            assert!(records.iter().any(
                |r| matches!(&r.record, RecordData::Trace(n) if n.status == TraceState::Failed)
            ));
            normalizer.accept(json!({"type":"complete"}));
        }
        assert!(normalizer.failed());
        assert!(normalizer.finish().is_empty());
    }
}

#[test]
fn failed_tool_can_recover_and_duplicates_do_not_regress_its_terminal_state() {
    let mut normalizer = GooseNormalizer::default();
    normalizer.accept(tool_request("call-1"));
    let failed = message(
        json!([{"type":"toolResponse","id":"call-1","toolResult":{"status":"success","value":{"content":[{"type":"text","text":"failed"}],"isError":true}}}]),
    );
    let records = normalizer.accept(failed.clone());
    assert!(matches!(&records[0].record, RecordData::Trace(n) if n.status == TraceState::Failed));
    assert!(normalizer.accept(tool_request("call-1")).is_empty());
    assert!(normalizer.accept(failed).is_empty());
    normalizer.accept(text("Recovered"));
    normalizer.accept(json!({"type":"complete"}));
    normalizer.accept(json!({"type":"error","error":"late error"}));
    assert!(!normalizer.failed());
    assert_single_report(&normalizer.finish(), json!("Recovered"));
}

#[test]
fn tool_protocol_errors_are_visible_without_panicking() {
    let mut normalizer = GooseNormalizer::default();
    let invalid = message(
        json!([{"type":"toolRequest","id":"bad","toolCall":{"status":"error","error":"invalid arguments"}}]),
    );
    let records = normalizer.accept(invalid);
    assert!(
        matches!(&records[0].record, RecordData::Trace(n) if n.status == TraceState::Failed && n.output == "invalid arguments")
    );
    normalizer.accept(tool_request("call-1"));
    let failed = message(
        json!([{"type":"toolResponse","id":"call-1","toolResult":{"status":"error","error":"tool unavailable"}}]),
    );
    let records = normalizer.accept(failed);
    assert!(
        matches!(&records[0].record, RecordData::Trace(n) if n.status == TraceState::Failed && n.output == "tool unavailable")
    );
    assert!(!normalizer.accept(message(json!([{"type":"toolResponse","id":"orphan","toolResult":{"status":"success","value":"unexpected"}}]))).is_empty());
}

#[test]
fn oversized_output_and_native_token_limit_never_become_a_complete_answer() {
    let mut normalizer = GooseNormalizer::default();
    let chunk = "\u{1}".repeat(100_000);
    normalizer.accept(text(&chunk));
    let records = normalizer.accept(text(&chunk));
    assert!(
        records
            .iter()
            .any(|r| matches!(&r.record, RecordData::Log(log) if log.diagnostic))
    );
    assert!(
        records
            .iter()
            .all(|r| serde_json::to_vec(r).unwrap().len() < wakeit::trace::RECORD_LIMIT)
    );
    assert!(normalizer.accept(text(&chunk)).is_empty());
    normalizer.accept(json!({"type":"complete"}));
    assert!(!normalizer.failed());
    assert!(normalizer.finish().is_empty());

    let mut normalizer = GooseNormalizer::default();
    normalizer.accept(text("partial"));
    normalizer.accept(json!({"type":"message","message":{"content":[],"metadata":{"outputTokenLimitReached":true}}}));
    normalizer.accept(json!({"type":"complete"}));
    assert!(normalizer.failed());
    assert!(normalizer.finish().is_empty());
}

#[test]
fn yaml_uses_aaif_goose_native_stdin_without_claiming_unverified_capabilities() {
    let config: Value = yaml_serde::from_str(include_str!("../../../agents/goose.yaml")).unwrap();
    assert_eq!(config["id"], "goose");
    assert!(config.get("models").is_none());
    assert!(config.get("session").is_none());
    let args = config["submit"]["args"].as_array().unwrap();
    assert!(args.windows(2).any(|w| w == [json!("goose"), json!("run")]));
    assert!(
        args.windows(2)
            .any(|w| w == [json!("--instructions"), json!("-")])
    );
    assert!(
        args.windows(2)
            .any(|w| w == [json!("--output-format"), json!("stream-json")])
    );
    assert_eq!(config["submit"]["stdin"], "{{task.prompt}}");
    assert!(
        !args
            .iter()
            .any(|arg| arg == "--resume" || arg == "--no-session")
    );
}
