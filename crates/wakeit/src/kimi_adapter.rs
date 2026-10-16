//! Kimi Code's current TypeScript CLI (`kimi --prompt --output-format stream-json`).
//! This is deliberately separate from the retired Python kimi-cli Wire protocol.
use crate::trace::{LogRecord, RecordData, StreamRecord, TraceKind, TraceNode, TraceState};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

#[derive(Default)]
pub struct KimiNormalizer {
    tools: BTreeMap<String, TraceNode>,
    retained_input_bytes: usize,
    next_message: usize,
    result: Option<Value>,
    completed: bool,
    invalid: bool,
    unknown: BTreeSet<String>,
}

impl KimiNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    fn diagnostic(&mut self, key: &str) -> Vec<StreamRecord> {
        let key: String = key.chars().take(128).collect();
        if self.unknown.len() >= 256 || !self.unknown.insert(key.clone()) {
            return vec![];
        }
        vec![StreamRecord::new(RecordData::Log(LogRecord {
            text: format!("Native Agent event omitted ({key}).\n"),
            stream: "stderr".into(),
            diagnostic: true,
        }))]
    }

    fn malformed(&mut self, key: &str) -> Vec<StreamRecord> {
        self.invalid = true;
        self.result = None;
        self.diagnostic(key)
    }

    pub fn accept(&mut self, event: Value) -> Vec<StreamRecord> {
        if self.completed {
            return if native_session_id(&event).is_some() {
                vec![]
            } else {
                self.diagnostic("Kimi event after completion")
            };
        }
        let at = Utc::now();
        match event["role"].as_str().unwrap_or("") {
            "assistant" => {
                // Each line is a completed message, not a text delta. Tools and
                // their arguments are flushed before their corresponding result.
                if event["tool_calls"].as_array().is_some_and(|calls| {
                    !calls.is_empty()
                        && calls.iter().all(|call| {
                            call["id"]
                                .as_str()
                                .is_some_and(|id| self.tools.contains_key(id))
                        })
                }) {
                    return self.diagnostic("duplicate Kimi tool call");
                }
                let mut records = vec![];
                if let Some(text) = event["content"].as_str().filter(|s| !s.is_empty()) {
                    self.next_message += 1;
                    records.push(StreamRecord::new(RecordData::Trace(TraceNode {
                        id: format!("kimi-message:{}", self.next_message),
                        parent_id: None,
                        kind: TraceKind::Message,
                        title: "Message".into(),
                        input: Value::Null,
                        output: json!(text),
                        status: TraceState::Succeeded,
                        occurred_at: at,
                        started_at: None,
                        finished_at: Some(at),
                    })));
                    self.result = Some(json!(text));
                } else if event
                    .get("content")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                {
                    records.extend(self.malformed("invalid Kimi message content"));
                }
                if let Some(calls) = event.get("tool_calls").filter(|v| !v.is_null()) {
                    let Some(calls) = calls.as_array() else {
                        records.extend(self.malformed("invalid Kimi tool calls"));
                        return records;
                    };
                    for call in calls {
                        let Some(id) = call["id"]
                            .as_str()
                            .filter(|id| !id.is_empty() && id.len() <= 256)
                        else {
                            records.extend(self.malformed("missing Kimi tool ID"));
                            continue;
                        };
                        let Some(name) =
                            call["function"]["name"].as_str().filter(|s| !s.is_empty())
                        else {
                            records.extend(self.malformed("missing Kimi tool name"));
                            continue;
                        };
                        let Some(arguments) = call["function"]["arguments"].as_str() else {
                            records.extend(self.malformed("missing Kimi tool arguments"));
                            continue;
                        };
                        if self.tools.contains_key(id) {
                            records.extend(self.diagnostic("duplicate Kimi tool call"));
                            continue;
                        }
                        if self.tools.len() >= 10_000 {
                            records.extend(self.malformed("Kimi tool count exceeded limit"));
                            break;
                        }
                        let input =
                            serde_json::from_str(arguments).unwrap_or_else(|_| json!(arguments));
                        let input_bytes =
                            serde_json::to_vec(&input).map_or(arguments.len(), |v| v.len());
                        if self.retained_input_bytes + input_bytes > 8 * 1024 * 1024 {
                            records
                                .extend(self.malformed("Kimi pending tool inputs exceeded limit"));
                            break;
                        }
                        self.retained_input_bytes += input_bytes;
                        self.result = None;
                        let node = TraceNode {
                            id: id.into(),
                            parent_id: None,
                            kind: if matches!(name, "Bash" | "Shell" | "bash" | "shell") {
                                TraceKind::Command
                            } else {
                                TraceKind::Tool
                            },
                            title: name.chars().take(512).collect(),
                            input,
                            output: Value::Null,
                            status: TraceState::Running,
                            occurred_at: at,
                            // Print mode buffers calls until flush, so their
                            // actual start times are not in the protocol.
                            started_at: None,
                            finished_at: None,
                        };
                        self.tools.insert(id.into(), node.clone());
                        records.push(StreamRecord::new(RecordData::Trace(node)));
                    }
                }
                records
            }
            "tool" => {
                let Some(id) = event["tool_call_id"].as_str() else {
                    return self.malformed("missing Kimi tool result ID");
                };
                let Some(content) = event["content"].as_str() else {
                    return self.malformed("invalid Kimi tool result");
                };
                let Some(mut node) = self.tools.get(id).cloned() else {
                    return self.malformed("unmatched Kimi tool result");
                };
                if node.finished_at.is_some() {
                    return self.diagnostic("duplicate Kimi tool result");
                }
                self.result = None;
                // The print renderer does not expose native isError; do not
                // guess a command exit code or tool failure from output text.
                node.output = json!(content);
                node.status = TraceState::Succeeded;
                node.occurred_at = at;
                node.finished_at = Some(at);
                self.retained_input_bytes = self
                    .retained_input_bytes
                    .saturating_sub(serde_json::to_vec(&node.input).map_or(0, |v| v.len()));
                let mut terminal = node.clone();
                // Keep only identity/status to latch duplicate terminal events;
                // already emitted tool payloads need not accumulate in memory.
                terminal.input = Value::Null;
                terminal.output = Value::Null;
                self.tools.insert(id.into(), terminal);
                vec![StreamRecord::new(RecordData::Trace(node))]
            }
            "meta" => match event["type"].as_str().unwrap_or("") {
                "session.resume_hint" => {
                    if native_session_id(&event).is_none() {
                        return self.malformed("missing Kimi session ID");
                    }
                    self.completed = true;
                    vec![]
                }
                "system.version" => vec![],
                "turn.step.retrying" => {
                    // This is a recoverable provider retry, never a final error.
                    self.result = None;
                    vec![StreamRecord::new(RecordData::Log(LogRecord {
                        text: "Agent is retrying the request.\n".into(),
                        stream: "stderr".into(),
                        diagnostic: false,
                    }))]
                }
                other => self.diagnostic(other),
            },
            other => self.diagnostic(other),
        }
    }

    pub fn failed(&self) -> bool {
        self.invalid
            || !self.completed
            || self.tools.values().any(|n| n.status == TraceState::Running)
    }

    pub fn finish(&self) -> Vec<StreamRecord> {
        if self.failed() {
            return vec![];
        }
        self.result
            .clone()
            .map(|v| vec![StreamRecord::new(RecordData::Result(v))])
            .unwrap_or_default()
    }
}

