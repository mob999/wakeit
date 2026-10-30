pub fn official_agent(id: &str) -> wakeit_core::config::AgentConfig {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("wakeit-plugin.json")).unwrap()).unwrap();
    let path = manifest["wakeit"]["agents"][id].as_str().unwrap();
    let mut value: serde_json::Value =
        yaml_serde::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap();
    value["id"] = id.into();
    serde_json::from_value(value).unwrap()
}
