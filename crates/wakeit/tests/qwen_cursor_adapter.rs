mod support;
use support::assert_single_report;

use serde_json::{Value, json};
use wakeit::qwen_cursor_adapter::{CursorNormalizer, QwenNormalizer, native_session_id};
use wakeit::trace::{RECORD_LIMIT, RecordData, StreamRecord, TraceKind, TraceNode, TraceState};

fn success(text: &str) -> Value {
    json!({"type":"result","subtype":"success","is_error":false,"result":text})
}
fn nodes(records: &[StreamRecord]) -> Vec<&TraceNode> {
    records
        .iter()
        .filter_map(|record| match &record.record {
            RecordData::Trace(node) => Some(node),
            _ => None,
        })
        .collect()
}
fn qwen_call() -> Value {
    json!({"type":"assistant","uuid":"tool1","message":{"id":"tool1","content":[{"type":"tool_use","id":"call1","name":"shell","input":{"command":"pwd"}}]}})
}
fn qwen_result(error: bool) -> Value {
    json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call1","is_error":error,"content":"tool output"}]}})
}
fn cursor_call(subtype: &str) -> Value {
    json!({"type":"tool_call","subtype":subtype,"call_id":"call1","tool_call":{"readToolCall":{"args":{"path":"README.md"},"result":{"success":{"content":"read output"}}}}})
}

#[test]
fn qwen_fixture_preserves_native_tool_identity_and_omits_thinking() {
    let mut adapter = QwenNormalizer::default();
    let mut records = vec![];
    for line in include_str!("fixtures/additional-cli/qwen-code.jsonl").lines() {
        records.extend(adapter.accept(serde_json::from_str(line).unwrap()));
    }
    assert!(!adapter.failed());
    let traces = nodes(&records);
    assert_eq!(traces.len(), 4);
    assert_eq!(traces[1].id, "call1");
    assert_eq!(traces[1].kind, TraceKind::Command);
    assert_eq!(traces[1].input, json!({"command":"pwd"}));
    assert_eq!(traces[2].status, TraceState::Succeeded);
    assert_eq!(traces[1].started_at, traces[2].started_at);
    assert!(traces[0].started_at.is_none());
    assert_single_report(&adapter.finish(), json!("Done."));
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("private hidden thinking")
    );
    assert!(
        records
            .iter()
            .all(|r| !matches!(r.record, RecordData::Result(_) | RecordData::Session(_)))
    );
}

#[test]
fn qwen_tool_failure_can_be_recovered_by_a_successful_turn() {
    let mut adapter = QwenNormalizer::default();
    adapter.accept(qwen_call());
    let result = adapter.accept(qwen_result(true));
    assert_eq!(nodes(&result)[0].status, TraceState::Failed);
    adapter.accept(success("Recovered"));
    assert!(!adapter.failed());
    assert_single_report(&adapter.finish(), json!("Recovered"));
}

#[test]
fn qwen_native_parent_is_retained_without_self_parenting() {
    let mut adapter = QwenNormalizer::default();
    let mut event = qwen_call();
    event["parent_tool_use_id"] = json!("parent-task");
    assert_eq!(
        nodes(&adapter.accept(event))[0].parent_id.as_deref(),
        Some("parent-task")
    );
    let mut adapter = QwenNormalizer::default();
    let mut event = qwen_call();
    event["parent_tool_use_id"] = json!("call1");
    assert_eq!(nodes(&adapter.accept(event))[0].parent_id, None);
}

#[test]
fn terminal_errors_never_become_success_or_results() {
    let mut adapter = QwenNormalizer::default();
    let error = json!({"type":"result","subtype":"error_during_execution","is_error":true,"error":{"message":"Native failure"}});
    let records = adapter.accept(error);
    adapter.accept(success("Late success"));
    assert!(adapter.failed());
    assert!(adapter.finish().is_empty());
    assert!(
        serde_json::to_string(&records)
            .unwrap()
            .contains("Native failure")
    );
}

