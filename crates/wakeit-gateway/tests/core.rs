#[test]
fn openapi_references_resolve_in_the_document() {
    let doc = wakeit_gateway::openapi::document();
    fn check(value: &serde_json::Value, doc: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(serde_json::Value::as_str) {
                    assert!(
                        doc.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                        "broken reference: {reference}"
                    );
                }
                for value in map.values() {
                    check(value, doc);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    check(value, doc);
                }
            }
            _ => {}
        }
    }
    check(&doc, &doc);
    assert!(
        doc["paths"]["/api/v1/workspaces/{workspace}/tasks/{id}/runs"]["post"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Idempotency-Key")
    );
}
#[test]
fn openapi_exposes_task_template_contract() {
    let doc = wakeit_gateway::openapi::document();
    let base = "/api/v1/workspaces/{workspace}";
    for (path, method) in [
        ("/task-templates", "get"),
        ("/task-template-preview", "post"),
        ("/task-templates/{id}", "get"),
        ("/task-templates/{id}", "put"),
        ("/task-templates/{id}/archive", "post"),
        ("/task-templates/{id}/restore", "post"),
        ("/task-templates/{id}/preview", "post"),
        ("/task-templates/{id}/instantiate", "post"),
        ("/tasks/{id}/template", "get"),
    ] {
        let operation = &doc["paths"][format!("{base}{path}")][method];
        assert!(operation.is_object(), "missing {method} {path}");
        assert!(
            operation["parameters"]
                .as_array()
                .unwrap()
                .iter()
                .any(|parameter| parameter["name"] == "workspace"),
            "missing workspace scope for {method} {path}"
        );
    }
    let instantiate = &doc["paths"][format!("{base}/task-templates/{{id}}/instantiate")]["post"];
    assert_eq!(
        instantiate["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/TaskTemplateRequest"
    );
    assert_eq!(
        instantiate["responses"]["200"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/TaskTemplateInstance"
    );
    assert!(
        instantiate["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|parameter| parameter["name"] == "Idempotency-Key")
    );
    assert_eq!(
        instantiate["responses"]["422"]["content"]["application/json"]["schema"]["properties"]["issues"]
            ["items"]["$ref"],
        "#/components/schemas/TaskTemplateIssue"
    );
    assert_eq!(
        doc["components"]["schemas"]["TaskTemplateInstance"]["properties"]["run_id"]["type"],
        serde_json::json!(["string", "null"])
    );
    assert!(
        doc["paths"]["/api/v1/schema/{kind}"]["get"]["parameters"][0]["schema"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|kind| kind == "template")
    );
}
