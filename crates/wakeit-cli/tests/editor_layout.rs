use serde_json::json;
use wakeit::{config::TaskConfig, editor_layout::EditorLayout};
fn graph() -> TaskConfig {
    serde_json::from_value(json!({"version":1,"id":"layout-test","name":"Layout test","schedule":null,
        "dag":{"nodes":[{"id":"a","name":"Source","runner":"preview","agent":"pi","cwd":"/workspace","prompt":"collect"},
        {"id":"b","name":"Report","pre":["a"],"runner":"preview","agent":"pi","cwd":"/workspace","prompt":"report"}]}})).unwrap()
}
fn layout() -> EditorLayout {
    serde_json::from_value(
        json!({"version":1,"positions":{"a":{"x":-120.5,"y":0},"b":{"x":320,"y":180}}}),
    )
    .unwrap()
}
#[test]
fn shared_layout_stays_outside_execution_configuration() {
    let c = graph();
    let l = layout();
    l.validate(&c).unwrap();
    assert!(
        serde_json::to_value(&c)
            .unwrap()
            .get("editor_layout")
            .is_none()
    );
    let mut c = c;
    c.dag.as_mut().unwrap().nodes.pop();
    let pruned = l.retain_nodes(&c).unwrap();
    assert_eq!(pruned.positions.len(), 1);
    assert_eq!(pruned.positions["a"].x, -120.5);
    c.dag = None;
    assert!(pruned.retain_nodes(&c).is_none());
}
#[test]
fn layout_rejects_unknown_nodes_versions_and_nonfinite_coordinates() {
    let c = graph();
    let mut l = layout();
    l.version = 2;
    assert!(l.validate(&c).is_err());
    l.version = 1;
    l.positions.get_mut("a").unwrap().x = f64::NAN;
    assert!(l.validate(&c).is_err());
    l.positions.get_mut("a").unwrap().x = f64::INFINITY;
    assert!(l.validate(&c).is_err());
    let mut l = layout();
    l.positions.insert("other".into(), l.positions["a"].clone());
    assert!(l.validate(&c).is_err());
    let mut c = c;
    c.dag = None;
    assert!(layout().validate(&c).is_err());
    assert!(
        serde_json::from_value::<EditorLayout>(
            json!({"version":1,"positions":{"a":{"x":null,"y":0}}})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<EditorLayout>(json!({"version":1,"positions":{},"extra":true}))
            .is_err()
    );
}
