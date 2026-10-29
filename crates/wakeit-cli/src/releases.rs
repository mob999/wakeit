//! Downloads from official CLI releases, shared by executable and Skill installation.
use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{process::Stdio, time::Duration};

const API: &str = "https://api.github.com";
pub(crate) const REPO: &str = "mob999/wakeit";
pub(crate) const MAX_METADATA: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Asset {
    pub(crate) name: String,
    pub(crate) url: String,
    pub(crate) size: usize,
}
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Release {
    pub(crate) tag_name: String,
    pub(crate) draft: bool,
    pub(crate) prerelease: bool,
    pub(crate) assets: Vec<Asset>,
}
pub(crate) fn newest(releases: Vec<Release>) -> Option<(Version, Release)> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix("cli-v")?).ok()?;
            version.pre.is_empty().then_some((version, r))
        })
        .max_by(|a, b| a.0.cmp_precedence(&b.0))
}

pub(crate) struct Github {
    pub(crate) http: reqwest::Client,
    pub(crate) token: Option<String>,
    pub(crate) base: String,
}
impl Github {
    pub(crate) async fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(concat!("wakeit/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(180))
                .build()?,
            token: credentials().await?,
            base: API.into(),
        })
    }

    pub(crate) async fn get(&self, path: &str, octets: bool, limit: usize) -> Result<Vec<u8>> {
        let url = if path.starts_with('/') {
            format!("{}{path}", self.base)
        } else {
            path.into()
        };
        // Asset API URLs may use authentication; never forward a token to an
        // arbitrary manifest URL. reqwest strips credentials across redirects.
        ensure!(
            url.starts_with(&format!("{}/repos/{REPO}/", self.base)),
            "invalid GitHub asset URL"
        );
        let mut request = self.http.get(url);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .header(
                "Accept",
                if octets {
                    "application/octet-stream"
                } else {
                    "application/vnd.github+json"
                },
            )
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("GitHub request failed; check the network connection"))?;
        match response.status().as_u16() {
            401 | 403 => bail!("GitHub authentication, repository permission or rate limit error"),
            404 => bail!("repository or release asset is inaccessible"),
            code if !(200..300).contains(&code) => bail!("GitHub returned HTTP {code}"),
            _ => {}
        }
        if let Some(length) = response.content_length() {
            ensure!(
                length <= limit as u64,
                "release download exceeds size limit"
            );
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow::anyhow!("release download interrupted"))?;
            ensure!(
                bytes.len() + chunk.len() <= limit,
                "release download exceeds size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub(crate) async fn releases(&self) -> Result<Option<(Version, Release)>> {
        let mut releases = Vec::new();
        for page in 1..=100 {
            let bytes = self
                .get(
                    &format!("/repos/{REPO}/releases?per_page=100&page={page}"),
                    false,
                    MAX_METADATA,
                )
                .await?;
            let batch: Vec<Release> =
                serde_json::from_slice(&bytes).context("invalid GitHub release metadata")?;
            let done = batch.len() < 100;
            releases.extend(batch);
            if done {
                return Ok(newest(releases));
            }
        }
        bail!("too many release pages; could not determine the newest version")
    }
}
pub(crate) fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid release checksum"
    );
    ensure!(
        hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(expected),
        "release checksum mismatch; installed files were not changed"
    );
    Ok(())
}
async fn credentials() -> Result<Option<String>> {
    for name in ["WAKEIT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(name)
            && !value.trim().is_empty()
        {
            return Ok(Some(value.trim().into()));
        }
    }
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("gh")
            .args(["auth", "token", "--hostname", "github.com"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    if let Ok(Ok(output)) = result
        && output.status.success()
    {
        let token = String::from_utf8(output.stdout).unwrap_or_default();
        if !token.trim().is_empty() {
            return Ok(Some(token.trim().into()));
        }
    }
    Ok(None)
}
