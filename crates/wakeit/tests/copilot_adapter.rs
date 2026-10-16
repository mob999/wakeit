use serde_json::json;
use wakeit::{
    copilot_adapter::CopilotNormalizer,
    stream_adapter::Normalizer,
    trace::{RecordData, TraceState},
};
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
    assert_eq!(n.finish().len(), 1);
    assert!(matches!(&n.finish()[0].record,RecordData::Result(v) if v=="# Final report"));
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
