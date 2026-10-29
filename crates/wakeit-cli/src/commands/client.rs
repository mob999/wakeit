use super::args::Cli;
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;

pub(super) struct Client {
    pub(super) http: reqwest::Client,
    pub(super) url: String,
    pub(super) token: String,
    pub(super) key: String,
    pub(super) workspace: Uuid,
}
impl Client {
    pub(super) fn new(cli: &Cli) -> Result<Self> {
        let token = cli
            .token
            .clone()
            .or_else(|| std::fs::read_to_string(&cli.token_file).ok())
            .context("login first, or set WAKEIT_TOKEN / WAKEIT_TOKEN_FILE")?;
        Ok(Self {
            workspace: cli
                .workspace
                .context("choose a workspace with --workspace or connection use")?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            url: cli.gateway.trim_end_matches('/').into(),
            token,
            key: cli
                .request_key
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        })
    }
    pub(super) fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1/workspaces/{}{path}", self.url, self.workspace)
    }
    pub(super) async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, self.api_url(path))
            .bearer_auth(self.token.trim())
            .header("Idempotency-Key", &self.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let value: Value = response.json().await?;
        ensure!(
            status.is_success(),
            "{status}: {}{}",
            value["error"],
            value
                .get("issues")
                .map(|v| format!("\n{}", serde_json::to_string_pretty(v).unwrap_or_default()))
                .unwrap_or_default()
        );
        Ok(value)
    }
    pub(super) async fn optional(&self, path: &str) -> Result<Option<Value>> {
        let response = self
            .http
            .get(self.api_url(path))
            .bearer_auth(self.token.trim())
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let status = response.status();
        let value: Value = response.json().await?;
        ensure!(status.is_success(), "{status}: {}", value["error"]);
        Ok(Some(value))
    }
    pub(super) async fn get(&self, path: &str) -> Result<Value> {
        self.request(reqwest::Method::GET, path, None).await
    }
    pub(super) async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }
    pub(super) async fn post_idempotent(&self, path: &str, body: Value) -> Result<Value> {
        for attempt in 0..3 {
            match self.post(path, body.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) if attempt < 2 && error.downcast_ref::<reqwest::Error>().is_some() => {
                    tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!()
    }
}
