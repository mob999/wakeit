//! Agent metadata shared by declarations and discovery; command/YAML configuration stays in Core.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Sync,
    Async,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParameterSpec {
    #[serde(rename = "type", default = "string_type")]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    pub default: Option<String>,
    #[serde(rename = "enum", default)]
    pub choices: Vec<String>,
}
fn string_type() -> String {
    "string".into()
}
