//! Gateway-scoped profiles. Tokens are separate private files, never part of listing output.
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
    format!("{}-{}", host, &crate::auth::hash(gateway)[..8])
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
pub fn save(gateway: &str, username: &str, token: &str) -> Result<Profile> {
    let key = name(gateway);
    let path = directory()?.join(format!("{key}.token"));
    atomic(&path, token)?;
    let mut profile = Profile {
        gateway: gateway.into(),
        username: username.into(),
        token_file: path,
        runner_config: None,
    };
    update(|all| {
        if let Some(old) = all.connections.get(&key) {
            profile.runner_config = old.runner_config.clone();
        }
        all.connections.insert(key.clone(), profile.clone());
        all.current = Some(key);
        Ok(())
    })?;
    Ok(profile)
}
pub fn resolve(gateway: Option<&str>) -> Result<Option<Profile>> {
    let all = load()?;
    Ok(match gateway {
        Some(g) => all.connections.values().find(|p| p.gateway == g),
        None => all.current.as_ref().and_then(|k| all.connections.get(k)),
    }
    .cloned())
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
}
