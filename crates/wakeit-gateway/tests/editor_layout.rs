#[test]
fn openapi_exposes_shared_layout_only_on_task_records_and_apply() {
    let doc = wakeit_gateway::openapi::document();
    let schemas = &doc["components"]["schemas"];
    assert!(
        schemas["ApplyTask"]["properties"]
            .get("editor_layout")
            .is_some()
    );
    assert!(
        schemas["TaskRecord"]["properties"]
            .get("editor_layout")
            .is_some()
    );
    assert!(schemas["Task"]["properties"].get("editor_layout").is_none());
    assert!(schemas["Run"]["properties"].get("editor_layout").is_none());
}