pub fn native_session_id(event: &Value) -> Option<&str> {
    if event["role"] != "meta" || event["type"] != "session.resume_hint" {
        return None;
    }
    event["session_id"]
        .as_str()
        .filter(|id| crate::sessions::validate_id(id).is_ok())
}

/// Metadata read from Kimi's native index and persisted session state only.
#[derive(Debug, Clone)]
pub struct KimiSession {
    pub id: String,
    pub title: Option<String>,
    pub cwd: PathBuf,
    pub updated_at: Option<DateTime<Utc>>,
    session_dir: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    session_id: String,
    work_dir: PathBuf,
    session_dir: PathBuf,
}

fn kimi_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("KIMI_CODE_HOME").filter(|s| !s.is_empty()) {
        return Ok(path.into());
    }
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(|path| PathBuf::from(path).join(".kimi-code"))
        .context("user home directory unavailable")
}

/// Never boots Kimi's SDK, reads config/credentials, or opens conversation logs.
/// Current official create/fork paths append the native session_index.jsonl.
/// The newest mapping must still agree with persisted state.json and cwd.
pub async fn session_summaries(cwd: &Path) -> Result<Vec<KimiSession>> {
    tokio::time::timeout(Duration::from_secs(20), sessions_in(&kimi_home()?, cwd))
        .await
        .context("Kimi session listing timed out")?
}

