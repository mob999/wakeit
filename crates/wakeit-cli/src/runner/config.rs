use crate::config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

pub(super) fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub(super) fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
fn registry_directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .context("APPDATA is unavailable")?;
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|p| PathBuf::from(p).join("Library/Application Support"))
        .context("user home is unavailable")?;
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .context("user configuration directory is unavailable")?;
    Ok(base.join("wakeit"))
}
pub fn registered_configs() -> Result<Vec<PathBuf>> {
    registered_configs_at(&registry_directory()?)
}
pub(super) fn registered_configs_at(directory: &Path) -> Result<Vec<PathBuf>> {
    let path = directory.join("runners.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("invalid local Runner registry"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
pub fn register(path: &Path) -> Result<()> {
    register_at(path, &registry_directory()?)
}
pub(super) fn register_at(path: &Path, directory: &Path) -> Result<()> {
    let path = path
        .canonicalize()
        .context("Runner configuration is unavailable")?;
    private_directory(directory)?;
    let guard = private_file(&directory.join("registry.lock"))?;
    fs2::FileExt::lock_exclusive(&guard)?;
    let mut configs = registered_configs_at(directory)?;
    if !configs.contains(&path) {
        configs.push(path);
        use std::io::Write;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(serde_json::to_string(&configs)?.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(directory.join("runners.json"))?;
    }
    Ok(())
}
pub fn read_runner(path: &Path) -> Result<RunnerConfig> {
    let path = path.canonicalize()?;
    let mut config: RunnerConfig = config::read(&path)?;
    let base = path
        .parent()
        .context("Runner configuration directory is unavailable")?;
    for file in [
        &mut config.token_file,
        &mut config.credentials_file,
        &mut config.data_dir,
    ]
    .into_iter()
    .chain(config.agents.iter_mut())
    .chain(config.allowed_roots.iter_mut())
    {
        if file.is_relative() {
            *file = base.join(&*file);
        }
    }
    Ok(config)
}

fn version() -> u32 {
    1
}
fn concurrency() -> usize {
    2
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
