//! Gateway/workspace/account-scoped profiles. Tokens remain in private files.
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Profile {
    #[serde(default)]
    pub workspace_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub workspace_name: Option<String>,
    #[serde(default)]
    pub user_id: Option<uuid::Uuid>,
    pub gateway: String,
    pub username: String,
    pub token_file: PathBuf,
    #[serde(default)]
    pub runner_config: Option<PathBuf>,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Profiles {
    pub current: Option<String>,
    pub connections: BTreeMap<String, Profile>,
}
pub fn directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = PathBuf::from(std::env::var_os("APPDATA").context("APPDATA is unset")?);
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?)
        .join("Library/Application Support");
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(
            PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?).join(".config"),
        );
    let dir = base.join("wakeit");
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
pub fn atomic(path: &Path, bytes: impl AsRef<[u8]>) -> Result<()> {
    let parent = path.parent().context("file has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(bytes.as_ref())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn load() -> Result<Profiles> {
    let path = directory()?.join("connections.json");
    if !path.exists() {
        return Ok(Profiles::default());
    }
    serde_json::from_slice(&std::fs::read(path)?).context("invalid connection profiles")
}
pub fn name(gateway: &str) -> String {
    let host = reqwest::Url::parse(gateway)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_else(|| "gateway".into());
    format!("{}-{}", host, &wakeit_core::identity::hash(gateway)[..8])
}
pub fn update(f: impl FnOnce(&mut Profiles) -> Result<()>) -> Result<()> {
    let dir = directory()?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("connections.lock"))?;
    lock.lock_exclusive()?;
    let mut all = load()?;
    f(&mut all)?;
    atomic(
        &dir.join("connections.json"),
        serde_json::to_vec_pretty(&all)?,
    )?;
    Ok(())
}
pub fn scoped_name(gateway: &str, workspace: uuid::Uuid, user: uuid::Uuid) -> String {
    format!("{}-{}-{}", name(gateway), workspace.simple(), user.simple())
}
pub fn save(
    gateway: &str,
    workspace: uuid::Uuid,
    user: uuid::Uuid,
    workspace_name: &str,
    username: &str,
    token: &str,
) -> Result<Profile> {
    let gateway = crate::device_auth::validate_url(gateway)?;
    let key = scoped_name(&gateway, workspace, user);
    let path = directory()?.join(format!("{key}.token"));
    let mut profile = Profile {
        gateway,
        workspace_id: Some(workspace),
        workspace_name: Some(workspace_name.into()),
        user_id: Some(user),
        username: username.into(),
        token_file: path,
        runner_config: None,
    };
    update(|all| {
        if let Some(old) = all.connections.get(&key) {
            profile.runner_config = old.runner_config.clone();
        }
        atomic(&profile.token_file, token)?;
        all.connections.insert(key.clone(), profile.clone());
        all.current = Some(key);
        Ok(())
    })?;
    Ok(profile)
}
fn select(
    all: &Profiles,
    gateway: Option<&str>,
    workspace: Option<uuid::Uuid>,
) -> Result<Option<Profile>> {
    if gateway.is_none() && workspace.is_none() {
        return Ok(all
            .current
            .as_ref()
            .and_then(|k| all.connections.get(k))
            .cloned());
    }
    let matches: Vec<_> = all
        .connections
        .values()
        .filter(|p| {
            gateway.is_none_or(|g| p.gateway == g)
                && workspace.is_none_or(|w| p.workspace_id.is_none_or(|own| own == w))
        })
        .collect();
    ensure!(
        matches.len() <= 1,
        "multiple connections match; use connection list and connection use to choose one"
    );
    Ok(matches.first().map(|p| (*p).clone()))
}
pub fn resolve(gateway: Option<&str>, workspace: Option<uuid::Uuid>) -> Result<Option<Profile>> {
    select(&load()?, gateway, workspace)
}
/// Upgrade only after authenticating with the profile's own Gateway. Never rotate
/// the token, move device files, or infer a scope from another connection.
pub async fn upgrade_legacy(profile: Profile, expected: Option<uuid::Uuid>) -> Result<Profile> {
    if profile.workspace_id.is_some() && profile.user_id.is_some() {
        return Ok(profile);
    }
    let token = std::fs::read_to_string(&profile.token_file)
        .context("legacy connection token is unavailable; login again")?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = http
        .get(format!("{}/api/v1/session", profile.gateway))
        .bearer_auth(token.trim())
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "legacy connection authentication failed; existing files were preserved"
    );
    let identity: serde_json::Value = response.json().await?;
    let workspace: uuid::Uuid = identity["workspace_id"]
        .as_str()
        .context("Gateway has no workspace metadata; update it before migrating this connection")?
        .parse()?;
    ensure!(
        expected.is_none_or(|w| w == workspace),
        "legacy connection belongs to another workspace; login to the requested workspace"
    );
    let user: uuid::Uuid = identity["id"]
        .as_str()
        .context("account ID missing")?
        .parse()?;
    let response = http
        .get(format!("{}/api/v1/workspaces/{workspace}", profile.gateway))
        .bearer_auth(token.trim())
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "legacy workspace is unavailable; existing files were preserved"
    );
    let metadata: serde_json::Value = response.json().await?;
    let mut upgraded = profile.clone();
    upgraded.workspace_id = Some(workspace);
    upgraded.user_id = Some(user);
    upgraded.workspace_name = metadata["name"].as_str().map(str::to_owned);
    let key = scoped_name(&profile.gateway, workspace, user);
    update(|all| {
        ensure!(
            !all.connections.contains_key(&key),
            "a scoped connection already exists; select it explicitly"
        );
        let legacy: Vec<_> = all
            .connections
            .iter()
            .filter(|(_, p)| {
                p.gateway == profile.gateway
                    && p.token_file == profile.token_file
                    && p.workspace_id.is_none()
            })
            .map(|(k, _)| k.clone())
            .collect();
        ensure!(
            legacy.len() == 1,
            "legacy connection changed; retry without overwriting it"
        );
        let old = &legacy[0];
        all.connections.remove(old);
        if all.current.as_ref() == Some(old) {
            all.current = Some(key.clone());
        }
        all.connections.insert(key.clone(), upgraded.clone());
        Ok(())
    })?;
    Ok(upgraded)
}
pub fn use_profile(name: &str) -> Result<()> {
    update(|all| {
        ensure!(
            all.connections.contains_key(name),
            "unknown connection: {name}"
        );
        all.current = Some(name.into());
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_replacement_and_scoped_names() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("key");
        atomic(&p, "one").unwrap();
        atomic(&p, "two").unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "two");
        assert_ne!(name("https://a.example"), name("https://b.example"));
    }
    #[test]
    fn public_url_rules() {
        assert!(crate::device_auth::validate_url("http://example.com").is_err());
        assert!(crate::device_auth::validate_url("http://127.0.0.1:8").is_ok());
        assert!(crate::device_auth::validate_url("https://x.com/a").is_err());
        assert!(crate::device_auth::validate_url("https://u:p@x.com").is_err());
    }
    #[test]
    fn selection_never_guesses_between_accounts_or_workspaces() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let user = uuid::Uuid::new_v4();
        let other = uuid::Uuid::new_v4();
        let gateway = "https://a.example";
        let profile = |workspace, account| Profile {
            workspace_id: Some(workspace),
            workspace_name: None,
            user_id: Some(account),
            gateway: gateway.into(),
            username: "fixture".into(),
            token_file: PathBuf::from(format!("{workspace}-{account}.token")),
            runner_config: None,
        };
        let key = scoped_name(gateway, a, user);
        let mut all = Profiles {
            current: Some(key.clone()),
            ..Default::default()
        };
        all.connections.insert(key, profile(a, user));
        all.connections
            .insert(scoped_name(gateway, b, user), profile(b, user));
        assert_eq!(
            select(&all, None, None).unwrap().unwrap().workspace_id,
            Some(a)
        );
        assert!(select(&all, Some(gateway), None).is_err());
        assert_eq!(
            select(&all, Some(gateway), Some(b))
                .unwrap()
                .unwrap()
                .workspace_id,
            Some(b)
        );
        assert!(
            select(&all, Some("https://b.example"), Some(a))
                .unwrap()
                .is_none()
        );
        all.connections
            .insert(scoped_name(gateway, a, other), profile(a, other));
        assert!(select(&all, Some(gateway), Some(a)).is_err());
        assert_ne!(
            scoped_name(gateway, a, user),
            scoped_name(gateway, a, other)
        );
        assert_ne!(scoped_name(gateway, a, user), scoped_name(gateway, b, user));
    }
}
