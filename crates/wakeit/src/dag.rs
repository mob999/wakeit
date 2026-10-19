//! Complete, inline AND graphs. This module contains no scheduling side effects.
use crate::config::{Execution, SessionBinding, TaskConfig, duration, validate_id};
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
    pub prompt: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionBinding>,
    #[serde(default = "node_timeout")]
    pub timeout: String,
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
                timeout: self.timeout.clone(),
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
                ("prompt", node.prompt.trim().is_empty()),
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
            if node.prompt.len() > 1024 * 1024 {
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

/// Only the reserved nodes namespace is interpreted. Literal template examples remain literal.
fn segments(prompt: &str) -> Result<Vec<(&str, Option<String>)>> {
    let mut rest = prompt;
    let mut result = vec![];
    while let Some(start) = rest.find("{{") {
        let (before, tail) = rest.split_at(start);
        let Some(end) = tail.find("}}") else {
            ensure!(
                !tail[2..].trim_start().starts_with("nodes."),
                "Unclosed result reference"
            );
            break;
        };
        let key = tail[2..end].trim();
        if let Some(key) = key.strip_prefix("nodes.") {
            let id = key
                .strip_suffix(".result")
                .ok_or_else(|| anyhow::anyhow!("Invalid result reference: {key}"))?;
            validate_id(id)?;
            result.push((before, Some(id.to_owned())));
        } else {
            result.push((&rest[..start + end + 2], None));
        }
        rest = &tail[end + 2..];
    }
    result.push((rest, None));
    Ok(result)
}
pub fn references(prompt: &str) -> Result<Vec<String>> {
    Ok(segments(prompt)?
        .into_iter()
        .filter_map(|(_, id)| id)
        .collect())
}
pub fn render(prompt: &str, results: &BTreeMap<String, Value>) -> Result<String> {
    let mut output = String::new();
    for (text, id) in segments(prompt)? {
        output.push_str(text);
        if let Some(id) = id {
            let value = results
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("Upstream result unavailable: {id}"))?;
            ensure!(
                !value.is_null() && value.get("truncated") != Some(&Value::Bool(true)),
                "Upstream result missing or truncated: {id}"
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
