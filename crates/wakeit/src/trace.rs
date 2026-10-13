//! Public Agent output. Run lifecycle is deliberately outside this protocol.
use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Reserve transport-envelope space below the default 1 MiB NATS payload limit.
pub const RECORD_LIMIT: usize = 1000 * 1024;
#[derive(Debug)]
pub struct RecordTooLarge;
impl std::fmt::Display for RecordTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stream record exceeds 1000 KiB")
    }
}
impl std::error::Error for RecordTooLarge {}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceKind {
    Message,
    Tool,
    Command,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraceNode {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub kind: TraceKind,
    pub status: TraceState,
    pub title: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LogRecord {
    pub text: String,
    #[serde(default = "stdout")]
    pub stream: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diagnostic: bool,
}
fn stdout() -> String {
    "stdout".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RecordData {
    Trace(TraceNode),
    Log(LogRecord),
    Result(Value),
    Session(crate::sessions::SessionInfo),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamRecord {
    pub version: u32,
    #[serde(flatten)]
    pub record: RecordData,
}
impl StreamRecord {
    pub fn parse(line: &[u8]) -> Result<Self> {
        let record: Self = serde_json::from_slice(line)?;
        ensure!(record.version == 1, "unsupported stream version");
        if let RecordData::Trace(node) = &record.record {
            ensure!(
                !node.id.trim().is_empty() && node.id.len() <= 256,
                "invalid trace id"
            );
            ensure!(node.title.len() <= 4096, "trace title too long");
            ensure!(
                node.parent_id
                    .as_ref()
                    .is_none_or(|id| !id.is_empty() && id.len() <= 256 && id != &node.id),
                "invalid parent id"
            );
        }
        if let RecordData::Session(info) = &record.record {
            crate::sessions::validate_id(&info.id)?;
            ensure!(
                info.version == 1 && !info.cwd.is_empty() && info.cwd.len() <= 32768,
                "invalid session record"
            );
        }
        Ok(record)
    }
    pub fn new(record: RecordData) -> Self {
        Self { version: 1, record }
    }
}
/// Bounded framing, independent of UTF-8 chunk boundaries. Oversized records are discarded
/// through the next newline so a suffix cannot be mistaken for a fresh record.
#[derive(Default)]
pub struct Lines {
    bytes: Vec<u8>,
    dropping: bool,
}
impl Lines {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Result<StreamRecord>> {
        let mut records = vec![];
        for &b in bytes {
            if b == b'\n' {
                if !self.dropping && !self.bytes.iter().all(u8::is_ascii_whitespace) {
                    records.push(StreamRecord::parse(&self.bytes));
                }
                self.bytes.clear();
                self.dropping = false;
            } else if !self.dropping {
                if self.bytes.len() == RECORD_LIMIT {
                    self.bytes.clear();
                    self.dropping = true;
                    records.push(Err(RecordTooLarge.into()));
                } else {
                    self.bytes.push(b);
                }
            }
        }
        records
    }
    pub fn finish(&mut self) -> Vec<Result<StreamRecord>> {
        self.push(b"\n")
    }
}
