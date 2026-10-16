use std::process::Stdio;
use tokio::{io::AsyncWriteExt, process::Command};
use wakeit::trace::{RecordData, StreamRecord};

#[tokio::test]
async fn native_helpers_frame_unicode_prompts_and_preserve_exit_codes() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("native-agent.cjs");
    let prompt = "中文\n\"quote\" \\ literal $(not-a-shell) {\"event\":\"result\"}\n".repeat(4000);
    let expected = dir.path().join("expected.txt");
    std::fs::write(&expected, &prompt).unwrap();
    std::fs::write(&script, r#"
const fs = require('node:fs');
const [protocol, fixture, expected, failure] = process.argv.slice(2);
if (failure) process.exit(7);
const output = () => process.stdout.write(fs.readFileSync(fixture));
if (protocol === 'agy') {
  const init = JSON.parse(fs.readFileSync(fixture, 'utf8').split('\n')[0]);
  init.padding = 'x'.repeat(200000);
  // Fill stdout before reading stdin: the bridge must drain both concurrently.
  process.stdout.write(JSON.stringify(init) + '\n', () => {
    let input = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', chunk => input += chunk);
    process.stdin.on('end', () => {
      const lines = input.trimEnd().split('\n');
      if (lines.length !== 1) process.exit(8);
      const event = JSON.parse(lines[0]);
      if (event.event !== 'user' || event.message.content !== fs.readFileSync(expected, 'utf8')) process.exit(9);
      output();
    });
  });
} else output();
"#).unwrap();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for (protocol, fixture) in [
        ("kimi-code", fixtures.join("kimi-code/stream.jsonl")),
        ("agy", fixtures.join("agy-stream.ndjson")),
        ("gemini-cli", fixtures.join("trace/gemini-cli.jsonl")),
    ] {
        for failure in [false, true] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_wakeit"));
            command
                .args(["adapter", "stream", protocol, "--", "node"])
                .arg(&script)
                .arg(protocol)
                .arg(&fixture)
                .arg(&expected);
            if failure {
                command.arg("fail");
            }
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            if protocol == "agy" {
                // A failing native child may close input immediately.
                let _ = stdin.write_all(prompt.as_bytes()).await;
            }
            drop(stdin);
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(20), child.wait_with_output())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(
                output.status.code(),
                Some(if failure { 7 } else { 0 }),
                "{protocol}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            if !failure {
                let records: Vec<_> = output
                    .stdout
                    .split(|b| *b == b'\n')
                    .filter(|line| !line.is_empty())
                    .map(|line| StreamRecord::parse(line).unwrap())
                    .collect();
                assert_eq!(
                    records
                        .iter()
                        .filter(|r| matches!(r.record, RecordData::Result(_)))
                        .count(),
                    1,
                    "{protocol}"
                );
                assert!(
                    records
                        .iter()
                        .any(|r| matches!(r.record, RecordData::Trace(_))),
                    "{protocol}"
                );
                assert!(
                    records
                        .iter()
                        .any(|r| matches!(&r.record, RecordData::Session(s) if !s.persisted)),
                    "{protocol}"
                );
            }
        }
    }
}
