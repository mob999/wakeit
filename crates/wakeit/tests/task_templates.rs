use serde_json::{Value, json};
use std::collections::BTreeMap;
use wakeit::{
    dag,
    task_templates::{self as templates, Definition, InstanceRequest},
};

fn definition() -> Definition {
    serde_json::from_value(json!({
      "version":1,"id":"repository-review","name":"Repository review",
      "inputs":{"type":"object","additionalProperties":false,"required":["repositories"],"properties":{
        "repositories":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["key","url"],"properties":{"key":{"type":"string"},"url":{"type":"string","format":"repository-url"},"enabled":{"type":"boolean"}}}},
        "summarize":{"type":"boolean","default":true},"note":{"type":"string","default":"default"},"optional":{"type":"string","default":"present"}
      }},
      "bindings":{"worker":{"defaults":{"runner":"device","agent":"pi","parameters":{}}}},
      "task":{"name":{"template":"Review [[ inputs.repositories | length ]]"},"dag":{"nodes":[
        {"group":"review","for_each":{"expr":"inputs.repositories"},"as":"repo","key":"key","when":{"expr":"repo.enabled | default(true)"},"node":{
          "binding":"worker","name":{"template":"Review [[ repo.key ]]"},"cwd":"/workspace","prompt":{"parts":[{"template":"检查 [[ repo.url ]]\n[[ inputs.note ]]"}]}}},
        {"id":"summary","when":{"expr":"inputs.summarize"},"binding":"worker","name":"Summary","cwd":"/workspace","pre":[{"group":"review","all":true}],"prompt":{"parts":[{"text":"Reports:\n"},{"result":{"group":"review","all":true},"separator":"\n---\n"}]}}
      ],"results":[{"node":"summary","when":{"expr":"inputs.summarize"}},{"group":"review","all":true,"when":{"expr":"not inputs.summarize"}}]}}
    })).unwrap()
}
fn request(values: Value) -> InstanceRequest {
    serde_json::from_value(json!({"expected_revision":3,"values":values,"request_id":"draft-7"}))
        .unwrap()
}
fn repositories() -> Value {
    json!([{"key":"a","url":"https://example.com/a"},{"key":"b","url":"git@example.com:b.git"}])
}

#[test]
fn expands_keyed_groups_then_resolves_fan_in_and_runtime_results() {
    let preview = templates::compile(
        &definition(),
        &request(json!({"repositories":repositories()})),
    );
    assert_eq!(preview.issues, []);
    assert_eq!(preview.request_id.as_deref(), Some("draft-7"));
    let config = preview.config.unwrap();
    config.validate().unwrap();
    let graph = config.dag.as_ref().unwrap();
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["review__a", "review__b", "summary"]
    );
    assert_eq!(graph.nodes[2].pre, ["review__a", "review__b"]);
    assert_eq!(graph.outputs(), ["summary"]);
    assert_eq!(
        dag::references(&graph.nodes[2].prompt).unwrap(),
        ["review__a", "review__b"]
    );
    assert_eq!(
        dag::render(
            &graph.nodes[2].prompt,
            &BTreeMap::from([
                ("review__a".into(), json!("Report A")),
                ("review__b".into(), json!("Report B"))
            ])
        )
        .unwrap(),
        "Reports:\nReport A\n---\nReport B"
    );
    assert_eq!(
        preview.source_map["review__b"].input_path.as_deref(),
        Some("inputs.repositories[1]")
    );
    let execution = graph.nodes[2].execution(&config, "Rendered once".into());
    assert!(execution.dag.is_none());
    assert_eq!(execution.version, 1);
    assert_eq!(execution.prompt, "Rendered once");
}

#[test]
fn reordering_filtering_and_conditional_outputs_keep_actual_identities() {
    let values = json!({"repositories":[{"key":"b","url":"https://example.com/b"},{"key":"disabled","url":"https://example.com/disabled","enabled":false},{"key":"a","url":"https://example.com/a"}],"summarize":false});
    let preview = templates::compile(&definition(), &request(values));
    assert_eq!(preview.issues, []);
    let graph = preview.config.unwrap().dag.unwrap();
    assert_eq!(
        graph
            .nodes
            .iter()
            .map(|n| n.id.as_str())
            .collect::<Vec<_>>(),
        ["review__b", "review__a"]
    );
    assert_eq!(graph.outputs(), ["review__b", "review__a"]);
    assert!(!preview.source_map.contains_key("review__disabled"));
}

