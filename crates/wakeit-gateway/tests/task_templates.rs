use serde_json::json;
#[test]
fn remote_directory_scopes_use_path_components_not_text_prefixes() {
    use wakeit_gateway::template_api::directory_in_roots as within;
    assert!(within("/work/projects/../report", &json!(["/work"]), false));
    assert!(!within("/work-else/report", &json!(["/work"]), false));
    assert!(!within("/work/../../etc", &json!(["/work"]), false));
    assert!(!within("relative/report", &json!(["/"]), false));
    assert!(within("C:\\Work\\report", &json!(["c:/work"]), true));
    assert!(!within("C:\\Work\\..\\private", &json!(["c:/work"]), true));
    assert!(!within("D:/Work/report", &json!(["c:/work"]), true));
}
