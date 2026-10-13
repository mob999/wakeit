use crate::config::{AgentMode, ParameterSpec, TaskConfig};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    pub mode: AgentMode,
    pub digest: String,
    pub available: bool,
    pub version: Option<String>,
    pub cancellable: bool,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterSpec>,
    #[serde(default)]
    pub models_supported: bool,
    #[serde(default)]
    pub trace_supported: bool,
    #[serde(default)]
    pub session_supported: bool,
    #[serde(default)]
    pub session_list_supported: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub version: u32,
    pub run_id: Uuid,
    pub runner_id: Uuid,
    pub task: TaskConfig,
    pub deadline: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub version: u32,
    pub runner_id: Uuid,
    pub run_id: Uuid,
    pub claim_id: Uuid,
    pub seq: i64,
    pub kind: String,
    pub data: Value,
    pub timestamp: DateTime<Utc>,
}
pub fn terminal(state: &str) -> bool {
    matches!(
        state,
        "succeeded" | "failed" | "cancelled" | "timed_out" | "skipped" | "interrupted"
    )
}
pub fn active(state: &str) -> bool {
    matches!(state, "starting" | "running" | "unknown")
}
pub fn work_subject(id: Uuid) -> String {
    format!("wakeit.v1.runner.{id}.work")
}
pub fn control_subject(id: Uuid) -> String {
    format!("wakeit.v1.runner.{id}.control")
}
pub fn event_subject(id: Uuid) -> String {
    format!("wakeit.v1.runner.{id}.events")
}
pub fn consumer(id: Uuid, kind: &str) -> String {
    format!("r_{}_{}", id.simple(), kind)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub parameters: BTreeMap<String, String>,
    #[serde(default)]
    pub parameter_options: BTreeMap<String, Vec<String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalog {
    pub version: u32,
    pub models: Vec<ModelEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub version: u32,
    pub id: Uuid,
    pub runtime_id: Uuid,
    pub action: String,
    pub payload: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryReply {
    pub version: u32,
    pub id: Uuid,
    pub runtime_id: Uuid,
    pub data: Option<Value>,
    pub error: Option<QueryError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryError {
    pub code: String,
    pub message: String,
}
pub fn query_subject(id: Uuid) -> String {
    format!("wakeit.v1.runner.{id}.query")
}
pub fn reply_prefix(id: Uuid) -> String {
    format!("wakeit.v1.runner.{id}.reply")
}