#[test]
fn inserted_template_syntax_stays_literal_and_legacy_conversion_keeps_references() {
    let literal = "中文 {{ nodes.summary.result }} [[ inputs.secret ]] {% if missing %}";
    let mut preview = templates::compile(
        &definition(),
        &request(json!({"repositories":repositories(),"note":literal})),
    );
    assert_eq!(preview.issues, []);
    let mut config = preview.config.take().unwrap();
    let graph = config.dag.as_mut().unwrap();
    assert_eq!(
        dag::references(&graph.nodes[0].prompt).unwrap(),
        Vec::<String>::new()
    );
    assert!(
        dag::render(&graph.nodes[0].prompt, &BTreeMap::new())
            .unwrap()
            .ends_with(literal)
    );
    graph.nodes[2].prompt =
        " before {{ nodes.review__a.result }} / {{ nodes.review__a.result }} [[ example ]] ".into();
    let saved = templates::from_task(&config).unwrap();
    assert!(templates::validate_definition(&saved).is_empty());
    let again = templates::compile(&saved, &request(json!({})));
    assert_eq!(again.issues, []);
    let node = &again.config.as_ref().unwrap().dag.as_ref().unwrap().nodes[2];
    assert_eq!(
        dag::references(&node.prompt).unwrap(),
        ["review__a", "review__a"]
    );
    assert_eq!(
        dag::render(
            &node.prompt,
            &BTreeMap::from([("review__a".into(), json!("value"))])
        )
        .unwrap(),
        " before value / value [[ example ]] "
    );
    let encoded = serde_json::to_value(again.config.unwrap()).unwrap();
    assert!(
        encoded["dag"]["nodes"][0]["prompt"]["parts"][0]
            .get("text")
            .is_some()
    );
    assert!(
        serde_json::from_value::<dag::Prompt>(json!({"parts":[{"template":"forbidden"}]})).is_err()
    );
    assert!(
        serde_json::from_value::<dag::Prompt>(
            json!({"parts":[{"text":"literal","result":"summary"}]})
        )
        .is_err()
    );
}

#[test]
fn defaults_explicit_unset_and_whole_values_have_distinct_meanings() {
    let schema:templates::InputSchema=serde_json::from_value(json!({"type":"object","properties":{
      "flag":{"type":"boolean","default":true},"count":{"type":"integer","default":8},"note":{"type":"string","default":"text"},
      "items":{"type":"array","items":{"type":"string"},"default":["original"]},
      "object":{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"string"}},"default":{"a":"old","b":"old"}},
      "nullable":{"type":["string","null"],"default":null},"optional":{"type":"string","default":"restored"}
    }})).unwrap();
    let values = json!({"flag":false,"count":0,"note":"","items":[],"object":{"a":"new"}});
    let (resolved, issues) =
        templates::resolve_inputs(&schema, values.as_object().unwrap(), &["optional".into()]);
    assert_eq!(issues, []);
    assert_eq!(resolved["flag"], false);
    assert_eq!(resolved["count"], 0);
    assert_eq!(resolved["note"], "");
    assert_eq!(resolved["items"], json!([]));
    assert_eq!(resolved["object"], json!({"a":"new"}));
    assert_eq!(resolved["nullable"], Value::Null);
    assert!(!resolved.contains_key("optional"));
    let (_, issues) =
        templates::resolve_inputs(&schema, values.as_object().unwrap(), &["flag".into()]);
    assert!(issues.iter().any(|i| i.code == "overrideConflict"));
}

#[test]
fn empty_groups_are_allowed_but_empty_final_graphs_and_output_sets_are_not() {
    let preview = templates::compile(&definition(), &request(json!({"repositories":[]})));
    assert_eq!(preview.issues, []);
    let graph = preview.config.unwrap().dag.unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.nodes[0].pre.is_empty());
    assert_eq!(
        dag::render(&graph.nodes[0].prompt, &BTreeMap::new()).unwrap(),
        "Reports:\n"
    );
    let preview = templates::compile(
        &definition(),
        &request(json!({"repositories":[],"summarize":false})),
    );
    assert!(preview.config.is_none());
    assert!(preview.issues.iter().any(|i| i.path == "dag.nodes"));
    assert!(preview.issues.iter().any(|i| i.path == "dag.results"));
}

#[test]
fn invalid_generation_inputs_fail_before_a_task_exists() {
    let cases = [
        (
            "duplicateKey",
            json!([{"key":"a","url":"https://example.com/a"},{"key":"a","url":"https://example.com/b","enabled":false}]),
        ),
        ("itemKey", json!([{"key":"","url":"https://example.com/a"}])),
        (
            "identifier",
            json!([{"key":"bad.key","url":"https://example.com/a"}]),
        ),
        (
            "repositoryUrl",
            json!([{"key":"a","url":"not a repository"}]),
        ),
        (
            "unknownInput",
            json!([{"key":"a","url":"https://example.com/a","extra":true}]),
        ),
    ];
    for (code, repositories) in cases {
        let preview = templates::compile(
            &definition(),
            &request(json!({"repositories":repositories})),
        );
        assert!(preview.config.is_none(), "{code}");
        assert!(
            preview.issues.iter().any(|i| i.code == code),
            "{code}: {:?}",
            preview.issues
        );
    }
}

