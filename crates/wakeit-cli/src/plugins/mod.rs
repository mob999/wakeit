//! User-level adapter packages. Packages never execute inside the Gateway.
pub mod manifest;
pub mod source;
mod store;
use crate::config::AgentConfig;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use source::Source;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

pub const OFFICIAL: &str = "git:github.com/mob999/wakeit";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package {
    pub source: Source,
    pub root: PathBuf,
    pub name: String,
    pub version: String,
    pub revision: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub bundled: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub packages: Vec<Package>,
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub agent_sources: BTreeMap<String, String>,
}
pub fn home() -> Result<PathBuf> {
    let path = if let Some(path) = std::env::var_os("WAKEIT_PLUGIN_HOME") {
        PathBuf::from(path)
    } else {
        crate::connection::directory()?.join("plugins")
    };
    Ok(std::env::current_dir()?.join(path))
}
pub fn registry() -> Result<Registry> {
    match std::fs::read(home()?.join("packages.json")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("invalid plugin registry")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(e) => Err(e.into()),
    }
}
fn bundled_root() -> Result<Option<PathBuf>> {
    if let Some(path) = std::env::var_os("WAKEIT_BUNDLED_PLUGIN_DIR") {
        return Ok(Some(PathBuf::from(path).canonicalize()?));
    }
    let exe = std::env::current_exe()?;
    let dir = exe.parent().context("executable directory")?;
    let root = dir.join("wakeit-plugins").join(env!("CARGO_PKG_VERSION"));
    if root.join(manifest::FILE).is_file() {
        return Ok(Some(root));
    }
    #[cfg(debug_assertions)]
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        if root.join(manifest::FILE).is_file() {
            return Ok(Some(root));
        }
    }
    Ok(None)
}
pub fn packages(registry: &Registry, bundled: bool) -> Result<Vec<Package>> {
    let mut packages = registry.packages.clone();
    if bundled
        && !packages.iter().any(|p| p.source.identity() == OFFICIAL)
        && !registry.removed.iter().any(|p| p == OFFICIAL)
        && let Some(root) = bundled_root()?
    {
        let manifest = manifest::read(&root)?;
        packages.push(Package {
            source: Source::parse("https://github.com/mob999/wakeit", Path::new("."))?,
            root,
            name: manifest.name,
            version: manifest.version,
            revision: None,
            pinned: true,
            bundled: true,
        });
    }
    Ok(packages)
}
pub fn configurations(bundled: bool) -> Result<Vec<(AgentConfig, String)>> {
    configurations_in(&registry()?, bundled)
}
fn configurations_in(registry: &Registry, bundled: bool) -> Result<Vec<(AgentConfig, String)>> {
    let mut result = Vec::new();
    let mut owners = HashMap::new();
    for package in packages(registry, bundled)? {
        let source = package.source.identity();
        for agent in manifest::agents(&package.root, &manifest::read(&package.root)?)? {
            if registry
                .agent_sources
                .get(&agent.id)
                .is_some_and(|s| s != &source)
            {
                continue;
            }
            if let Some(old) = owners.insert(agent.id.clone(), source.clone()) {
                anyhow::bail!(
                    "agent {} is provided by both {old} and {source}; choose explicitly with plugin install SOURCE --replace-agent {}",
                    agent.id,
                    agent.id
                );
            }
            result.push((agent, source.clone()));
        }
    }
    Ok(result)
}
/// Merge explicit YAML and enabled plugin resources, preserving the task's Agent ID.
pub fn load_agents(files: &[PathBuf], bundled: bool) -> Result<HashMap<String, AgentConfig>> {
    let registry = registry()?;
    let mut agents = HashMap::new();
    let mut sources = HashMap::new();
    for (mut agent, source) in configurations_in(&registry, bundled)? {
        crate::launch::normalize_commands(&mut agent)?;
        sources.insert(agent.id.clone(), source);
        agents.insert(agent.id.clone(), agent);
    }
    for file in files {
        let mut agent: AgentConfig = crate::config::read(file)?;
        let source = format!("yaml:{}", file.display());
        if registry
            .agent_sources
            .get(&agent.id)
            .is_some_and(|s| s != &source)
        {
            continue;
        }
        crate::launch::normalize_commands(&mut agent)?;
        agent.validate()?;
        ensure!(
            !agents.contains_key(&agent.id),
            "duplicate agent {} from {} and {source}",
            agent.id,
            sources.get(&agent.id).unwrap_or(&source)
        );
        sources.insert(agent.id.clone(), source);
        agents.insert(agent.id.clone(), agent);
    }
    Ok(agents)
}
pub fn agent(id: &str) -> Result<AgentConfig> {
    configurations(true)?
        .into_iter()
        .find_map(|(agent, _)| (agent.id == id).then_some(agent))
        .with_context(|| format!("agent {id} is not provided by an enabled plugin"))
}
/// Run one explicitly declared package command without changing the caller's cwd.
pub fn execute(source: &str, command: &str, args: &[String]) -> Result<()> {
    let identity = Source::parse(source, &std::env::current_dir()?)?.identity();
    let package = packages(&registry()?, true)?
        .into_iter()
        .find(|p| p.source.identity() == identity)
        .context("plugin source is not installed")?;
    let manifest = manifest::read(&package.root)?;
    let relative = manifest
        .wakeit
        .commands
        .get(command)
        .context("plugin command is not declared")?;
    let mut executable = manifest::executable(package.root.join(relative));
    #[cfg(debug_assertions)]
    if identity == OFFICIAL && !executable.is_file() {
        executable = std::env::current_exe()?
            .parent()
            .context("executable directory")?
            .join(if cfg!(windows) {
                "wakeit-agent-runtime.exe"
            } else {
                "wakeit-agent-runtime"
            });
    }
    ensure!(
        executable.is_file(),
        "plugin executable is unavailable; source builds need cargo build -p wakeit-agent-runtime"
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(std::process::Command::new(executable)
            .args(args)
            .exec()
            .into())
    }
    #[cfg(not(unix))]
    {
        let status = std::process::Command::new(executable).args(args).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
pub async fn install(source: &str, replace: &[String]) -> Result<serde_json::Value> {
    store::install(source, replace).await
}
pub async fn update(source: Option<&str>) -> Result<serde_json::Value> {
    store::update(source).await
}
pub fn remove(source: &str) -> Result<serde_json::Value> {
    store::remove(source)
}
pub fn list() -> Result<serde_json::Value> {
    let registry = registry()?;
    let packages = packages(&registry, true)?;
    let rows = packages
        .into_iter()
        .map(|p| -> Result<_> {
            let info = manifest::read(&p.root)?;
            let agents = manifest::agents(&p.root, &info)?
                .into_iter()
                .map(|a| a.id)
                .collect::<Vec<_>>();
            let identity = p.source.identity();
            let enabled = agents
                .iter()
                .filter(|id| {
                    registry
                        .agent_sources
                        .get(*id)
                        .is_none_or(|s| s == &identity)
                })
                .collect::<Vec<_>>();
            Ok(serde_json::json!({
                "source":p.source.display(), "name":info.name, "version":info.version,
                "revision":p.revision, "pinned":p.pinned, "bundled":p.bundled,
                "enabled_agents":enabled, "agents":agents, "root":p.root
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(
        serde_json::json!({"packages":rows,"removed":registry.removed,"agent_sources":registry.agent_sources,"runners":runner_status()?}),
    )
}
fn fingerprints(bundled: bool) -> Result<BTreeMap<String, String>> {
    packages(&registry()?, bundled)?
        .into_iter()
        .map(|p| {
            let manifest = manifest::read(&p.root)?;
            let config = manifest::agents(&p.root, &manifest)?
                .into_iter()
                .map(|a| a.digest())
                .collect::<Vec<_>>();
            Ok((
                p.source.identity(),
                format!(
                    "{}:{}:{}:{config:?}",
                    p.root.display(),
                    manifest.version,
                    p.revision.as_deref().unwrap_or("local")
                ),
            ))
        })
        .collect()
}
pub fn record_loaded(config: &crate::runner::config::RunnerConfig) -> Result<()> {
    crate::connection::atomic(
        &config.data_dir.join("plugins-loaded.json"),
        serde_json::to_vec(
            &serde_json::json!({"packages":fingerprints(config.bundled_agents)?,"agent_sources":registry()?.agent_sources}),
        )?,
    )
}
fn runner_status() -> Result<Vec<serde_json::Value>> {
    let mut rows = Vec::new();
    for path in crate::runner::config::registered_configs()? {
        let Ok(config) = crate::runner::config::read_runner(&path) else {
            continue;
        };
        let desired = serde_json::json!({"packages":fingerprints(config.bundled_agents)?,"agent_sources":registry()?.agent_sources});
        let loaded = std::fs::read(config.data_dir.join("plugins-loaded.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let running = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(config.data_dir.join("runner.lock"))
            .ok()
            .is_some_and(|f| fs2::FileExt::try_lock_exclusive(&f).is_err());
        rows.push(serde_json::json!({"name":config.name,"running":running,"loaded_sources":loaded.as_ref().and_then(|v|v["packages"].as_object()).map(|p|p.keys().collect::<Vec<_>>()),"restart_required":running && loaded.as_ref()!=Some(&desired)}));
    }
    Ok(rows)
}

pub(crate) use store::unpack as unpack_bundle;