#[test]
fn missing_or_inconsistent_terminal_results_fail_closed() {
    let mut adapter = QwenNormalizer::default();
    adapter.accept(
        json!({"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}),
    );
    assert!(adapter.failed());
    assert!(adapter.finish().is_empty());
    let mut adapter = QwenNormalizer::default();
    adapter.accept(qwen_call());
    adapter.accept(success("tools still pending"));
    assert!(adapter.failed());
    assert!(adapter.finish().is_empty());
    let mut adapter = CursorNormalizer::default();
    adapter.accept(json!({"type":"result","subtype":"success","is_error":false}));
    assert!(adapter.failed());
}

#[test]
fn cursor_fixture_uses_complete_messages_and_authoritative_result() {
    let mut adapter = CursorNormalizer::default();
    let mut records = vec![];
    for line in include_str!("fixtures/additional-cli/cursor.jsonl").lines() {
        records.extend(adapter.accept(serde_json::from_str(line).unwrap()));
    }
    assert!(!adapter.failed());
    let traces = nodes(&records);
    assert_eq!(traces.len(), 4);
    assert_eq!(traces[1].id, "cursor-call1");
    assert_eq!(traces[1].kind, TraceKind::Tool);
    assert_eq!(traces[2].status, TraceState::Succeeded);
    assert_eq!(traces[2].output["success"]["content"], "Fixture README");
    assert_single_report(&adapter.finish(), json!("Reading.Done."));
}

#[test]
fn duplicate_tool_events_cannot_regress_completed_nodes() {
    let mut adapter = CursorNormalizer::default();
    assert_eq!(adapter.accept(cursor_call("started")).len(), 1);
    assert_eq!(adapter.accept(cursor_call("completed")).len(), 1);
    assert!(adapter.accept(cursor_call("started")).is_empty());
    assert!(adapter.accept(cursor_call("completed")).is_empty());
    adapter.accept(success("Done"));
    adapter.accept(cursor_call("started"));
    adapter.accept(
        json!({"type":"result","subtype":"error_during_execution","is_error":true,"error":"late"}),
    );
    assert!(!adapter.failed());
    assert_single_report(&adapter.finish(), json!("Done"));
}

#[test]
fn cursor_function_arguments_and_optional_partial_flushes_do_not_duplicate_text() {
    let mut adapter = CursorNormalizer::default();
    assert!(adapter.accept(json!({"type":"assistant","timestamp_ms":1,"message":{"content":[{"type":"text","text":"delta"}]}})).is_empty());
    assert!(adapter.accept(json!({"type":"assistant","timestamp_ms":2,"model_call_id":"m1","message":{"content":[{"type":"text","text":"duplicate flush"}]}})).is_empty());
    let started = json!({"type":"tool_call","subtype":"started","call_id":"function1","tool_call":{"function":{"name":"custom_tool","arguments":"{\"key\":1}"}}});
    let records = adapter.accept(started);
    assert_eq!(nodes(&records)[0].input, json!({"key":1}));
}

#[test]
fn unmatched_results_are_fatal_without_dumping_native_payload() {
    let mut adapter = CursorNormalizer::default();
    let records = adapter.accept(cursor_call("completed"));
    adapter.accept(success("invalid success"));
    assert!(adapter.failed());
    assert!(adapter.finish().is_empty());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("read output")
    );
}

#[test]
fn unknown_events_are_deduplicated_and_session_identity_is_only_metadata() {
    let mut adapter = QwenNormalizer::default();
    let event = json!({"type":"future_event","private":"never log this payload"});
    assert_eq!(adapter.accept(event.clone()).len(), 1);
    assert!(adapter.accept(event).is_empty());
    assert_eq!(
        native_session_id(&json!({"session_id":"11111111-1111-4111-8111-111111111111"})),
        Some("11111111-1111-4111-8111-111111111111")
    );
    assert!(native_session_id(&json!({"session_id":"../path"})).is_none());
}

#[test]
fn combined_tool_payloads_stay_within_the_public_record_limit() {
    let mut adapter = QwenNormalizer::default();
    let mut call = qwen_call();
    call["message"]["content"][0]["input"] = json!({"large":"x".repeat(600_000)});
    let start = adapter.accept(call);
    let mut output = qwen_result(false);
    output["message"]["content"][0]["content"] = json!("y".repeat(600_000));
    let end = adapter.accept(output);
    for record in start.iter().chain(end.iter()) {
        assert!(serde_json::to_vec(record).unwrap().len() <= RECORD_LIMIT);
    }
    assert!(nodes(&end)[0].output.as_str().unwrap().contains("omitted"));
    adapter.accept(success("Done"));
    assert!(!adapter.failed());
}

#[test]
fn qwen_and_cursor_trace_budgets_never_override_native_terminal_outcomes() {
    for protocol in ["qwen", "cursor"] {
        for budget in ["messages", "tools", "pending"] {
            for failed in [false, true] {
                let mut qwen = QwenNormalizer::default();
                let mut cursor = CursorNormalizer::default();
                let mut diagnostics = 0;
                let count = if budget == "pending" { 15 } else { 10_001 };
                for i in 0..count {
                    let input = if budget == "pending" {
                        "x".repeat(600_000)
                    } else {
                        "small".into()
                    };
                    let event = if budget == "messages" {
                        json!({"type":"assistant","message":{"id":format!("m-{i}"),"content":[{"type":"text","text":"partial"}]}})
                    } else if protocol == "qwen" {
                        json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":format!("tool-{i}"),"name":"read","input":{"path":input}}]}})
                    } else {
                        json!({"type":"tool_call","subtype":"started","call_id":format!("tool-{i}"),"tool_call":{"readToolCall":{"args":{"path":input}}}})
                    };
                    let records = if protocol == "qwen" {
                        qwen.accept(event)
                    } else {
                        cursor.accept(event)
                    };
                    diagnostics += records
                        .iter()
                        .filter(|r| matches!(&r.record,RecordData::Log(l) if l.diagnostic))
                        .count();
                }
                assert!(
                    diagnostics > 0,
                    "{protocol}/{budget} must mark omitted trace"
                );
                let terminal = if failed {
                    json!({"type":"result","subtype":"error_during_execution","is_error":true,"error":"native failure"})
                } else {
                    success("Completed")
                };
                if protocol == "qwen" {
                    qwen.accept(terminal);
                    assert_eq!(qwen.failed(), failed, "{protocol}/{budget}");
                    assert_eq!(qwen.finish().is_empty(), failed);
                } else {
                    cursor.accept(terminal);
                    assert_eq!(cursor.failed(), failed, "{protocol}/{budget}");
                    assert_eq!(cursor.finish().is_empty(), failed);
                }
            }
        }
    }
}

#[test]
fn oversized_final_reports_are_marked_incomplete_without_failing_native_execution() {
    for protocol in ["qwen", "cursor"] {
        let mut qwen = QwenNormalizer::default();
        let mut cursor = CursorNormalizer::default();
        let terminal = success(&"中".repeat(400_000));
        let records = if protocol == "qwen" {
            qwen.accept(terminal);
            assert!(!qwen.failed());
            qwen.finish()
        } else {
            cursor.accept(terminal);
            assert!(!cursor.failed());
            cursor.finish()
        };
        assert!(
            matches!(&records[0].record,RecordData::Result(v) if v["truncated"]==true && v["preview"].as_str().unwrap().len()<=32*1024)
        );
        assert!(serde_json::to_vec(&records[0]).unwrap().len() < RECORD_LIMIT);
    }
}
