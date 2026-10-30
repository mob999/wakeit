use super::{
    Package, Registry, home, manifest, packages, registry, runner_status, source::Source,
    validate_selection,
};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, time::Duration};

fn lock() -> Result<std::fs::File> {
    crate::runner::config::private_directory(&home()?)?;
    let file = crate::runner::config::private_file(&home()?.join("packages.lock"))?;
    file.lock_exclusive()?;
    Ok(file)
}
fn save(registry: &Registry) -> Result<()> {
    crate::connection::atomic(
        &home()?.join("packages.json"),
        serde_json::to_vec_pretty(registry)?,
    )
}
async fn git(directory: Option<&Path>, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        command
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("Git operation timed out")??;
    // Git can echo credential-bearing remote URLs. Do not include raw stderr.
    ensure!(
        output.status.success(),
        "Git operation failed; check repository access and ref ({})",
        args.first().unwrap_or(&"git")
    );
    Ok(String::from_utf8(output.stdout)?.trim().into())
}
async fn git_exists(root: &Path, reference: &str) -> bool {
    git(Some(root), &["show-ref", "--verify", reference])
        .await
        .is_ok()
}
async fn prepare(source: Source) -> Result<Package> {
    if let Source::Local { path } = &source {
        let info = manifest::read(path)?;
        manifest::validate(path)?;
        return Ok(Package {
            source: source.clone(),
            root: path.clone(),
            name: info.name,
            version: info.version,
            revision: None,
            pinned: false,
            bundled: false,
        });
    }
    let directory = home()?
        .join("packages")
        .join(hex::encode(Sha256::digest(source.identity())));
    std::fs::create_dir_all(&directory)?;
    let temporary = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(&directory)?;
    let root = temporary.path().join("package");
    let (url, reference) = source.git()?;
    git(
        None,
        &[
            "clone",
            "--no-checkout",
            "--",
            url,
            root.to_str().context("package path is not UTF-8")?,
        ],
    )
    .await?;
    let (reference, pinned) = if let Some(reference) = reference {
        let tag = format!("refs/tags/{reference}");
        let branch = format!("refs/remotes/origin/{reference}");
        if git_exists(&root, &tag).await {
            (tag, true)
        } else if git_exists(&root, &branch).await {
            (branch, false)
        } else {
            ensure!(
                reference.len() >= 7 && reference.bytes().all(|b| b.is_ascii_hexdigit()),
                "Git ref does not exist"
            );
            (reference.into(), true)
        }
    } else {
        ("HEAD".into(), false)
    };
    let revision = git(
        Some(&root),
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
    )
    .await?;
    git(Some(&root), &["checkout", "--detach", &revision]).await?;
    let info = manifest::read(&root)?;
    if !info.wakeit.artifacts.is_empty() || info.wakeit.artifact.is_some() {
        let artifact = info
            .wakeit
            .artifacts
            .get(env!("WAKEIT_BUILD_TARGET"))
            .or(info.wakeit.artifact.as_ref())
            .context("plugin has no prebuilt artifact for this platform")?;
        let expand = |v: &str| {
            v.replace("{version}", &info.version)
                .replace("{target}", env!("WAKEIT_BUILD_TARGET"))
        };
        let url = reqwest::Url::parse(&expand(&artifact.url))?;
        ensure!(
            url.scheme() == "https"
                || (url.scheme() == "http"
                    && url
                        .host_str()
                        .is_some_and(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]")),
            "plugin artifacts require HTTPS"
        );
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?
            .get(url)
            .send()
            .await?
            .error_for_status()?;
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 256 * 1024 * 1024,
                "plugin artifact exceeds 256 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let digest = if let Some(digest) = &artifact.sha256 {
            digest.clone()
        } else {
            let checksums = artifact
                .checksums_url
                .as_ref()
                .context("plugin artifact needs sha256 or checksums_url")?;
            let checksum_url = reqwest::Url::parse(&expand(checksums))?;
            ensure!(
                checksum_url.scheme() == "https"
                    || (checksum_url.scheme() == "http"
                        && checksum_url
                            .host_str()
                            .is_some_and(|h| h == "127.0.0.1" || h == "localhost")),
                "plugin checksums require HTTPS"
            );
            let response = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?
                .get(checksum_url)
                .send()
                .await?
                .error_for_status()?;
            ensure!(
                response.content_length().is_none_or(|n| n <= 1024 * 1024),
                "plugin checksum list is too large"
            );
            let text = response.text().await?;
            ensure!(
                text.len() <= 1024 * 1024,
                "plugin checksum list is too large"
            );
            let name = reqwest::Url::parse(&expand(&artifact.url))?
                .path_segments()
                .and_then(|mut s| s.next_back())
                .context("artifact filename")?
                .to_owned();
            text.lines()
                .find_map(|line| {
                    let mut fields = line.split_whitespace();
                    let digest = fields.next()?;
                    let file = fields.next()?.trim_start_matches('*');
                    (file == name).then(|| digest.to_owned())
                })
                .context("artifact is missing from checksum list")?
        };
        crate::releases::verify_checksum(&bytes, &digest)?;
        unpack(
            &bytes,
            &root,
            &info.wakeit.agents.values().cloned().collect::<Vec<_>>(),
        )?;
    }
    manifest::validate(&root)?;
    let destination = directory.join(&revision);
    if !destination.exists() {
        std::fs::rename(&root, &destination)?;
    }
    Ok(Package {
        source,
        root: destination,
        name: info.name,
        version: info.version,
        revision: Some(revision),
        pinned,
        bundled: false,
    })
}
/// Extract regular files and directories only; archive paths cannot escape a package.
pub(crate) fn unpack(
    bytes: &[u8],
    directory: &Path,
    definitions: &[std::path::PathBuf],
) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        manifest::relative(&path)?;
        let normalized = |path: &Path| {
            path.components()
                .filter(|c| !matches!(c, std::path::Component::CurDir))
                .collect::<std::path::PathBuf>()
        };
        ensure!(
            !definitions
                .iter()
                .any(|definition| normalized(definition) == normalized(&path)),
            "plugin artifact cannot replace an Agent definition from the selected Git revision"
        );
        ensure!(
            path != Path::new(manifest::FILE) && !path.starts_with(".git"),
            "plugin artifact cannot replace package metadata"
        );
        ensure!(
            entry.header().entry_type().is_file() || entry.header().entry_type().is_dir(),
            "plugin archive cannot contain links or special files"
        );
        ensure!(
            entry.unpack_in(directory)?,
            "plugin archive path escapes its package"
        );
    }
    Ok(())
}
pub(super) async fn install(value: &str, replace: &[String]) -> Result<Value> {
    let source = Source::parse(value, &std::env::current_dir()?)?;
    let package = prepare(source).await?;
    let _lock = lock()?;
    let mut registry = registry()?;
    let identity = package.source.identity();
    registry
        .packages
        .retain(|p| p.source.identity() != identity);
    registry.removed.retain(|s| s != &identity);
    let provided = manifest::read(&package.root)?.wakeit.agents;
    for id in replace {
        ensure!(
            provided.contains_key(id),
            "plugin does not provide agent {id}"
        );
        registry.agent_sources.insert(id.clone(), identity.clone());
    }
    registry.packages.push(package.clone());
    validate_selection(&registry)?;
    save(&registry)?;
    Ok(
        json!({"status":"installed","source":package.source.display(),"name":package.name,"version":package.version,"revision":package.revision,"activation":"next_runner_start","runners":runner_status(&registry)?}),
    )
}
pub(super) async fn update(value: Option<&str>) -> Result<Value> {
    let registry = registry()?;
    let identity = value
        .map(|v| Source::parse(v, &std::env::current_dir()?).map(|s| s.identity()))
        .transpose()?;
    let selected = packages(&registry, true)?
        .into_iter()
        .filter(|p| {
            identity
                .as_ref()
                .is_none_or(|id| &p.source.identity() == id)
        })
        .collect::<Vec<_>>();
    ensure!(
        identity.is_none() || !selected.is_empty(),
        "plugin source is not installed"
    );
    let mut results = Vec::new();
    for p in selected {
        if p.pinned || matches!(p.source, Source::Local { .. }) {
            results.push(
                json!({"source":p.source.display(),"status":if p.pinned{"pinned"}else{"local"}}),
            );
        } else {
            results.push(install(&p.source.display(), &[]).await?);
        }
    }
    Ok(json!({"packages":results}))
}
pub(super) fn remove(value: &str) -> Result<Value> {
    let source = Source::parse(value, &std::env::current_dir()?)?;
    let identity = source.identity();
    let _lock = lock()?;
    let mut registry = registry()?;
    ensure!(
        packages(&registry, true)?
            .iter()
            .any(|p| p.source.identity() == identity),
        "plugin source is not installed"
    );
    registry
        .packages
        .retain(|p| p.source.identity() != identity);
    registry.agent_sources.retain(|_, s| s != &identity);
    if !registry.removed.contains(&identity) {
        registry.removed.push(identity);
    }
    save(&registry)?;
    Ok(
        json!({"status":"removed","source":source.display(),"activation":"next_runner_start","runners":runner_status(&registry)?}),
    )
}
