use serde_json::json;
use wakeit_agent_runtime::kimi_acp::Normalizer;
use wakeit_plugin_sdk::trace::{RecordData, TraceState};
#[test]
fn tool_failure_can_recover_but_partial_or_private_output_is_not_the_report() {
    let mut normalizer = Normalizer::default();
    assert!(normalizer.update(&json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"PRIVATE"}})).unwrap().is_empty());
    normalizer.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Working"}})).unwrap();
    normalizer.update(&json!({"sessionUpdate":"tool_call","toolCallId":"tool","kind":"execute","title":"Command","status":"pending"})).unwrap();
    let failed=normalizer.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"failed","rawOutput":"Tool failed"})).unwrap();
    assert!(
        failed
            .iter()
            .any(|r| matches!(&r.record,RecordData::Trace(n) if n.status==TraceState::Failed))
    );
    assert!(normalizer.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"in_progress"})).unwrap().is_empty());
    normalizer.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Recovered answer"}})).unwrap();
    let final_records = normalizer.finish("end_turn").unwrap();
    let reports = final_records
        .iter()
        .filter_map(|r| {
            if let RecordData::Result(v) = &r.record {
                Some(v)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(reports, vec![&json!("Recovered answer")]);
}
#[test]
fn incomplete_or_cancelled_turns_do_not_publish_reports() {
    let mut incomplete = Normalizer::default();
    incomplete
        .update(
            &json!({"sessionUpdate":"tool_call","toolCallId":"unfinished","status":"in_progress"}),
        )
        .unwrap();
    incomplete.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"partial"}})).unwrap();
    assert!(incomplete.finish("end_turn").is_err());
    let cancelled = incomplete.finish("cancelled").unwrap();
    assert!(
        !cancelled
            .iter()
            .any(|r| matches!(r.record, RecordData::Result(_)))
    );
    assert!(
        cancelled
            .iter()
            .any(|r| matches!(&r.record,RecordData::Trace(n) if n.status==TraceState::Cancelled))
    );
}
#[test]
fn oversized_tools_and_messages_preserve_completion_and_mark_only_the_shortened_content() {
    let mut normalizer = Normalizer::default();
    normalizer.update(&json!({"sessionUpdate":"tool_call","toolCallId":"large","rawInput":{"value":"中".repeat(300000)}})).unwrap();
    let records=normalizer.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"large","status":"completed","rawOutput":"x".repeat(500000)})).unwrap();
    assert!(records.iter().any(|r|matches!(&r.record,RecordData::Trace(n) if n.status==TraceState::Succeeded && n.output["truncated"]==true)));
    normalizer.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Complete final report"}})).unwrap();
    assert!(
        normalizer
            .finish("end_turn")
            .unwrap()
            .iter()
            .any(|r| matches!(&r.record,RecordData::Result(v) if v=="Complete final report"))
    );
    let mut large = Normalizer::default();
    large.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"界".repeat(500000)}})).unwrap();
    assert!(
        large
            .finish("end_turn")
            .unwrap()
            .iter()
            .any(|r| matches!(&r.record,RecordData::Result(v) if v["truncated"]==true))
    );
}
