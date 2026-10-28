use super::*;
use crate::{
    config::{self, TaskConfig},
    dag::{Dag, Node, Prompt, PromptPart},
};
use minijinja::{
    AutoEscape, Environment, UndefinedBehavior,
    value::{Serde, ValueKind},
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

fn environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_auto_escape_callback(|_| AutoEscape::None);
    env.set_syntax(
        minijinja::syntax::SyntaxConfig::builder()
            .variable_delimiters("[[", "]]")
            .keep_trailing_newline(true)
            .build()
            .expect("static template syntax"),
    );
    // Configuration generation only: one field has the same 1 MiB prompt
    // boundary as ordinary Tasks. This never governs a running Agent.
    env.set_fuel(Some(50_000_000));
    env.set_formatter(|out, state, value| {
        if matches!(value.kind(), ValueKind::Seq | ValueKind::Map) {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "Use a loop or tojson to output a list or object",
            ));
        }
        minijinja::escape_formatter(out, state, value)
    });
    env
}
fn engine_issue(code: &str, path: &str, source: &str, error: minijinja::Error) -> Issue {
    let mut issue = Issue::new(code, path, error.to_string());
    issue.line = error.line();
    issue.column = error
        .range()
        .and_then(|r| source.get(..r.start))
        .map(|s| s.rsplit('\n').next().unwrap_or("").chars().count() + 1);
    issue
}
struct BoundedText {
    bytes: Vec<u8>,
}
impl io::Write for BoundedText {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > 1024 * 1024 {
            return Err(io::Error::other(
                "Generated text exceeds the existing 1 MiB prompt boundary",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn render(
    env: &Environment<'_>,
    text: &Text,
    context: &Value,
    path: &str,
    issues: &mut Vec<Issue>,
) -> String {
    let Text::Template(source) = text else {
        return match text {
            Text::Literal(s) => s.clone(),
            _ => unreachable!(),
        };
    };
    let mut output = BoundedText { bytes: vec![] };
    let result = env
        .template_from_str(&source.template)
        .and_then(|t| t.render_captured_to(Serde(context), &mut output));
    if let Err(error) = result {
        issues.push(engine_issue("render", path, &source.template, error));
        return String::new();
    }
    String::from_utf8(output.bytes).expect("MiniJinja emits UTF-8")
}
fn evaluate(
    env: &Environment<'_>,
    expression: &Expr,
    context: &Value,
    path: &str,
    issues: &mut Vec<Issue>,
) -> Option<minijinja::Value> {
    match env
        .compile_expression(&expression.expr)
        .and_then(|e| e.eval(Serde(context)))
    {
        Ok(value) => Some(value),
        Err(error) => {
            issues.push(engine_issue("expression", path, &expression.expr, error));
            None
        }
    }
}
fn enabled(
    env: &Environment<'_>,
    condition: Option<&Expr>,
    context: &Value,
    path: &str,
    issues: &mut Vec<Issue>,
) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    let Some(value) = evaluate(env, condition, context, path, issues) else {
        return false;
    };
    if value.kind() != ValueKind::Bool {
        issues.push(Issue::new(
            "conditionType",
            path,
            "Condition must return a boolean",
        ));
        return false;
    }
    value.is_true()
}
fn context(inputs: &Map<String, Value>, alias: Option<(&str, &Value)>) -> Value {
    let mut value = Map::from_iter([("inputs".into(), Value::Object(inputs.clone()))]);
    if let Some((name, item)) = alias {
        value.insert(name.into(), item.clone());
    }
    Value::Object(value)
}
impl Declaration {
    fn body(&self) -> Option<NodeBlueprint> {
        if self.group.is_some() {
            return self.node.clone();
        }
        Some(NodeBlueprint {
            name: self.name.clone()?,
            binding: self.binding.clone()?,
            cwd: self.cwd.clone()?,
            prompt: self.prompt.clone()?,
            pre: self.pre.clone(),
            timeout: self.timeout.clone().unwrap_or_else(|| "1h".into()),
            max_retries: self.max_retries,
        })
    }
}
fn syntax_text(env: &Environment<'_>, text: &Text, path: &str, issues: &mut Vec<Issue>) {
    if let Text::Template(text) = text
        && let Err(error) = env.template_from_str(&text.template)
    {
        issues.push(engine_issue("syntax", path, &text.template, error));
    }
}
fn syntax_expr(env: &Environment<'_>, expr: Option<&Expr>, path: &str, issues: &mut Vec<Issue>) {
    if let Some(expr) = expr
        && let Err(error) = env.compile_expression(&expr.expr)
    {
        issues.push(engine_issue("syntax", path, &expr.expr, error));
    }
}
fn selector_schema(
    env: &Environment<'_>,
    selector: &Selector,
    path: &str,
    names: &BTreeMap<String, bool>,
    issues: &mut Vec<Issue>,
) {
    let valid = matches!(
        (&selector.node, &selector.group, &selector.key, selector.all),
        (Some(_), None, None, None)
            | (None, Some(_), Some(_), None)
            | (None, Some(_), None, Some(true))
    );
    if !valid {
        issues.push(Issue::new(
            "selector",
            path,
            "Expected a fixed node, keyed group member, or all group members",
        ));
    }
    if let Some(node) = &selector.node
        && names.get(node) != Some(&false)
    {
        issues.push(
            Issue::new("unknownNode", path, "Unknown fixed node").param("name", node.clone()),
        );
    }
    if let Some(group) = &selector.group
        && names.get(group) != Some(&true)
    {
        issues.push(
            Issue::new("unknownGroup", path, "Unknown expansion group")
                .param("name", group.clone()),
        );
    }
    syntax_expr(env, selector.when.as_ref(), &format!("{path}.when"), issues);
    if let Some(Key::Expression(expr)) = &selector.key {
        syntax_expr(env, Some(expr), &format!("{path}.key"), issues);
    }
}
fn body_schema(
    env: &Environment<'_>,
    body: &NodeBlueprint,
    path: &str,
    definition: &Definition,
    names: &BTreeMap<String, bool>,
    issues: &mut Vec<Issue>,
) {
    if !definition.bindings.contains_key(&body.binding) {
        issues.push(
            Issue::new(
                "unknownBinding",
                format!("{path}.binding"),
                "Unknown environment binding",
            )
            .param("name", body.binding.clone()),
        );
    }
    syntax_text(env, &body.name, &format!("{path}.name"), issues);
    syntax_text(env, &body.cwd, &format!("{path}.cwd"), issues);
    if let Err(e) = config::duration(&body.timeout) {
        issues.push(Issue::new(
            "configuration",
            format!("{path}.timeout"),
            e.to_string(),
        ));
    }
    if body.max_retries > 10 {
        issues.push(Issue::new(
            "configuration",
            format!("{path}.max_retries"),
            "Maximum retries must be between 0 and 10",
        ));
    }
    for (i, selector) in body.pre.iter().enumerate() {
        selector_schema(env, selector, &format!("{path}.pre[{i}]"), names, issues);
    }
    for (i, part) in body.prompt.parts.iter().enumerate() {
        let path = format!("{path}.prompt.parts[{i}]");
        match part {
            Part::Template(text) => syntax_text(env, &Text::Template(text.clone()), &path, issues),
            Part::Result(result) => selector_schema(env, &result.result, &path, names, issues),
            Part::Text(_) => {}
        }
    }
}
pub fn validate_definition(definition: &Definition) -> Vec<Issue> {
    let mut issues = vec![];
    if definition.version != 1 {
        issues.push(Issue::new(
            "version",
            "version",
            "Unsupported template format version",
        ));
    }
    if let Err(e) = config::validate_id(&definition.id) {
        issues.push(Issue::new("identifier", "id", e.to_string()));
    }
    if definition.name.trim().is_empty() {
        issues.push(Issue::new("required", "name", "Template name is required"));
    }
    definition.inputs.validate_schema("inputs", 0, &mut issues);
    let env = environment();
    syntax_text(&env, &definition.task.name, "task.name", &mut issues);
    for name in definition.bindings.keys() {
        if let Err(e) = config::validate_id(name) {
            issues.push(Issue::new(
                "identifier",
                format!("bindings.{name}"),
                e.to_string(),
            ));
        }
    }
    if let Some(dag) = &definition.task.dag {
        if definition.task.binding.is_some()
            || definition.task.cwd.is_some()
            || definition.task.prompt.is_some()
        {
            issues.push(Issue::new(
                "blueprint",
                "task",
                "A DAG has execution bindings and instructions on its nodes",
            ));
        }
        let mut names = BTreeMap::new();
        for (i, node) in dag.nodes.iter().enumerate() {
            let path = format!("task.dag.nodes[{i}]");
            let name = match (&node.id, &node.group) {
                (Some(id), None) => Some((id, false)),
                (None, Some(group)) => Some((group, true)),
                _ => None,
            };
            if let Some((name, group)) = name {
                if let Err(e) = config::validate_id(name) {
                    issues.push(Issue::new("identifier", &path, e.to_string()));
                }
                if names.insert(name.clone(), group).is_some() {
                    issues.push(
                        Issue::new("duplicateDeclaration", &path, "Duplicate declaration name")
                            .param("name", name.clone()),
                    );
                }
            } else {
                issues.push(Issue::new(
                    "blueprint",
                    &path,
                    "Declare either a fixed node id or an expansion group",
                ));
            }
        }
        for (i, node) in dag.nodes.iter().enumerate() {
            let path = format!("task.dag.nodes[{i}]");
            syntax_expr(
                &env,
                node.when.as_ref(),
                &format!("{path}.when"),
                &mut issues,
            );
            if node.group.is_some() {
                if node.for_each.is_none()
                    || node.alias.is_none()
                    || node.key.as_deref().is_none_or(str::is_empty)
                    || node.node.is_none()
                    || node.name.is_some()
                    || node.binding.is_some()
                    || node.cwd.is_some()
                    || node.prompt.is_some()
                    || !node.pre.is_empty()
                    || node.timeout.is_some()
                    || node.max_retries != 0
                {
                    issues.push(Issue::new(
                        "blueprint",
                        &path,
                        "An expansion requires for_each, as, key and a node body",
                    ));
                }
                if let Some(alias) = &node.alias
                    && (alias.is_empty()
                        || !alias.bytes().enumerate().all(|(i, c)| {
                            c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                        })
                        || ["inputs", "nodes", "groups", "loop"].contains(&alias.as_str()))
                {
                    issues.push(Issue::new(
                        "alias",
                        format!("{path}.as"),
                        "Invalid or reserved item alias",
                    ));
                }
                syntax_expr(
                    &env,
                    node.for_each.as_ref(),
                    &format!("{path}.for_each"),
                    &mut issues,
                );
            } else if node.for_each.is_some()
                || node.alias.is_some()
                || node.key.is_some()
                || node.node.is_some()
            {
                issues.push(Issue::new(
                    "blueprint",
                    &path,
                    "Fixed nodes cannot contain generator fields",
                ));
            }
            if let Some(body) = node.body() {
                body_schema(&env, &body, &path, definition, &names, &mut issues);
            } else {
                issues.push(Issue::new(
                    "blueprint",
                    &path,
                    "Node name, binding, directory and instruction parts are required",
                ));
            }
        }
        if let Some(results) = &dag.results {
            for (i, selector) in results.iter().enumerate() {
                selector_schema(
                    &env,
                    selector,
                    &format!("task.dag.results[{i}]"),
                    &names,
                    &mut issues,
                );
            }
        }
    } else {
        let task = &definition.task;
        if task
            .binding
            .as_ref()
            .is_none_or(|s| !definition.bindings.contains_key(s))
        {
            issues.push(Issue::new(
                "unknownBinding",
                "task.binding",
                "Select a declared environment binding",
            ));
        }
        if let Some(cwd) = &task.cwd {
            syntax_text(&env, cwd, "task.cwd", &mut issues);
        } else {
            issues.push(Issue::new(
                "required",
                "task.cwd",
                "Working directory is required",
            ));
        }
        if let Some(prompt) = &task.prompt {
            syntax_text(&env, prompt, "task.prompt", &mut issues);
        } else {
            issues.push(Issue::new(
                "required",
                "task.prompt",
                "Instructions are required",
            ));
        }
    }
    issues
}

struct Expanded {
    id: String,
    body: NodeBlueprint,
    context: Value,
    source: Source,
}
fn input_list_path(expression: &str) -> Option<String> {
    if expression.starts_with("inputs.")
        && expression
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.')
    {
        Some(expression.into())
    } else {
        None
    }
}
fn select(
    env: &Environment<'_>,
    selector: &Selector,
    context: &Value,
    path: &str,
    groups: &BTreeMap<String, Vec<(String, String)>>,
    actual: &BTreeSet<String>,
    issues: &mut Vec<Issue>,
) -> Vec<String> {
    if !enabled(
        env,
        selector.when.as_ref(),
        context,
        &format!("{path}.when"),
        issues,
    ) {
        return vec![];
    }
    if let Some(node) = &selector.node {
        if actual.contains(node) {
            return vec![node.clone()];
        }
        issues.push(
            Issue::new("missingTarget", path, "Referenced node was not generated")
                .param("name", node.clone()),
        );
        return vec![];
    }
    let Some(group) = &selector.group else {
        return vec![];
    };
    let Some(members) = groups.get(group) else {
        return vec![];
    };
    if selector.all == Some(true) {
        return members.iter().map(|(_, id)| id.clone()).collect();
    }
    let key = match &selector.key {
        Some(Key::Literal(key)) => Some(key.clone()),
        Some(Key::Expression(expr)) => {
            evaluate(env, expr, context, path, issues).and_then(|value| {
                if let Some(value) = value.as_str() {
                    Some(value.to_owned())
                } else {
                    issues.push(Issue::new(
                        "keyType",
                        path,
                        "Member key expression must return a string",
                    ));
                    None
                }
            })
        }
        None => None,
    };
    let Some(key) = key else { return vec![] };
    if let Some((_, id)) = members.iter().find(|(k, _)| *k == key) {
        return vec![id.clone()];
    }
    issues.push(
        Issue::new(
            "missingTarget",
            path,
            "Referenced group member was not generated",
        )
        .param("name", format!("{group}/{key}")),
    );
    vec![]
}
fn binding(
    definition: &Definition,
    request: &InstanceRequest,
    name: &str,
    issues: &mut Vec<Issue>,
) -> Binding {
    let defaults = definition
        .bindings
        .get(name)
        .map(|s| s.defaults.clone())
        .unwrap_or_default();
    let supplied = request.bindings.get(name).cloned().unwrap_or_default();
    let value = Binding {
        runner: supplied.runner.or(defaults.runner),
        agent: supplied.agent.or(defaults.agent),
        parameters: supplied.parameters.or(defaults.parameters),
    };
    for (field, present) in [
        ("runner", value.runner.as_deref()),
        ("agent", value.agent.as_deref()),
    ] {
        if present.is_none_or(|s| s.trim().is_empty()) {
            issues.push(
                Issue::new(
                    "bindingRequired",
                    format!("bindings.{name}.{field}"),
                    "Execution environment is incomplete",
                )
                .param("name", name.to_owned()),
            );
        }
    }
    value
}
fn parts(
    env: &Environment<'_>,
    source: &Expanded,
    groups: &BTreeMap<String, Vec<(String, String)>>,
    actual: &BTreeSet<String>,
    issues: &mut Vec<Issue>,
) -> Prompt {
    let mut parts = vec![];
    for (i, part) in source.body.prompt.parts.iter().enumerate() {
        let path = format!("{}.prompt.parts[{i}]", source.source.declaration);
        match part {
            Part::Text(text) => parts.push(PromptPart::text(text.text.clone())),
            Part::Template(text) => parts.push(PromptPart::text(render(
                env,
                &Text::Template(text.clone()),
                &source.context,
                &path,
                issues,
            ))),
            Part::Result(result) => {
                let ids = select(
                    env,
                    &result.result,
                    &source.context,
                    &path,
                    groups,
                    actual,
                    issues,
                );
                for (i, id) in ids.into_iter().enumerate() {
                    if i > 0 && !result.separator.is_empty() {
                        parts.push(PromptPart::text(result.separator.clone()));
                    }
                    parts.push(PromptPart::result(id));
                }
            }
        }
    }
    Prompt::structured(parts)
}
pub fn compile(definition: &Definition, request: &InstanceRequest) -> Preview {
    let mut preview = Preview {
        template_revision: request.expected_revision,
        request_id: request.request_id.clone(),
        issues: validate_definition(definition),
        ..Preview::default()
    };
    if !preview.issues.is_empty() {
        return preview;
    }
    let (inputs, issues) = resolve_inputs(&definition.inputs, &request.values, &request.unset);
    preview.resolved_inputs = inputs;
    preview.issues.extend(issues);
    for name in request.bindings.keys() {
        if !definition.bindings.contains_key(name) {
            preview.issues.push(
                Issue::new(
                    "unknownBinding",
                    format!("bindings.{name}"),
                    "Unknown environment binding",
                )
                .param("name", name.clone()),
            );
        }
    }
    if !preview.issues.is_empty() {
        return preview;
    }
    let env = environment();
    let base = context(&preview.resolved_inputs, None);
    let mut config = TaskConfig {
        version: 1,
        id: request
            .task_id
            .clone()
            .unwrap_or_else(|| "template-preview".into()),
        name: request.name.clone().unwrap_or_else(|| {
            render(
                &env,
                &definition.task.name,
                &base,
                "task.name",
                &mut preview.issues,
            )
        }),
        runner: String::new(),
        agent: String::new(),
        cwd: String::new(),
        prompt: String::new(),
        parameters: BTreeMap::new(),
        session: None,
        schedule: definition.task.schedule.clone(),
        enabled: definition.task.enabled,
        execution: definition.task.execution.clone(),
        dag: None,
    };
    if let Some(schedule) = &request.schedule {
        match serde_json::from_value(schedule.clone()) {
            Ok(schedule) => config.schedule = schedule,
            Err(e) => preview
                .issues
                .push(Issue::new("configuration", "schedule", e.to_string())),
        }
    }
    let mut used = BTreeSet::new();
    if let Some(dag) = &definition.task.dag {
        let mut expanded = vec![];
        let mut groups = BTreeMap::<String, Vec<(String, String)>>::new();
        for (index, declaration) in dag.nodes.iter().enumerate() {
            let path = format!("task.dag.nodes[{index}]");
            let Some(body) = declaration.body() else {
                continue;
            };
            if let Some(group) = &declaration.group {
                groups.insert(group.clone(), vec![]);
                let expression = declaration.for_each.as_ref().expect("validated generator");
                let Some(value) = evaluate(
                    &env,
                    expression,
                    &base,
                    &format!("{path}.for_each"),
                    &mut preview.issues,
                ) else {
                    continue;
                };
                let value = serde_json::to_value(&value);
                let Ok(Value::Array(items)) = value else {
                    preview.issues.push(Issue::new(
                        "listType",
                        format!("{path}.for_each"),
                        "Node expansion must return a list of objects",
                    ));
                    continue;
                };
                let key_field = declaration.key.as_deref().expect("validated key");
                let alias = declaration.alias.as_deref().expect("validated alias");
                let mut keys = BTreeSet::new();
                for (i, item) in items.iter().enumerate() {
                    let input_path = input_list_path(&expression.expr).map(|p| format!("{p}[{i}]"));
                    let key_path = input_path
                        .as_ref()
                        .map(|p| format!("{p}.{key_field}"))
                        .unwrap_or_else(|| format!("{path}.for_each[{i}].{key_field}"));
                    let Some(key) = item
                        .as_object()
                        .and_then(|o| o.get(key_field))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    else {
                        preview.issues.push(Issue::new(
                            "itemKey",
                            key_path,
                            "Each expansion item requires a stable string key",
                        ));
                        continue;
                    };
                    if !keys.insert(key.to_owned()) {
                        preview.issues.push(
                            Issue::new("duplicateKey", &key_path, "Duplicate expansion key")
                                .param("name", key.to_owned()),
                        );
                        continue;
                    }
                    let ctx = context(&preview.resolved_inputs, Some((alias, item)));
                    if !enabled(
                        &env,
                        declaration.when.as_ref(),
                        &ctx,
                        &format!("{path}.when"),
                        &mut preview.issues,
                    ) {
                        continue;
                    }
                    let id = format!("{group}__{key}");
                    if let Err(e) = config::validate_id(&id) {
                        preview.issues.push(
                            Issue::new("identifier", key_path, e.to_string())
                                .param("name", id.clone()),
                        );
                    }
                    groups
                        .get_mut(group)
                        .expect("registered group")
                        .push((key.into(), id.clone()));
                    expanded.push(Expanded {
                        id,
                        body: body.clone(),
                        context: ctx,
                        source: Source {
                            declaration: format!("{path}.node"),
                            binding: body.binding.clone(),
                            group: Some(group.clone()),
                            key: Some(key.into()),
                            input_path,
                        },
                    });
                }
            } else if enabled(
                &env,
                declaration.when.as_ref(),
                &base,
                &format!("{path}.when"),
                &mut preview.issues,
            ) {
                expanded.push(Expanded {
                    id: declaration.id.clone().expect("validated fixed id"),
                    body: body.clone(),
                    context: base.clone(),
                    source: Source {
                        declaration: path,
                        binding: body.binding.clone(),
                        group: None,
                        key: None,
                        input_path: None,
                    },
                });
            }
        }
        let actual = expanded
            .iter()
            .map(|n| n.id.clone())
            .collect::<BTreeSet<_>>();
        let mut bindings = BTreeMap::new();
        for n in &expanded {
            used.insert(n.body.binding.clone());
        }
        for name in &used {
            bindings.insert(
                name.clone(),
                binding(definition, request, name, &mut preview.issues),
            );
        }
        let mut nodes = vec![];
        for n in &expanded {
            let bound = &bindings[&n.body.binding];
            let mut pre = vec![];
            for (i, selector) in n.body.pre.iter().enumerate() {
                pre.extend(select(
                    &env,
                    selector,
                    &n.context,
                    &format!("{}.pre[{i}]", n.source.declaration),
                    &groups,
                    &actual,
                    &mut preview.issues,
                ));
            }
            nodes.push(Node {
                id: n.id.clone(),
                name: render(
                    &env,
                    &n.body.name,
                    &n.context,
                    &format!("{}.name", n.source.declaration),
                    &mut preview.issues,
                ),
                pre,
                runner: bound.runner.clone().unwrap_or_default(),
                agent: bound.agent.clone().unwrap_or_default(),
                cwd: render(
                    &env,
                    &n.body.cwd,
                    &n.context,
                    &format!("{}.cwd", n.source.declaration),
                    &mut preview.issues,
                ),
                prompt: parts(&env, n, &groups, &actual, &mut preview.issues),
                parameters: bound.parameters.clone().unwrap_or_default(),
                session: None,
                timeout: n.body.timeout.clone(),
                max_retries: n.body.max_retries,
            });
            if preview
                .source_map
                .insert(n.id.clone(), n.source.clone())
                .is_some()
            {
                preview.issues.push(
                    Issue::new(
                        "graph",
                        &n.source.declaration,
                        "Generated node ID collision",
                    )
                    .param("name", n.id.clone()),
                );
            }
        }
        let results = dag.results.as_ref().map(|results| {
            results
                .iter()
                .enumerate()
                .flat_map(|(i, selector)| {
                    select(
                        &env,
                        selector,
                        &base,
                        &format!("task.dag.results[{i}]"),
                        &groups,
                        &actual,
                        &mut preview.issues,
                    )
                })
                .collect()
        });
        let graph = Dag { nodes, results };
        for issue in graph.issues() {
            if issue.path.ends_with(".runner") || issue.path.ends_with(".agent") {
                continue;
            }
            preview.issues.push(Issue {
                code: "graph".into(),
                path: issue.path,
                message: issue.message,
                params: Map::new(),
                node_id: issue.node_id,
                line: None,
                column: None,
            });
        }
        preview.graph = Some(graph.clone());
        config.dag = Some(graph);
    } else {
        let slot = definition
            .task
            .binding
            .as_deref()
            .expect("validated ordinary binding");
        used.insert(slot.into());
        let bound = binding(definition, request, slot, &mut preview.issues);
        config.runner = bound.runner.unwrap_or_default();
        config.agent = bound.agent.unwrap_or_default();
        config.parameters = bound.parameters.unwrap_or_default();
        config.cwd = render(
            &env,
            definition.task.cwd.as_ref().expect("validated cwd"),
            &base,
            "task.cwd",
            &mut preview.issues,
        );
        config.prompt = render(
            &env,
            definition.task.prompt.as_ref().expect("validated prompt"),
            &base,
            "task.prompt",
            &mut preview.issues,
        );
        if config.execution.timeout.is_none() {
            config.execution.timeout = Some("1h".into());
        }
    }
    preview.used_bindings = used.into_iter().collect();
    if preview.issues.is_empty()
        && let Err(e) = config.validate()
    {
        preview
            .issues
            .push(Issue::new("configuration", "task", e.to_string()));
    }
    if preview.issues.is_empty() {
        preview.config = Some(config);
    }
    preview
}

pub fn from_task(task: &TaskConfig) -> anyhow::Result<Definition> {
    let mut bindings = BTreeMap::<String, BindingSlot>::new();
    let mut slots = BTreeMap::<String, String>::new();
    let mut slot = |runner: &str, agent: &str, parameters: &BTreeMap<String, String>| {
        let key =
            serde_json::to_string(&(runner, agent, parameters)).expect("binding serialization");
        if let Some(name) = slots.get(&key) {
            return name.clone();
        }
        let name = format!("environment-{}", slots.len() + 1);
        bindings.insert(
            name.clone(),
            BindingSlot {
                title: agent.into(),
                defaults: Binding {
                    runner: Some(runner.into()),
                    agent: Some(agent.into()),
                    parameters: Some(parameters.clone()),
                },
            },
        );
        slots.insert(key, name.clone());
        name
    };
    let mut blueprint = TaskBlueprint {
        name: Text::Literal(task.name.clone()),
        schedule: task.schedule.clone(),
        enabled: task.enabled,
        execution: task.execution.clone(),
        binding: None,
        cwd: None,
        prompt: None,
        dag: None,
    };
    if let Some(dag) = &task.dag {
        let mut declarations = vec![];
        for n in &dag.nodes {
            let parts = n
                .prompt
                .to_parts()?
                .into_iter()
                .map(|part| match part {
                    PromptPart::Text(text) => Part::Text(text),
                    PromptPart::Result(reference) => Part::Result(ResultSelection {
                        result: Selector {
                            node: Some(reference.result),
                            group: None,
                            key: None,
                            all: None,
                            when: None,
                        },
                        separator: String::new(),
                    }),
                })
                .collect();
            declarations.push(Declaration {
                id: Some(n.id.clone()),
                group: None,
                for_each: None,
                alias: None,
                key: None,
                when: None,
                node: None,
                name: Some(Text::Literal(n.name.clone())),
                binding: Some(slot(&n.runner, &n.agent, &n.parameters)),
                cwd: Some(Text::Literal(n.cwd.clone())),
                prompt: Some(BlueprintPrompt { parts }),
                pre: n
                    .pre
                    .iter()
                    .map(|id| Selector {
                        node: Some(id.clone()),
                        group: None,
                        key: None,
                        all: None,
                        when: None,
                    })
                    .collect(),
                timeout: Some(n.timeout.clone()),
                max_retries: n.max_retries,
            });
        }
        blueprint.dag = Some(DagBlueprint {
            nodes: declarations,
            results: dag.results.as_ref().map(|ids| {
                ids.iter()
                    .map(|id| Selector {
                        node: Some(id.clone()),
                        group: None,
                        key: None,
                        all: None,
                        when: None,
                    })
                    .collect()
            }),
        });
    } else {
        blueprint.binding = Some(slot(&task.runner, &task.agent, &task.parameters));
        blueprint.cwd = Some(Text::Literal(task.cwd.clone()));
        blueprint.prompt = Some(Text::Literal(task.prompt.clone()));
    }
    Ok(Definition {
        version: 1,
        id: uuid::Uuid::new_v4().to_string(),
        name: task.name.clone(),
        description: String::new(),
        inputs: InputSchema::object(),
        bindings,
        task: blueprint,
    })
}
