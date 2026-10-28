//! Shared presentation state; never part of an execution configuration or assignment.
use crate::config::TaskConfig;
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorLayout {
    pub version: u32,
    pub positions: BTreeMap<String, Point>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl EditorLayout {
    pub fn validate(&self, task: &TaskConfig) -> Result<()> {
        ensure!(self.version == 1, "unsupported editor layout version");
        let graph = task
            .dag
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("editor layout requires a DAG task"))?;
        let ids: BTreeSet<_> = graph.nodes.iter().map(|node| node.id.as_str()).collect();
        for (id, point) in &self.positions {
            ensure!(
                ids.contains(id.as_str()),
                "editor layout references an unknown node: {id}"
            );
            ensure!(
                point.x.is_finite() && point.y.is_finite(),
                "editor layout coordinates must be finite"
            );
        }
        Ok(())
    }
    pub fn retain_nodes(mut self, task: &TaskConfig) -> Option<Self> {
        let graph = task.dag.as_ref()?;
        self.positions
            .retain(|id, _| graph.nodes.iter().any(|node| &node.id == id));
        Some(self)
    }
}
