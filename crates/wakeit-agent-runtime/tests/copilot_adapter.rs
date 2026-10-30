use wakeit_agent_runtime::copilot::CopilotNormalizer;
use wakeit_agent_runtime::stream::Normalizer;
mod support;
use support::assert_single_report;

use serde_json::json;
use wakeit_core::trace::{RecordData, TraceState};
#[test]
fn cli_jsonl_tracks_public_messages_and_tools_and_emits_one_final_report() {
    let mut n = CopilotNormalizer::default();
    let mut output = vec![];
    for line in include_str!("fixtures/copilot/stream.jsonl").lines() {
        output.extend(n.accept(serde_json::from_str(line).unwrap()));
    }
    assert!(!n.failed());
    assert!(
        !serde_json::to_string(&output)
            .unwrap()
            .contains("PRIVATE HIDDEN REASONING")
    );
    assert!(output.iter().any(|r|matches!(&r.record,RecordData::Trace(t) if t.status==TraceState::Running&&t.input["command"]=="printf fixture")));
    assert!(output.iter().any(|r|matches!(&r.record,RecordData::Trace(t) if t.status==TraceState::Succeeded&&t.output["content"]=="fixture")));
    assert!(
        !output
            .iter()
            .any(|r| matches!(r.record, RecordData::Result(_)))
    );
    assert_single_report(&n.finish(), json!("# Final report"));
}
#[test]
fn missing_failed_or_incomplete_terminal_events_do_not_publish_reports() {
    for code in [None, Some(1)] {
        let mut n = CopilotNormalizer::default();
        n.accept(json!({"type":"assistant.message","data":{"messageId":"m","content":"partial"}}));
        if let Some(code) = code {
            n.accept(json!({"type":"result","exitCode":code}));
        }
        assert!(n.failed());
        assert!(n.finish().is_empty());
    }
    let mut n = CopilotNormalizer::default();
    n.accept(json!({"type":"tool.execution_start","data":{"toolCallId":"t","toolName":"bash","arguments":{}}}));
    n.accept(json!({"type":"result","exitCode":0}));
    assert!(n.failed());
}
#[test]
fn duplicate_and_late_events_never_regress_a_finished_tool() {
    let mut n = CopilotNormalizer::default();
    let start = json!({"type":"tool.execution_start","data":{"toolCallId":"t","toolName":"bash","arguments":{}}});
    n.accept(start.clone());
    n.accept(json!({"type":"tool.execution_complete","data":{"toolCallId":"t","success":false,"error":{"code":"denied"}}}));
    assert!(n.accept(start).is_empty());
    assert!(
        n.accept(
            json!({"type":"tool.execution_complete","data":{"toolCallId":"t","success":true}})
        )
        .is_empty()
    );
}
#[test]
fn codex_and_claude_defer_reports_until_process_success_instead_of_emitting_twice() {
    for (protocol, message, done) in [
        (
            "codex",
            json!({"type":"item.completed","item":{"id":"m","type":"agent_message","text":"report"}}),
            json!({"type":"turn.completed"}),
        ),
        (
            "claude-code",
            json!({"type":"assistant","message":{"id":"m","content":[{"type":"text","text":"report"}]}}),
            json!({"type":"result","subtype":"success","is_error":false,"result":"report"}),
        ),
    ] {
        let mut n = Normalizer::new(protocol).unwrap();
        assert!(
            n.accept(message)
                .iter()
                .all(|r| !matches!(r.record, RecordData::Result(_)))
        );
        assert!(n.finish().is_empty());
        assert!(
            n.accept(done)
                .iter()
                .all(|r| !matches!(r.record, RecordData::Result(_)))
        );
        assert_eq!(n.finish().len(), 1);
    }
}

fn copilot_done(n: &mut CopilotNormalizer, failed: bool) {
    n.accept(
        json!({"type":"assistant.message","data":{"messageId":"final","content":"Completed"}}),
    );
    n.accept(json!({"type":"result","exitCode":if failed {1} else {0}}));
}

#[test]
fn large_copilot_tool_payloads_are_previews_not_execution_failures() {
    let mut n = CopilotNormalizer::default();
    let start=n.accept(json!({"type":"tool.execution_start","data":{"toolCallId":"large","toolName":"read","arguments":{"path":"x".repeat(400_000)}}}));
    assert!(
        start
            .iter()
            .any(|r| matches!(&r.record,RecordData::Trace(t) if t.input["truncated"]==true))
    );
    let end=n.accept(json!({"type":"tool.execution_complete","data":{"toolCallId":"large","success":true,"result":{"text":"x".repeat(400_000)}}}));
    assert!(end.iter().any(|r|matches!(&r.record,RecordData::Trace(t) if t.status==TraceState::Succeeded && t.output["truncated"]==true)));
    for record in start.iter().chain(end.iter()) {
        assert!(serde_json::to_vec(record).unwrap().len() < wakeit_core::trace::RECORD_LIMIT);
    }
    copilot_done(&mut n, false);
    assert!(!n.failed());
    assert_single_report(&n.finish(), json!("Completed"));
}

#[test]
fn copilot_large_message_deltas_preserve_native_success_and_authoritative_final_text() {
    let mut n = CopilotNormalizer::default();
    for _ in 0..3 {
        n.accept(json!({"type":"assistant.message_delta","data":{"messageId":"large","deltaContent":"x".repeat(100_000)}}));
    }
    let text = "完整结果".repeat(30_000);
    n.accept(json!({"type":"assistant.message","data":{"messageId":"large","content":text}}));
    n.accept(json!({"type":"result","exitCode":0}));
    assert!(!n.failed());
    assert_single_report(&n.finish(), json!(text));
}

#[test]
fn copilot_history_budgets_keep_reading_terminal_outcomes() {
    for budget in ["events", "nodes", "pending", "messages"] {
        for failed in [false, true] {
            let mut n = CopilotNormalizer::default();
            let mut diagnostics = 0;
            let count = match budget {
                "events" => 100_001,
                "nodes" => 10_001,
                _ => 35,
            };
            for i in 0..count {
                let event = match budget {
                    "events" => json!({"id":format!("event-{i}"),"type":"future_event"}),
                    "nodes" => {
                        json!({"type":"assistant.message","data":{"messageId":format!("message-{i}"),"content":"partial"}})
                    }
                    "messages" => {
                        json!({"type":"assistant.message_delta","data":{"messageId":format!("message-{i}"),"deltaContent":"x".repeat(250_000)}})
                    }
                    _ => {
                        json!({"type":"tool.execution_start","data":{"toolCallId":format!("tool-{i}"),"toolName":"read","arguments":{"value":"x".repeat(250_000)}}})
                    }
                };
                diagnostics += n
                    .accept(event)
                    .iter()
                    .filter(|r| matches!(&r.record,RecordData::Log(l) if l.diagnostic))
                    .count();
            }
            assert!(diagnostics > 0, "budget {budget} must be observable");
            copilot_done(&mut n, failed);
            assert_eq!(n.failed(), failed, "budget {budget}");
            assert_eq!(n.finish().is_empty(), failed, "budget {budget}");
        }
    }
}

#[test]
fn oversized_copilot_final_reports_are_explicitly_incomplete() {
    let mut n = CopilotNormalizer::default();
    n.accept(
        json!({"type":"assistant.message","data":{"messageId":"m","content":"中".repeat(400_000)}}),
    );
    n.accept(json!({"type":"result","exitCode":0}));
    assert!(!n.failed());
    assert!(
        matches!(&n.finish()[0].record,RecordData::Result(v) if v["truncated"]==true && v["preview"].as_str().unwrap().len()<=32*1024)
    );
}
