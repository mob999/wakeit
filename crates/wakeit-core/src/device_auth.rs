use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub os: String,
    #[serde(default)]
    pub register_runner: bool,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
}
pub fn validate_url(value: &str) -> anyhow::Result<String> {
    let u = reqwest::Url::parse(value)?;
    anyhow::ensure!(
        u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path().trim_matches('/').is_empty(),
        "Gateway must be an origin without credentials, path, query or fragment"
    );
    let local = matches!(
        u.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    anyhow::ensure!(
        u.scheme() == "https" || (u.scheme() == "http" && local),
        "Gateway requires HTTPS (HTTP is allowed only on loopback)"
    );
    Ok(u.as_str().trim_end_matches('/').to_string())
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub struct Connect {
    pub request_id: Uuid,
    pub public_key: String,
    pub signature: String,
    pub timestamp: i64,
    pub name: String,
    pub allowed_roots: Vec<String>,
}
pub fn proof(id: Uuid, name: &str, roots: &[String], timestamp: i64) -> String {
    serde_json::to_string(&serde_json::json!([
        "wakeit-connect-v1",
        id,
        name,
        roots,
        timestamp
    ]))
    .unwrap()
}
