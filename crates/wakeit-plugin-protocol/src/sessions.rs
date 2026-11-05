use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SessionInfo {
    pub version: u32,
    pub id: String,
    pub cwd: String,
    pub persisted: bool,
    #[serde(default)]
    pub title: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunSession {
    pub id: String,
    pub runner_id: Uuid,
    pub agent: String,
    pub cwd: String,
    pub ready: bool,
    pub reason: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}
pub fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && !id.starts_with('-')
            && id != "."
            && id != ".."
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "session ID must be an explicit identifier, not a path or option"
    );
    Ok(())
}
