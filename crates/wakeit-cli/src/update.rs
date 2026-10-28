//! Official release checks and replacement of the current executable.
use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write, path::Path, process::Stdio, time::Duration};

const API: &str = "https://api.github.com";
const REPO: &str = "mob999/wakeit";
const MAX_BINARY: usize = 128 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    url: String,
    size: usize,
}
#[derive(Clone, Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Debug, Deserialize)]
struct Manifest {
    product: String,
    version: String,
    assets: BTreeMap<String, ManifestAsset>,
}
#[derive(Debug, Deserialize)]
struct ManifestAsset {
    name: String,
    sha256: String,
}
#[derive(Debug, Serialize)]
pub struct UpdateReport {
    pub status: &'static str,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub target: &'static str,
}

fn newest(releases: Vec<Release>) -> Option<(Version, Release)> {
    releases
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix("cli-v")?).ok()?;
            version.pre.is_empty().then_some((version, r))
        })
        .max_by(|a, b| a.0.cmp_precedence(&b.0))
}

struct Github {
    http: reqwest::Client,
    token: Option<String>,
    base: String,
}
impl Github {
    async fn get(&self, path: &str, octets: bool, limit: usize) -> Result<Vec<u8>> {
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

    async fn releases(&self) -> Result<Option<(Version, Release)>> {
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

    async fn candidate(
        &self,
        version: &Version,
        release: &Release,
        target: &str,
    ) -> Result<Vec<u8>> {
        let metadata = release
            .assets
            .iter()
            .find(|a| a.name == "wakeit-cli-release.json")
            .context("release is missing wakeit-cli-release.json")?;
        ensure!(
            metadata.size <= MAX_METADATA,
            "release manifest exceeds size limit"
        );
        let manifest: Manifest =
            serde_json::from_slice(&self.get(&metadata.url, true, MAX_METADATA).await?)
                .context("invalid release manifest")?;
        ensure!(
            manifest.product == "cli" && Version::parse(&manifest.version)? == *version,
            "release product or version does not match the CLI"
        );
        let entry = manifest
            .assets
            .get(target)
            .with_context(|| format!("release has no binary for {target}"))?;
        let expected_name = binary_name(target);
        ensure!(
            entry.name == expected_name,
            "unexpected release binary name"
        );
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == entry.name)
            .context("release binary asset is missing")?;
        ensure!(
            asset.size <= MAX_BINARY,
            "release binary exceeds size limit"
        );
        let bytes = self.get(&asset.url, true, MAX_BINARY).await?;
        ensure!(
            bytes.len() == asset.size,
            "release binary size differs from metadata"
        );
        verify_checksum(&bytes, &entry.sha256)?;
        Ok(bytes)
    }
}

fn binary_name(target: &str) -> String {
    format!(
        "wakeit-{target}{}",
        if target.contains("windows") {
            ".exe"
        } else {
            ""
        }
    )
}
fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid release checksum"
    );
    ensure!(
        hex::encode(Sha256::digest(bytes)).eq_ignore_ascii_case(expected),
        "release checksum mismatch; executable was not changed"
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

/// Verify a downloaded candidate before asking the OS to replace this process's
/// executable. Also used by the isolated cross-platform replacement probe.
pub async fn replace_verified(candidate: &Path, version: &Version) -> Result<()> {
    let output = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(candidate)
            .arg("--version")
            .env_clear()
            .envs(std::env::vars_os().filter(|(key, _)| {
                key.to_str().is_some_and(|key| {
                    [
                        "PATH",
                        "SystemRoot",
                        "SYSTEMROOT",
                        "WINDIR",
                        "TEMP",
                        "TMP",
                        "TMPDIR",
                        "HOME",
                        "USERPROFILE",
                    ]
                    .contains(&key)
                })
            }))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("candidate version check timed out")?
    .context("candidate cannot execute on this platform")?;
    ensure!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).trim() == format!("wakeit {version}"),
        "candidate version check failed; executable was not changed"
    );
    let executable = std::env::current_exe()?.canonicalize()?;
    let directory = executable.parent().context("executable has no directory")?;
    let backup = tempfile::Builder::new()
        .prefix(".wakeit-backup-")
        .tempfile_in(directory)?
        .into_temp_path();
    std::fs::copy(&executable, &backup).context("could not back up executable")?;
    if let Err(error) = self_replace::self_replace(candidate) {
        // Windows replacement moves the running file aside first. Restore the
        // public path if a later operation could not install the new binary.
        if !executable.exists() && std::fs::rename(&backup, &executable).is_err() {
            let saved = backup
                .keep()
                .context("could not retain executable backup")?;
            bail!(
                "replacement failed; recover the original from {}: {error}",
                saved.display()
            );
        }
        bail!(
            "could not replace executable; original retained or restored; check directory permissions and running services: {error}"
        );
    }
    Ok(())
}

