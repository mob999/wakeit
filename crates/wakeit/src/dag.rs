//! Complete, inline AND graphs. This module contains no scheduling side effects.
use crate::config::{Execution, SessionBinding, TaskConfig, duration, validate_id};
pub use crate::dag_prompt::{Prompt, PromptPart, PromptSource};
use anyhow::{Result, bail, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dag {
    pub nodes: Vec<Node>,
    /// Absent means the leaves of the current definition, rather than a cached list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub pre: Vec<String>,
    pub runner: String,
    pub agent: String,
    pub cwd: String,
    pub prompt: Prompt,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionBinding>,
    #[serde(default = "node_timeout")]
    pub timeout: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    #[schemars(range(max = 10))]
    pub max_retries: u32,
}
fn is_zero(value: &u32) -> bool {
    *value == 0
}
fn node_timeout() -> String {
    "1h".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ValidationIssue {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cycle: Vec<String>,
}

impl Node {
    pub fn execution(&self, graph: &TaskConfig, prompt: String) -> TaskConfig {
        TaskConfig {
            version: 1,
            id: graph.id.clone(),
            name: format!("{} / {}", graph.name, self.name),
            runner: self.runner.clone(),
            agent: self.agent.clone(),
            cwd: self.cwd.clone(),
            prompt,
            parameters: self.parameters.clone(),
            session: self.session.clone(),
            schedule: None,
            enabled: true,
            execution: Execution {
                timeout: Some(self.timeout.clone()),
                ..graph.execution.clone()
            },
            dag: None,
        }
    }
}

impl Dag {
    pub fn outputs(&self) -> Vec<String> {
        self.results.clone().unwrap_or_else(|| {
            let parents: BTreeSet<_> = self.nodes.iter().flat_map(|n| n.pre.iter()).collect();
            self.nodes
                .iter()
                .filter(|n| !parents.contains(&n.id))
                .map(|n| n.id.clone())
                .collect()
        })
    }
    pub fn ancestors(&self, id: &str) -> BTreeSet<String> {
        let mut result = BTreeSet::new();
        let mut pending = vec![id.to_owned()];
        while let Some(id) = pending.pop() {
            if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                for parent in &n.pre {
                    if result.insert(parent.clone()) {
                        pending.push(parent.clone());
                    }
                }
            }
        }
        result
    }
    pub fn issues(&self) -> Vec<ValidationIssue> {
        let mut issues = vec![];
        let mut push = |node: Option<&Node>, field: &str, message: String, cycle: Vec<String>| {
            issues.push(ValidationIssue {
                path: node.map_or_else(
                    || format!("dag.{field}"),
                    |n| format!("dag.nodes.{}.{field}", n.id),
                ),
                node_id: node.map(|n| n.id.clone()),
                message,
                cycle,
            });
        };
        if self.nodes.is_empty() {
            push(
                None,
                "nodes",
                "At least one node is required".into(),
                vec![],
            );
        }
        let mut ids = BTreeSet::new();
        for node in &self.nodes {
            if let Err(e) = validate_id(&node.id) {
                push(Some(node), "id", e.to_string(), vec![]);
            }
            if !ids.insert(node.id.as_str()) {
                push(Some(node), "id", "Duplicate node ID".into(), vec![]);
            }
        }
        for node in &self.nodes {
            let mut parents = BTreeSet::new();
            for parent in &node.pre {
                let error = if parent == &node.id {
                    Some("A node cannot depend on itself")
                } else if !ids.contains(parent.as_str()) {
                    Some("Predecessor does not exist")
                } else if !parents.insert(parent) {
                    Some("Duplicate predecessor")
                } else {
                    None
                };
                if let Some(e) = error {
                    push(Some(node), "pre", format!("{e}: {parent}"), vec![]);
                }
            }
            for (field, missing) in [
                ("name", node.name.trim().is_empty()),
                ("runner", node.runner.trim().is_empty()),
                ("agent", node.agent.is_empty()),
                ("cwd", node.cwd.trim().is_empty()),
                ("prompt", node.prompt.is_blank()),
            ] {
                if missing {
                    push(Some(node), field, format!("{field} is required"), vec![]);
                }
            }
            if let Err(e) = validate_id(&node.agent) {
                push(Some(node), "agent", e.to_string(), vec![]);
            }
            if let Err(e) = duration(&node.timeout) {
                push(Some(node), "timeout", e.to_string(), vec![]);
            }
            if node.max_retries > 10 {
                push(
                    Some(node),
                    "max_retries",
                    "Maximum retries must be between 0 and 10".into(),
                    vec![],
                );
            }
            if node.prompt.source_len() > 1024 * 1024 {
                push(Some(node), "prompt", "Prompt exceeds 1 MiB".into(), vec![]);
            }
            if let Some(session) = &node.session
                && let Err(e) = crate::sessions::validate_id(&session.id)
            {
                push(Some(node), "session", e.to_string(), vec![]);
            }
        }
        // Iterative path traversal also handles deep graphs without recursive stack growth.
        let mut done = BTreeSet::new();
        for root in &self.nodes {
            if done.contains(&root.id) {
                continue;
            }
            let mut stack = vec![(root.id.clone(), false)];
            let mut path = vec![];
            while let Some((id, leaving)) = stack.pop() {
                if leaving {
                    done.insert(id);
                    path.pop();
                    continue;
                }
                if let Some(pos) = path.iter().position(|v| v == &id) {
                    let mut cycle = path[pos..].to_vec();
                    cycle.push(id.clone());
                    push(
                        self.nodes.iter().find(|n| n.id == id),
                        "pre",
                        format!("Dependency cycle: {}", cycle.join(" → ")),
                        cycle,
                    );
                    break;
                }
                if done.contains(&id) {
                    continue;
                }
                path.push(id.clone());
                stack.push((id.clone(), true));
                if let Some(n) = self.nodes.iter().find(|n| n.id == id) {
                    for p in n.pre.iter().rev() {
                        if ids.contains(p.as_str()) {
                            stack.push((p.clone(), false));
                        }
                    }
                }
            }
        }
        for node in &self.nodes {
            let ancestors = self.ancestors(&node.id);
            match references(&node.prompt) {
                Ok(refs) => {
                    for id in refs {
                        if !ancestors.contains(&id) {
                            push(
                                Some(node),
                                "prompt",
                                format!("Result source is not an ancestor: {id}"),
                                vec![],
                            );
                        }
                    }
                }
                Err(e) => push(Some(node), "prompt", e.to_string(), vec![]),
            }
        }
        if let Some(outputs) = &self.results {
            if outputs.is_empty() {
                push(
                    None,
                    "results",
                    "Select at least one result node".into(),
                    vec![],
                );
            }
            let mut seen = BTreeSet::new();
            for id in outputs {
                if !ids.contains(id.as_str()) || !seen.insert(id) {
                    push(
                        None,
                        "results",
                        format!("Invalid or duplicate result node: {id}"),
                        vec![],
                    );
                }
            }
        }
        issues
    }
    pub fn validate(&self) -> Result<()> {
        if let Some(issue) = self.issues().first() {
            bail!("{}: {}", issue.path, issue.message);
        }
        Ok(())
    }
}

pub fn references(prompt: &(impl PromptSource + ?Sized)) -> Result<Vec<String>> {
    Ok(prompt
        .fragments()?
        .into_iter()
        .filter_map(|(_, id)| id)
        .collect())
}
pub fn render(
    prompt: &(impl PromptSource + ?Sized),
    results: &BTreeMap<String, Value>,
) -> Result<String> {
    let mut output = String::new();
    for (text, id) in prompt.fragments()? {
        output.push_str(text);
        if let Some(id) = id {
            let value = results
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("Upstream result unavailable: {id}"))?;
            ensure!(
                !value.is_null()
                    && !["truncated", "expired", "incomplete"]
                        .iter()
                        .any(|k| value.get(*k) == Some(&Value::Bool(true))),
                "Upstream result missing, truncated or unavailable: {id}"
            );
            if let Some(text) = value.as_str() {
                ensure!(!text.is_empty(), "Upstream result is empty: {id}");
                output.push_str(text);
            } else {
                output.push_str(&serde_json::to_string_pretty(value)?);
            }
        }
        ensure!(output.len() <= 1024 * 1024, "Expanded prompt exceeds 1 MiB");
    }
    Ok(output)
}

/// All nodes have settled before this is called. A warning never hides an unknown execution.
pub fn outcome(dag: &Dag, states: &BTreeMap<String, String>) -> &'static str {
    if states.values().any(|s| s == "unknown") {
        return "unknown";
    }
    if states.len() != dag.nodes.len() || states.values().any(|s| !crate::protocol::terminal(s)) {
        return "running";
    }
    if dag
        .outputs()
        .iter()
        .any(|id| states.get(id).is_none_or(|s| s != "succeeded"))
    {
        "failed"
    } else if states.values().all(|s| s == "succeeded") {
        "succeeded"
    } else {
        "warning"
    }
}
