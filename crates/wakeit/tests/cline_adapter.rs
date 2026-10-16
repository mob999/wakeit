use serde_json::{Value, json};
use wakeit::{
    cline_adapter::ClineNormalizer,
    trace::{RECORD_LIMIT, RecordData, TraceKind, TraceState},
};

#[test]
fn cline_snapshots_tools_and_authoritative_report_are_separate() {
    let mut n = ClineNormalizer::default();
    let records: Vec<_> = include_str!("fixtures/cline-stream.jsonl")
        .lines()
        .flat_map(|line| n.accept(serde_json::from_str(line).unwrap()))
        .collect();
    assert!(!n.failed());
    assert!(
        !serde_json::to_string(&records)
            .unwrap()
            .contains("private reasoning")
    );
    let messages: Vec<_> = records
        .iter()
        .filter_map(|r| match &r.record {
            RecordData::Trace(node) if node.kind == TraceKind::Message => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 3);
    assert!(messages.iter().all(|node| node.id == messages[0].id));
    assert_eq!(messages[1].output, "正在检查");
    let tool = records
        .iter()
        .filter_map(|r| match &r.record {
            RecordData::Trace(node) if node.kind == TraceKind::Command => Some(node),
            _ => None,
        })
        .next_back()
        .unwrap();
    assert_eq!(tool.status, TraceState::Succeeded);
    assert_eq!(tool.input["command"], "printf '中文'");
    assert_eq!(tool.output, "中文");
    assert_eq!(
        tool.started_at.unwrap().to_rfc3339(),
        "2026-10-16T01:00:05+00:00"
    );
    let result = n.finish();
    assert!(matches!(&result[0].record, RecordData::Result(v) if v == "# 检查结果\n\n完成。"));
}

#[test]
fn cline_requires_successful_terminal_result_even_when_native_exit_is_zero() {
    for tail in [
        Value::Null,
        json!({"type":"run_result","finishReason":"aborted","text":"partial"}),
        json!({"type":"run_result","finishReason":"completed"}),
    ] {
        let mut n = ClineNormalizer::default();
        n.accept(json!({"type":"agent_event","event":{"type":"done","reason":"completed","text":"partial"}}));
        if !tail.is_null() {
            n.accept(tail);
        }
        assert!(n.failed());
        assert!(n.finish().is_empty());
    }
    let mut n = ClineNormalizer::default();
    n.accept(json!({"type":"run_result","finishReason":"completed","text":"ok"}));
    n.accept(json!({"type":"run_aborted","reason":"external_abort"}));
    assert!(n.failed());
    assert!(n.finish().is_empty());
}

#[test]
fn cline_retains_failure_latch_and_recovers_only_recoverable_native_errors() {
    for recoverable in [false, true] {
        let mut n = ClineNormalizer::default();
        n.accept(json!({"type":"agent_event","event":{"type":"error","recoverable":recoverable,"error":{"message":"test failure"}}}));
        n.accept(json!({"type":"run_result","finishReason":"completed","text":"ok"}));
        assert_eq!(n.failed(), !recoverable);
        assert_eq!(n.finish().len(), usize::from(recoverable));
    }
}

#[test]
fn cline_late_duplicate_tools_do_not_regress_or_invent_start_times() {
    let mut n = ClineNormalizer::default();
    let event = json!({"ts":"2026-10-16T01:00:05Z","type":"agent_event","event":{"type":"content_end","contentType":"tool","toolCallId":"a","toolName":"read_file","output":"done"}});
    let records = n.accept(event.clone());
    let RecordData::Trace(node) = &records[0].record else {
        panic!()
    };
    assert!(node.started_at.is_none());
    assert!(node.finished_at.is_some());
    assert!(n.accept(event).is_empty());
    assert!(n.accept(json!({"type":"agent_event","event":{"type":"content_start","contentType":"tool","toolCallId":"a","toolName":"read_file"}})).is_empty());
}

#[test]
fn cline_delta_only_text_and_oversized_content_stay_bounded() {
    let mut n = ClineNormalizer::default();
    for text in ["中文", "\nquotes \" / "] {
        let records = n.accept(json!({"type":"agent_event","event":{"type":"content_start","contentType":"text","text":text}}));
        assert_eq!(records.len(), 1);
    }
    let end = n.accept(json!({"type":"agent_event","event":{"type":"content_end","contentType":"text","text":"中文\nquotes \" / "}}));
    assert!(
        matches!(&end[0].record, RecordData::Trace(node) if node.output == "中文\nquotes \" / ")
    );
    let huge = "\u{0}".repeat(RECORD_LIMIT);
    let diagnostic = n.accept(json!({"type":"agent_event","event":{"type":"content_start","contentType":"text","text":huge}}));
    assert!(matches!(&diagnostic[0].record, RecordData::Log(log) if log.diagnostic));
    n.accept(json!({"type":"run_result","finishReason":"completed","text":"ok"}));
    assert!(!n.failed());
    assert_eq!(n.finish().len(), 1);
}
