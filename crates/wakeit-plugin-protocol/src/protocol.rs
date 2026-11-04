//! Agent discovery and model catalogs. Task assignment and NATS transport stay in Core.
use crate::agent::{AgentMode, ParameterSpec};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
