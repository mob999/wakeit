//! Synthetic commands only: complete syntax validation must not retain or spool
//! unbounded native output, stop draining pipes, or confuse a preview with data.
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wakeit::{config::*, process};

async fn run(code: &str, format: OutputFormat) -> process::Output {
    let directory = tempfile::tempdir().unwrap();
    let task: TaskConfig = serde_json::from_value(serde_json::json!({
        "version":1,"id":"bounded-output","name":"Synthetic output","runner":"fixture",
        "agent":"fixture","cwd":directory.path(),"prompt":"fixture","schedule":null,
    }))
    .unwrap();
    let spec = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), code.into()],
        ..Default::default()
    };
    process::execute_output_named_format(
        &spec,
        &task,
        Uuid::new_v4(),
        None,
        Duration::from_secs(20),
        CancellationToken::new(),
        None,
        format,
        None,
    )
    .await
    .unwrap()
}
fn spec(format: OutputFormat) -> OutputSpec {
    OutputSpec {
        format,
        result_pointer: None,
    }
}

#[tokio::test]
async fn complete_large_json_remains_successful_and_explicitly_incomplete() {
    let out = run(
        r#"console.log(JSON.stringify({report:'中文😀'.repeat(300000),answer:42}))"#,
        OutputFormat::Json,
    )
    .await;
    assert_eq!(out.code, Some(0));
    assert!(out.confirmed && out.stopped.is_none() && out.stdout_truncated);
    assert!(out.stdout.len() <= 1024 * 1024 + 3);
    let value = out.result_value(&spec(OutputFormat::Json)).unwrap();
    assert_eq!(value["truncated"], true);
    assert!(value["preview"].as_str().unwrap().len() <= 64 * 1024);
    // A preview must not silently substitute for a required complete DAG result.
    assert!(
        wakeit::dag::render(
            "{{ nodes.source.result }}",
            &[("source".into(), value)].into()
        )
        .is_err()
    );
    // Missing pointer in a retained prefix is not evidence of invalid Agent output.
    assert_eq!(
        out.result_value(&OutputSpec {
            format: OutputFormat::Json,
            result_pointer: Some("/answer".into())
        })
        .unwrap()["truncated"],
        true
    );
}

#[tokio::test]
async fn jsonl_validates_many_records_and_a_single_large_record_without_retaining_them() {
    for code in [
        r#"for(let i=0;i<40000;i++) console.log(JSON.stringify({i,text:'中'.repeat(20)}))"#,
        r#"console.log(JSON.stringify({text:'z'.repeat(3*1024*1024)})); console.log('{"last":true}')"#,
    ] {
        let out = run(code, OutputFormat::Jsonl).await;
        assert_eq!(out.code, Some(0));
        assert!(out.confirmed && out.stdout_truncated);
        assert_eq!(
            out.result_value(&spec(OutputFormat::Jsonl)).unwrap()["truncated"],
            true
        );
    }
}

#[tokio::test]
async fn malformed_suffix_beyond_preview_is_still_a_semantic_error() {
    for (format, code) in [
        (
            OutputFormat::Json,
            r#"process.stdout.write('{"text":"'+'a'.repeat(2*1024*1024)+'","broken":}')"#,
        ),
        (
            OutputFormat::Jsonl,
            r#"console.log(JSON.stringify({text:'a'.repeat(2*1024*1024)})); console.log('{broken}')"#,
        ),
    ] {
        let out = run(code, format.clone()).await;
        assert_eq!(out.code, Some(0));
        assert!(out.confirmed && out.stopped.is_none() && out.stdout_truncated);
        assert!(out.result_value(&spec(format)).is_err());
    }
}