pub async fn run(check_only: bool, json: bool) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let github = Github {
        http: reqwest::Client::builder()
            .user_agent(concat!("wakeit/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(180))
            .build()?,
        token: credentials().await?,
        base: API.into(),
    };
    let release = github.releases().await?;
    let mut report = UpdateReport {
        status: "no_release",
        current_version: current.to_string(),
        latest_version: release.as_ref().map(|r| r.0.to_string()),
        update_available: false,
        target: env!("WAKEIT_BUILD_TARGET"),
    };
    if let Some((version, release)) = release {
        report.update_available = version.cmp_precedence(&current).is_gt();
        report.status = if report.update_available {
            "update_available"
        } else {
            "up_to_date"
        };
        if report.update_available {
            ensure!(
                release
                    .assets
                    .iter()
                    .any(|asset| asset.name == binary_name(report.target)),
                "new release has no binary for {}",
                report.target
            );
            ensure!(
                release
                    .assets
                    .iter()
                    .any(|asset| asset.name == "wakeit-cli-release.json"),
                "release is missing wakeit-cli-release.json"
            );
        }
        if report.update_available && !check_only {
            let executable = std::env::current_exe()?.canonicalize()?;
            let directory = executable
                .parent()
                .context("executable has no parent directory")?;
            // Do not unlink the lock file: an unlinked flock permits competing
            // updates to acquire locks on different inodes.
            let lock = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(directory.join(".wakeit-update.lock"))
                .context("update needs write access to the executable directory")?;
            fs2::FileExt::try_lock_exclusive(&lock).context("another update is in progress")?;
            let bytes = github.candidate(&version, &release, report.target).await?;
            let mut file = tempfile::Builder::new()
                .prefix(".wakeit-update-")
                .suffix(std::env::consts::EXE_SUFFIX)
                .tempfile_in(directory)?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            std::fs::set_permissions(file.path(), std::fs::metadata(&executable)?.permissions())?;
            // Windows cannot execute a candidate while its writable handle is
            // open; keep only the TempPath until replacement finishes.
            let path = file.into_temp_path();
            replace_verified(&path, &version).await?;
            report.status = "updated";
            report.update_available = false;
        }
    }
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        match report.status {
            "no_release" => println!(
                "No official release has been published. Current: {}.",
                report.current_version
            ),
            "up_to_date" => println!("Wakeit {} is up to date.", report.current_version),
            "updated" => println!(
                "Updated to Wakeit {}. Running Runner processes require a separate restart.",
                report.latest_version.as_deref().unwrap()
            ),
            _ => println!(
                "Wakeit {} is available (current {}). Run: wakeit update",
                report.latest_version.as_deref().unwrap(),
                report.current_version
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    fn release(tag: &str, draft: bool, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            draft,
            prerelease,
            assets: vec![],
        }
    }
    #[test]
    fn official_versions_are_numeric_and_not_publish_order() {
        let (version, _) = newest(vec![
            release("cli-v0.9.0", false, false),
            release("cli-v0.10.0", false, false),
            release("cli-v1.0.0", true, false),
            release("cli-v2.0.0", false, true),
            release("cli-v3.0.0-rc.1", false, false),
            release("preview-main", false, false),
            release("gateway-v99.0.0", false, false),
            release("v99.0.0", false, false),
        ])
        .unwrap();
        assert_eq!(version, Version::parse("0.10.0").unwrap());
        assert!(newest(vec![release("cli-v0.1.0", true, false)]).is_none());
    }
    #[test]
    fn checksum_rejects_corruption() {
        let digest = hex::encode(Sha256::digest(b"binary"));
        assert!(verify_checksum(b"binary", &digest).is_ok());
        assert!(verify_checksum(b"modified", &digest).is_err());
        assert!(verify_checksum(b"binary", "broken").is_err());
        assert!(binary_name("x86_64-pc-windows-msvc").ends_with(".exe"));
    }
    async fn github(router: Router) -> Github {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Github {
            http: reqwest::Client::new(),
            token: Some("fixture-secret".into()),
            base,
        }
    }
    #[tokio::test]
    async fn no_releases_and_permission_errors_are_distinct() {
        let client = github(Router::new().route(
            "/repos/mob999/wakeit/releases",
            get(|headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer fixture-secret");
                "[]"
            }),
        ))
        .await;
        assert!(client.releases().await.unwrap().is_none());
        let client = github(Router::new().route(
            "/repos/mob999/wakeit/releases",
            get(|| async { StatusCode::NOT_FOUND }),
        ))
        .await;
        let error = client.releases().await.unwrap_err().to_string();
        assert!(error.contains("inaccessible"));
        assert!(!error.contains("fixture-secret"));
    }
    #[tokio::test]
    async fn public_release_check_does_not_require_github_login() {
        let mut client = github(Router::new().route(
            "/repos/mob999/wakeit/releases",
            get(|headers: HeaderMap| async move {
                assert!(!headers.contains_key("authorization"));
                Json(serde_json::json!([{"tag_name":"cli-v0.1.0","draft":false,"prerelease":true,"assets":[]}]))
            }),
        )).await;
        client.token = None;
        assert!(client.releases().await.unwrap().is_none());
    }
    #[tokio::test]
    async fn asset_urls_must_stay_on_the_repository_api() {
        let client = github(Router::new()).await;
        assert!(
            client
                .get("https://other.invalid/asset", true, 1024)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn manifest_download_validates_version_platform_size_and_hash() {
        let target = env!("WAKEIT_BUILD_TARGET");
        let name = binary_name(target);
        let body = b"fixture binary".to_vec();
        let checksum = hex::encode(Sha256::digest(&body));
        let manifest = serde_json::json!({"product":"cli","version":"0.2.0","assets":{target:{"name":name,"sha256":checksum}}}).to_string();
        let manifest_size = manifest.len();
        let client = github(
            Router::new()
                .route(
                    "/repos/mob999/wakeit/releases/assets/1",
                    get(move || {
                        let manifest = manifest.clone();
                        async move { manifest }
                    }),
                )
                .route(
                    "/repos/mob999/wakeit/releases/assets/2",
                    get(move || {
                        let body = body.clone();
                        async move { body }
                    }),
                ),
        )
        .await;
        let mut release = release("cli-v0.2.0", false, false);
        release.assets = vec![
            Asset {
                name: "wakeit-cli-release.json".into(),
                url: format!("{}/repos/{REPO}/releases/assets/1", client.base),
                size: manifest_size,
            },
            Asset {
                name,
                url: format!("{}/repos/{REPO}/releases/assets/2", client.base),
                size: 14,
            },
        ];
        let version = Version::parse("0.2.0").unwrap();
        assert_eq!(
            client.candidate(&version, &release, target).await.unwrap(),
            b"fixture binary"
        );
        assert!(
            client
                .candidate(&version, &release, "unsupported-target")
                .await
                .unwrap_err()
                .to_string()
                .contains("no binary")
        );
        assert!(
            client
                .candidate(&Version::parse("0.3.0").unwrap(), &release, target)
                .await
                .unwrap_err()
                .to_string()
                .contains("differ")
        );
        release.assets[1].size += 1;
        assert!(
            client
                .candidate(&version, &release, target)
                .await
                .unwrap_err()
                .to_string()
                .contains("size differs")
        );
    }
    #[tokio::test]
    async fn interrupted_and_oversized_downloads_do_not_become_candidates() {
        let client = github(Router::new().route(
            "/repos/mob999/wakeit/releases/assets/1",
            get(|| async { "too large".into_response() }),
        ))
        .await;
        assert!(
            client
                .get("/repos/mob999/wakeit/releases/assets/1", true, 3)
                .await
                .is_err()
        );
    }
}