#[test]
fn expressions_are_typed_and_guarded_selectors_do_not_evaluate_disabled_keys() {
    let mut def = definition();
    let graph = def.task.dag.as_mut().unwrap();
    graph.nodes[1].pre.push(serde_json::from_value(json!({"group":"review","key":{"expr":"inputs.absent.key"},"when":{"expr":"inputs.absent is defined"}})).unwrap());
    let preview = templates::compile(&def, &request(json!({"repositories":repositories()})));
    assert_eq!(preview.issues, []);
    def.task.dag.as_mut().unwrap().nodes[0].when = Some(templates::Expr {
        expr: "'false'".into(),
    });
    let preview = templates::compile(&def, &request(json!({"repositories":repositories()})));
    assert!(preview.issues.iter().any(|i| i.code == "conditionType"));
    assert!(preview.config.is_none());
    def.task.dag.as_mut().unwrap().nodes[0].for_each = Some(templates::Expr {
        expr: "'not a list'".into(),
    });
    let preview = templates::compile(&def, &request(json!({"repositories":repositories()})));
    assert!(preview.issues.iter().any(|i| i.code == "listType"));
}

#[test]
fn text_loops_conditions_macros_and_json_use_the_actual_engine_without_html_escaping() {
    let mut def = definition();
    def.task.dag.as_mut().unwrap().nodes[0].node.as_mut().unwrap().prompt.parts=serde_json::from_value(json!([
      {"template":"{% macro label(x) %}<[[ x ]]>{% endmacro %}{% for r in inputs.repositories %}{% if r.key == repo.key %}[[ label(r.key) ]][% literal ]{% endif %}{% endfor %}\n[[ inputs.repositories | tojson ]]"}
    ])).unwrap();
    let preview = templates::compile(&def, &request(json!({"repositories":repositories()})));
    assert_eq!(preview.issues, []);
    let graph = preview.config.unwrap().dag.unwrap();
    let text = dag::render(&graph.nodes[0].prompt, &BTreeMap::new()).unwrap();
    assert!(text.starts_with("<a>[% literal ]\n"));
    let parsed: Value = serde_json::from_str(text.split_once('\n').unwrap().1).unwrap();
    assert_eq!(parsed, repositories());
}

#[test]
fn previews_show_graph_and_used_bindings_without_claiming_incomplete_environment_is_valid() {
    let mut def = definition();
    def.bindings.get_mut("worker").unwrap().defaults = templates::Binding::default();
    let preview = templates::compile(&def, &request(json!({"repositories":repositories()})));
    assert!(preview.config.is_none());
    assert_eq!(preview.graph.unwrap().nodes.len(), 3);
    assert_eq!(preview.used_bindings, ["worker"]);
    assert!(
        preview
            .issues
            .iter()
            .any(|i| i.path == "bindings.worker.runner")
    );
    let mut request = request(json!({"repositories":repositories()}));
    request.bindings.insert(
        "worker".into(),
        serde_json::from_value(json!({"runner":"actual-device","agent":"kimi-code"})).unwrap(),
    );
    let preview = templates::compile(&def, &request);
    assert_eq!(preview.issues, []);
    assert!(
        preview
            .config
            .unwrap()
            .dag
            .unwrap()
            .nodes
            .iter()
            .all(|n| n.agent == "kimi-code" && n.runner == "actual-device")
    );
}

#[test]
fn documents_preserve_types_and_reject_ambiguous_yaml_or_non_json_values() {
    use templates::document::parse;
    assert_eq!(parse("values: {flag: false, count: 0, note: '', list: [], empty: {}, missing: null}\nunset: [optional]\n").unwrap(),json!({"values":{"flag":false,"count":0,"note":"","list":[],"empty":{},"missing":null},"unset":["optional"]}));
    for source in [
        "a: 1\na: 2",
        "{\"a\":1,\"a\":2}",
        "1: value",
        "x: !custom value",
        "x: .nan",
        "x: .inf",
        "---\na: 1\n---\nb: 2",
    ] {
        assert!(parse(source).is_err(), "{source}");
    }
}

#[test]
fn repository_urls_accept_git_scp_without_treating_local_paths_as_remote_urls() {
    for (url, valid) in [
        ("github.com:org/repo.git", true),
        ("git@github.com:org/repo.git", true),
        ("ssh://git@example.com/org/repo", true),
        ("https://example.com/org/repo", true),
        ("/tmp/repo", false),
        ("C:\\repo", false),
        ("file:///tmp/repo", false),
        ("https://example.com/", false),
    ] {
        let preview = templates::compile(
            &definition(),
            &request(json!({"repositories":[{"key":"a","url":url}]})),
        );
        assert_eq!(
            !preview.issues.iter().any(|i| i.code == "repositoryUrl"),
            valid,
            "{url}: {:?}",
            preview.issues
        );
    }
}

#[test]
fn remote_directory_scopes_use_path_components_not_text_prefixes() {
    use wakeit::template_api::directory_in_roots as within;
    assert!(within("/work/projects/../report", &json!(["/work"]), false));
    assert!(!within("/work-else/report", &json!(["/work"]), false));
    assert!(!within("/work/../../etc", &json!(["/work"]), false));
    assert!(!within("relative/report", &json!(["/"]), false));
    assert!(within("C:\\Work\\report", &json!(["c:/work"]), true));
    assert!(!within("C:\\Work\\..\\private", &json!(["c:/work"]), true));
    assert!(!within("D:/Work/report", &json!(["c:/work"]), true));
}