async fn sessions_in(home: &Path, cwd: &Path) -> Result<Vec<KimiSession>> {
    let cwd = std::fs::canonicalize(cwd).context("working directory unavailable")?;
    let file = match tokio::fs::File::open(home.join("session_index.jsonl")).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error).context("read Kimi session index"),
    };
    ensure!(
        file.metadata().await?.is_file(),
        "Kimi session index is not a file"
    );
    let mut reader = BufReader::new(file);
    let mut entries = BTreeMap::new();
    let mut total = 0;
    loop {
        let mut bytes = Vec::new();
        let count = (&mut reader)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .await?;
        if count == 0 {
            break;
        }
        total += count;
        ensure!(
            count <= 1024 * 1024 && total <= 64 * 1024 * 1024,
            "Kimi session index exceeded limit"
        );
        // An incomplete trailing append or unrelated corrupt entry is never a
        // source of identity. Later complete mappings supersede earlier ones.
        let Ok(item) = serde_json::from_slice::<IndexEntry>(&bytes) else {
            continue;
        };
        if crate::sessions::validate_id(&item.session_id).is_err() {
            continue;
        }
        entries.insert(item.session_id.clone(), item);
        ensure!(
            entries.len() <= 100_000,
            "Kimi session index exceeded limit"
        );
    }
    let mut sessions = Vec::new();
    for item in entries.into_values() {
        if std::fs::canonicalize(&item.work_dir).ok().as_ref() != Some(&cwd) {
            continue;
        }
        let mut session = KimiSession {
            id: item.session_id,
            title: None,
            cwd: cwd.clone(),
            updated_at: None,
            session_dir: item.session_dir,
        };
        if let Some(metadata) = persisted_metadata(&session).await? {
            session.title = metadata["title"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.chars().take(512).collect());
            session.updated_at = metadata["updatedAt"]
                .as_i64()
                .and_then(DateTime::from_timestamp_millis);
            sessions.push(session);
        }
    }
    Ok(sessions)
}

