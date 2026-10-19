use serde_json::json;
use std::collections::BTreeMap;
use wakeit::{
    config::TaskConfig,
    dag::{self, Dag},
};

fn graph() -> TaskConfig {
    serde_json::from_value(json!({"version":1,"id":"daily","name":"Daily","schedule":null,
      "dag":{"nodes":[
        {"id":"a","name":"Collect A","runner":"one","agent":"pi","cwd":"/work","prompt":"collect"},
        {"id":"b","name":"Collect B","runner":"two","agent":"codex","cwd":"/work","prompt":"collect"},
        {"id":"c","name":"Summarize","pre":["a","b"],"runner":"one","agent":"pi","cwd":"/work","prompt":"{{ nodes.a.result }}\n{{ nodes.b.result }}"}
      ]}})).unwrap()
}
#[test]
fn validates_inline_multi_root_graph_and_preserves_version_one() {
    let c = graph();
    c.validate().unwrap();
    let d = c.dag.as_ref().unwrap();
    assert_eq!(d.outputs(), ["c"]);
    assert_eq!(d.ancestors("c").into_iter().collect::<Vec<_>>(), ["a", "b"]);
    let wire = serde_json::to_value(&c).unwrap();
    assert!(wire.get("runner").is_none());
    let execution = d.nodes[2].execution(&c, "resolved".into());
    execution.validate().unwrap();
    assert!(
        serde_json::to_value(execution)
            .unwrap()
            .get("dag")
            .is_none()
    );
}
#[test]
fn rejects_cycles_references_duplicates_and_empty_output_selection() {
    let mut c = graph();
    let d = c.dag.as_mut().unwrap();
    d.nodes[0].pre = vec!["c".into()];
    assert!(d.issues().iter().any(|i| !i.cycle.is_empty()));
    d.nodes[0].pre = vec!["missing".into()];
    assert!(d.validate().is_err());
    d.nodes[0].pre.clear();
    d.nodes[2].pre = vec!["a".into(), "a".into()];
    assert!(d.validate().is_err());
    d.nodes[2].pre = vec!["a".into(), "b".into()];
    d.results = Some(vec![]);
    assert!(d.validate().is_err());
    d.results = Some(vec!["a".into()]);
    d.nodes[0].prompt = "{{ nodes.c.result }}".into();
    assert!(d.validate().is_err());
    d.nodes[0].prompt = "a".into();
    d.nodes.push(d.nodes[0].clone());
    assert!(d.validate().is_err());
}
#[test]
fn resolves_unicode_and_json_once_without_evaluating_inserted_content() {
    let results = BTreeMap::from([
        ("a".into(), json!("中文 {{ nodes.b.result }}")),
        ("b".into(), json!({"value":42})),
    ]);
    let actual = dag::render(
        "{{ unrelated.example }} {{ nodes.a.result }}\n{{ nodes.b.result }}",
        &results,
    )
    .unwrap();
    assert!(actual.contains("中文 {{ nodes.b.result }}"));
    assert!(actual.contains("\"value\": 42"));
    assert!(actual.starts_with("{{ unrelated.example }}"));
    assert!(dag::render("{{ nodes.missing.result }}", &results).is_err());
    assert!(dag::references("{{ nodes.a.result").is_err());
    assert!(dag::references("{{ nodes.a.run }}").is_err());
    assert!(
        dag::render(
            "{{ nodes.a.result }}",
            &BTreeMap::from([("a".into(), json!({"truncated":true,"preview":"partial"}))])
        )
        .is_err()
    );
}
#[test]
fn aggregates_warning_only_after_all_executions_settle() {
    let mut d: Dag = graph().dag.unwrap();
    let mut states = BTreeMap::from([
        ("a".into(), "succeeded".into()),
        ("b".into(), "failed".into()),
        ("c".into(), "skipped".into()),
    ]);
    assert_eq!(dag::outcome(&d, &states), "failed");
    d.results = Some(vec!["a".into()]);
    assert_eq!(dag::outcome(&d, &states), "warning");
    states.insert("b".into(), "unknown".into());
    assert_eq!(dag::outcome(&d, &states), "unknown");
    states.insert("b".into(), "running".into());
    assert_eq!(dag::outcome(&d, &states), "running");
    for value in states.values_mut() {
        *value = "succeeded".into();
    }
    assert_eq!(dag::outcome(&d, &states), "succeeded");
}
