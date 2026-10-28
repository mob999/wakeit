use super::InputSchema;
use crate::config::{Execution, Schedule, TaskConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub inputs: InputSchema,
    #[serde(default)]
    pub bindings: BTreeMap<String, BindingSlot>,
    pub task: TaskBlueprint,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct BindingSlot {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub defaults: Binding,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub runner: Option<String>,
    pub agent: Option<String>,
    pub parameters: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Text {
    Literal(String),
    Template(TemplateText),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateText {
    pub template: String,
}
impl From<String> for Text {
    fn from(text: String) -> Self {
        Self::Literal(text)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Expr {
    pub expr: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskBlueprint {
    pub name: Text,
    #[serde(default)]
    pub schedule: Option<Schedule>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub execution: Execution,
    pub binding: Option<String>,
    pub cwd: Option<Text>,
    pub prompt: Option<Text>,
    pub dag: Option<DagBlueprint>,
}
fn yes() -> bool {
    true
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DagBlueprint {
    pub nodes: Vec<Declaration>,
    pub results: Option<Vec<Selector>>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub id: Option<String>,
    pub group: Option<String>,
    pub for_each: Option<Expr>,
    #[serde(rename = "as")]
    pub alias: Option<String>,
    pub key: Option<String>,
    pub when: Option<Expr>,
    pub node: Option<NodeBlueprint>,
    pub name: Option<Text>,
    pub binding: Option<String>,
    pub cwd: Option<Text>,
    pub prompt: Option<BlueprintPrompt>,
    #[serde(default)]
    pub pre: Vec<Selector>,
    pub timeout: Option<String>,
    #[serde(default)]
    pub max_retries: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NodeBlueprint {
    pub name: Text,
    pub binding: String,
    pub cwd: Text,
    pub prompt: BlueprintPrompt,
    #[serde(default)]
    pub pre: Vec<Selector>,
    #[serde(default = "timeout")]
    pub timeout: String,
    #[serde(default)]
    pub max_retries: u32,
}
fn timeout() -> String {
    "1h".into()
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlueprintPrompt {
    pub parts: Vec<Part>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Part {
    Text(crate::dag_prompt::TextPart),
    Template(TemplateText),
    Result(ResultSelection),
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResultSelection {
    pub result: Selector,
    #[serde(default)]
    pub separator: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub node: Option<String>,
    pub group: Option<String>,
    pub key: Option<Key>,
    pub all: Option<bool>,
    pub when: Option<Expr>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Key {
    Literal(String),
    Expression(Expr),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct InstanceRequest {
    pub expected_revision: i64,
    #[serde(default)]
    pub values: Map<String, Value>,
    #[serde(default)]
    pub unset: Vec<String>,
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    pub task_id: Option<String>,
    pub name: Option<String>,
    #[serde(
        default,
        deserialize_with = "super::schema::present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub run_now: bool,
    pub request_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct Issue {
    pub code: String,
    pub path: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub params: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}
impl Issue {
    pub fn new(code: &str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            path: path.into(),
            message: message.into(),
            params: Map::new(),
            node_id: None,
            line: None,
            column: None,
        }
    }
    pub fn param(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Source {
    pub declaration: String,
    pub binding: String,
    pub group: Option<String>,
    pub key: Option<String>,
    pub input_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct Preview {
    pub template_revision: i64,
    pub request_id: Option<String>,
    pub resolved_inputs: Map<String, Value>,
    pub config: Option<TaskConfig>,
    pub graph: Option<crate::dag::Dag>,
    pub source_map: BTreeMap<String, Source>,
    pub used_bindings: Vec<String>,
    pub issues: Vec<Issue>,
}