async fn persisted_metadata(item: &KimiSession) -> Result<Option<Value>> {
    // Kimi's official session lifecycle stores metadata in sessionDir/state.json.
    // Paths come from its native index, never from an external session ID.
    let file = match tokio::fs::File::open(item.session_dir.join("state.json")).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("read Kimi session metadata"),
    };
    const LIMIT: usize = 1024 * 1024;
    let metadata = file.metadata().await?;
    if !metadata.is_file() || metadata.len() > LIMIT as u64 {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() > LIMIT {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    let actual_cwd = value["cwd"].as_str().or(value["workDir"].as_str());
    if value["id"].as_str() != Some(item.id.as_str())
        || actual_cwd
            .and_then(|cwd| std::fs::canonicalize(cwd).ok())
            .as_ref()
            != Some(&item.cwd)
    {
        return Ok(None);
    }
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trailer() -> Value {
        json!({"role":"meta","type":"session.resume_hint","session_id":"session_fixture1","command":"kimi -r session_fixture1"})
    }

    #[test]
    fn messages_tools_and_native_session_trailer() {
        let mut adapter = KimiNormalizer::new();
        let mut records = Vec::new();
        for line in include_str!("../tests/fixtures/kimi-code/stream.jsonl").lines() {
            records.extend(adapter.accept(serde_json::from_str(line).unwrap()));
        }
        assert!(!adapter.failed());
        let traces: Vec<_> = records
            .iter()
            .filter_map(|r| match &r.record {
                RecordData::Trace(n) => Some(n),
                _ => None,
            })
            .collect();
        assert_eq!(traces.len(), 4);
        assert_eq!(traces[1].kind, TraceKind::Command);
        assert_eq!(traces[1].input, json!({"command":"pwd"}));
        assert_eq!(traces[2].id, "call_fixture1");
        assert_eq!(traces[2].status, TraceState::Succeeded);
        assert_eq!(traces[1].started_at, traces[2].started_at);
        assert!(matches!(&adapter.finish()[0].record, RecordData::Result(v) if v == "Done."));
        assert_eq!(native_session_id(&trailer()), Some("session_fixture1"));
        assert!(
            records
                .iter()
                .all(|r| !matches!(r.record, RecordData::Session(_)))
        );
    }

    #[test]
    fn partial_message_without_success_trailer_is_not_a_result() {
        let mut adapter = KimiNormalizer::new();
        adapter.accept(json!({"role":"assistant","content":"Partial"}));
        assert!(adapter.failed());
        assert!(adapter.finish().is_empty());
    }

    #[test]
    fn tool_call_commentary_is_not_the_final_result() {
        let mut adapter = KimiNormalizer::new();
        adapter.accept(json!({"role":"assistant","content":"Checking","tool_calls":[{"id":"call1","function":{"name":"Read","arguments":"{}"}}]}));
        adapter.accept(trailer());
        assert!(adapter.failed());
        assert!(adapter.finish().is_empty());
        adapter.accept(json!({"role":"tool","tool_call_id":"call1","content":"ok"}));
        assert!(adapter.finish().is_empty());
    }

    #[test]
    fn retries_are_recoverable_and_unknown_events_are_deduplicated() {
        let mut adapter = KimiNormalizer::new();
        adapter.accept(json!({"role":"meta","type":"turn.step.retrying","error_message":"private provider detail"}));
        let records = adapter.accept(json!({"role":"meta","type":"future.event"}));
        assert_eq!(records.len(), 1);
        assert!(
            adapter
                .accept(json!({"role":"meta","type":"future.event"}))
                .is_empty()
        );
        adapter.accept(json!({"role":"assistant","content":"Recovered"}));
        adapter.accept(trailer());
        assert!(!adapter.failed());
        assert_eq!(adapter.finish().len(), 1);
    }

    #[tokio::test]
    async fn native_index_is_project_scoped_and_uses_newest_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let first = home.path().join("first");
        let latest = home.path().join("latest");
        tokio::fs::create_dir(&first).await.unwrap();
        tokio::fs::create_dir(&latest).await.unwrap();
        tokio::fs::write(
            first.join("state.json"),
            serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path(),"title":"Stale"}))
                .unwrap(),
        )
        .await
        .unwrap();
        tokio::fs::write(latest.join("state.json"), serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path(),"updatedAt":2000,"lastPrompt":"private prompt"})).unwrap()).await.unwrap();
        let rows = [
            json!({"sessionId":"session_one","workDir":dir.path(),"sessionDir":first}),
            json!({"sessionId":"session_one","workDir":dir.path(),"sessionDir":latest}),
            json!({"sessionId":"session_moved","workDir":dir.path(),"sessionDir":first}),
            json!({"sessionId":"session_moved","workDir":other.path(),"sessionDir":first}),
            json!({"sessionId":"session_missing","workDir":dir.path(),"sessionDir":home.path().join("missing")}),
            json!({"sessionId":"session_other","workDir":other.path(),"sessionDir":home.path().join("unopened")}),
            json!({"sessionId":"../invalid","workDir":dir.path(),"sessionDir":latest}),
        ];
        let index = rows
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n{partial";
        tokio::fs::write(home.path().join("session_index.jsonl"), index)
            .await
            .unwrap();
        let sessions = sessions_in(home.path(), dir.path()).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "session_one");
        assert_eq!(sessions[0].title, None);
        assert_eq!(sessions[0].updated_at.unwrap().timestamp_millis(), 2000);
        assert!(!home.path().join("config.toml").exists());
        assert!(!home.path().join("unopened").exists());
    }

    #[tokio::test]
    async fn missing_storage_is_empty_and_never_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("missing-home");
        assert!(sessions_in(&home, dir.path()).await.unwrap().is_empty());
        assert!(!home.exists());
    }

    #[tokio::test]
    async fn persisted_metadata_requires_exact_id_and_directory() {
        let dir = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let item = KimiSession {
            id: "session_one".into(),
            title: None,
            cwd: std::fs::canonicalize(dir.path()).unwrap(),
            updated_at: None,
            session_dir: storage.path().into(),
        };
        assert!(persisted_metadata(&item).await.unwrap().is_none());
        let path = storage.path().join("state.json");
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_one","cwd":dir.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_some());
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_other","cwd":dir.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_none());
        tokio::fs::write(
            &path,
            serde_json::to_vec(&json!({"id":"session_one","cwd":storage.path()})).unwrap(),
        )
        .await
        .unwrap();
        assert!(persisted_metadata(&item).await.unwrap().is_none());
    }

    #[test]
    fn completed_stream_cannot_regress_after_duplicate_tool_events() {
        let mut adapter = KimiNormalizer::new();
        let call = json!({"role":"assistant","content":"Checking","tool_calls":[{"id":"call1","function":{"name":"Read","arguments":"{}"}}]});
        let result = json!({"role":"tool","tool_call_id":"call1","content":"ok"});
        adapter.accept(call.clone());
        adapter.accept(result.clone());
        adapter.accept(call.clone());
        assert_eq!(adapter.tools["call1"].status, TraceState::Succeeded);
        adapter.accept(json!({"role":"assistant","content":"Final"}));
        adapter.accept(call.clone());
        adapter.accept(result.clone());
        adapter.accept(trailer());
        adapter.accept(call);
        adapter.accept(result);
        assert!(!adapter.failed());
        assert!(matches!(&adapter.finish()[0].record, RecordData::Result(v) if v == "Final"));
    }

    #[test]
    fn invalid_messages_fail_without_dumping_native_payload() {
        let mut adapter = KimiNormalizer::new();
        let records = adapter.accept(
            json!({"role":"tool","tool_call_id":"unknown","content":"private native output"}),
        );
        adapter.accept(trailer());
        assert!(adapter.failed());
        assert!(adapter.finish().is_empty());
        assert!(
            !serde_json::to_string(&records)
                .unwrap()
                .contains("private native output")
        );
        assert!(
            native_session_id(
                &json!({"role":"meta","type":"session.resume_hint","session_id":"../path"})
            )
            .is_none()
        );
    }
}
