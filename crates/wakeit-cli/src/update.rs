//! Official release checks and replacement of the current executable.
#[cfg(test)]
use crate::releases::newest;
use crate::releases::{Github, MAX_METADATA, Release, verify_checksum};
use anyhow::{Context, Result, bail, ensure};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, process::Stdio, time::Duration};

const MAX_BINARY: usize = 128 * 1024 * 1024;

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

impl Github {
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
    format!("wakeit-{target}.tar.gz")
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

/// Install a complete CLI release beside this executable, retaining older bundles.
/// Callers validate the release checksum and hold the update lock before invoking this.
pub async fn replace_bundle(bytes: &[u8], version: &Version) -> Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let directory = executable.parent().context("executable directory")?;
    let staged = tempfile::Builder::new()
        .prefix(".wakeit-update-")
        .tempdir_in(directory)?;
    crate::plugins::unpack_bundle(bytes, staged.path(), &[])?;
    let candidate = staged.path().join(if cfg!(windows) {
        "wakeit.exe"
    } else {
        "wakeit"
    });
    let bundle = staged
        .path()
        .join("wakeit-plugins")
        .join(version.to_string());
    crate::plugins::manifest::validate(&bundle)?;
    let installed = directory.join("wakeit-plugins").join(version.to_string());
    std::fs::create_dir_all(installed.parent().unwrap())?;
    // A distinct version directory preserves every running Runner and saved session.
    ensure!(
        !installed.exists(),
        "plugin bundle for this version already exists; inspect the installation before retrying"
    );
    std::fs::rename(&bundle, &installed)?;
    if let Err(error) = replace_verified(&candidate, version).await {
        let _ = std::fs::remove_dir_all(&installed);
        return Err(error);
    }
    Ok(())
}

pub async fn run(check_only: bool, json: bool) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let github = Github::new().await?;
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
            replace_bundle(&bytes, &version).await?;
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
    use crate::releases::{Asset, REPO};
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    use sha2::{Digest, Sha256};
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
        assert!(binary_name("x86_64-pc-windows-msvc").ends_with(".tar.gz"));
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
                .contains("does not match the CLI")
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