#[tokio::test]
async fn early_syntax_failure_does_not_close_or_stop_draining_the_agent_pipe() {
    let out = run(r#"process.stdout.write('{broken}\n'); for(let i=0;i<300;i++) process.stdout.write('x'.repeat(8192));"#, OutputFormat::Jsonl).await;
    assert_eq!(out.code, Some(0));
    assert!(out.confirmed && out.stopped.is_none() && out.stdout_truncated);
    assert!(out.result_value(&spec(OutputFormat::Jsonl)).is_err());
}

#[tokio::test]
async fn text_preview_is_explicit_and_small_json_pointer_behavior_is_preserved() {
    let out = run(
        r#"process.stdout.write('x'.repeat(2*1024*1024));"#,
        OutputFormat::Text,
    )
    .await;
    assert_eq!(out.code, Some(0));
    assert!(out.confirmed && out.stdout_truncated);
    assert_eq!(
        out.result_value(&spec(OutputFormat::Text)).unwrap()["truncated"],
        true
    );
    let out = run(
        r#"console.log(JSON.stringify({answer:{text:'完整 😀'}}))"#,
        OutputFormat::Json,
    )
    .await;
    assert!(!out.stdout_truncated);
    assert_eq!(
        out.result_value(&OutputSpec {
            format: OutputFormat::Json,
            result_pointer: Some("/answer/text".into())
        })
        .unwrap(),
        "完整 😀"
    );
    assert!(
        out.result_value(&OutputSpec {
            format: OutputFormat::Json,
            result_pointer: Some("/missing".into())
        })
        .is_err()
    );
    let out = run(r#"console.log('{"a":1}\r\n{"b":2}')"#, OutputFormat::Jsonl).await;
    assert_eq!(
        out.result_value(&spec(OutputFormat::Jsonl)).unwrap(),
        serde_json::json!([{"a":1},{"b":2}])
    );
}

#[tokio::test]
async fn validation_rejects_invalid_unicode_and_unpaired_surrogates_beyond_preview() {
    for tail in ["\\ud800", "\\udc00", "\\ud800\\u0041", "raw-invalid"] {
        let code = if tail == "raw-invalid" {
            r#"process.stdout.write('{"text":"'+'a'.repeat(2*1024*1024)); process.stdout.write(Buffer.from([0xff])); process.stdout.write('"}')"#.to_owned()
        } else {
            format!(
                r#"process.stdout.write('{{"text":"'+'a'.repeat(2*1024*1024)+{tail}+'"}}')"#,
                tail = serde_json::to_string(tail).unwrap()
            )
        };
        let out = run(&code, OutputFormat::Json).await;
        assert!(out.confirmed && out.stopped.is_none());
        assert!(
            out.result_value(&spec(OutputFormat::Json)).is_err(),
            "{tail}"
        );
    }
}

async fn native(protocol: &str, code: &str) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    tokio::process::Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args(["adapter", "stream", protocol, "--", "node", "-e", code])
        .current_dir(dir.path())
        .output()
        .await
        .unwrap()
}
fn records(bytes: &[u8]) -> Vec<wakeit::trace::StreamRecord> {
    let mut lines = wakeit::trace::Lines::default();
    let mut records = lines.push(bytes);
    records.extend(lines.finish());
    records.into_iter().map(Result::unwrap).collect()
}

#[tokio::test]
async fn native_large_terminal_frame_keeps_session_and_actual_success() {
    use wakeit::trace::RecordData;
    // Session and native terminal result intentionally share one > 1 MiB frame.
    let out = native("claude-code", r#"console.log(JSON.stringify({type:'result',subtype:'success',is_error:false,session_id:'d524c274-cf24-425c-867a-e9f35ae4e749',result:'完整'.repeat(400000)}))"#).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let records = records(&out.stdout);
    assert!(records.iter().any(|r| matches!(&r.record, RecordData::Session(info) if info.id == "d524c274-cf24-425c-867a-e9f35ae4e749")));
    assert!(records.iter().any(|r| matches!(&r.record, RecordData::Result(value) if value["truncated"] == true && value["preview"].is_string())));
    assert!(out.stdout.len() < 1000 * 1024);
}

#[tokio::test]
async fn native_large_tool_observation_does_not_discard_final_result_or_fail_execution() {
    use wakeit::trace::RecordData;
    let out = native("codex", r#"
console.log(JSON.stringify({type:'thread.started',thread_id:'83ef7e5e-e6d9-44c8-95bb-e01c9a993c15'}));
console.log(JSON.stringify({type:'item.completed',item:{id:'tool',type:'command_execution',command:'synthetic',exit_code:0,aggregated_output:'x'.repeat(3*1024*1024)}}));
console.log(JSON.stringify({type:'item.completed',item:{id:'answer',type:'agent_message',text:'complete report'}}));
console.log(JSON.stringify({type:'turn.completed'}));
"#).await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let records = records(&out.stdout);
    assert!(records.iter().any(|r| matches!(&r.record, RecordData::Trace(node) if node.id == "tool" && node.output["truncated"] == true)));
    assert!(
        records
            .iter()
            .any(|r| matches!(&r.record, RecordData::Result(value) if value == "complete report"))
    );
}

#[tokio::test]
async fn malformed_or_explicitly_failed_native_terminal_output_does_not_become_success() {
    for code in [
        r#"process.stdout.write('{"type":"result","result":"'+'x'.repeat(2*1024*1024)+'",broken}')"#,
        r#"console.log(JSON.stringify({type:'result',subtype:'error_during_execution',is_error:true,result:'x'.repeat(2*1024*1024)}))"#,
    ] {
        let out = native("claude-code", code).await;
        assert!(!out.status.success());
        assert!(
            !records(&out.stdout)
                .iter()
                .any(|r| matches!(r.record, wakeit::trace::RecordData::Result(_)))
        );
    }
}
