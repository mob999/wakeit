use super::version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn concurrency() -> usize {
    2
}
fn address() -> String {
    "127.0.0.1:8080".into()
}
fn replicas() -> usize {
    1
}
fn log_limit() -> usize {
    20 * 1024 * 1024
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayConfig {
    #[serde(default = "version")]
    pub version: u32,
    #[serde(default = "address")]
    pub listen: String,
    pub public_url: String,
    pub database_url_env: String,
    pub nats_url: String,
    pub nats_public_url: String,
    pub nats_auth_dir: PathBuf,
    #[serde(default = "replicas")]
    pub nats_replicas: usize,
    #[serde(default = "log_limit")]
    pub log_limit_bytes: usize,
    #[serde(default)]
    pub auth: GatewayAuth,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewayAuth {
    #[serde(default = "session_cookie")]
    pub session_cookie: String,
    #[serde(default = "local_auth")]
    pub mode: String,
    #[serde(default)]
    pub allow_registration: bool,
    #[serde(default)]
    pub github: Option<GitHubAuth>,
    #[serde(default)]
    pub demo_workspace: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubAuth {
    pub client_id_env: String,
    pub client_secret_env: String,
}
fn local_auth() -> String {
    "local".into()
}
fn session_cookie() -> String {
    "wakeit_session".into()
}
impl Default for GatewayAuth {
    fn default() -> Self {
        Self {
            session_cookie: session_cookie(),
            mode: local_auth(),
            allow_registration: false,
            github: None,
            demo_workspace: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerConfig {
    #[serde(default = "version")]
    pub version: u32,
    pub gateway: String,
    pub id: uuid::Uuid,
    pub name: String,
    pub token_file: PathBuf,
    pub nats_url: String,
    pub credentials_file: PathBuf,
    pub data_dir: PathBuf,
    #[serde(default = "concurrency")]
    pub concurrency: usize,
    pub agents: Vec<PathBuf>,
    #[serde(default)]
    pub allowed_roots: Vec<PathBuf>,
}
