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
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentStamp {
    source: String,
    root: Option<PathBuf>,
    version: Option<String>,
    revision: Option<String>,
    digest: String,
}
pub type Snapshot = BTreeMap<String, AgentStamp>;
pub struct LoadedAgents {
    pub agents: HashMap<String, AgentConfig>,
    pub snapshot: Snapshot,
}
struct Selected {
    agent: AgentConfig,
    stamp: AgentStamp,
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
fn bundled_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("WAKEIT_BUNDLED_PLUGIN_DIR") {
        return Ok(std::env::current_dir()?.join(path));
    }
    Ok(std::env::current_exe()?
        .parent()
        .context("executable directory")?
        .join("wakeit-plugins")
        .join(env!("CARGO_PKG_VERSION")))
}
pub fn packages(registry: &Registry, bundled: bool) -> Result<Vec<Package>> {
    let mut packages = registry.packages.clone();
    if bundled
        && !packages.iter().any(|p| p.source.identity() == OFFICIAL)
        && !registry.removed.iter().any(|s| s == OFFICIAL)
    {
        let root = bundled_root()?;
        let info = manifest::read(&root).ok();
        packages.push(Package {
            source: Source::parse("https://github.com/mob999/wakeit", Path::new("."))?,
            root,
            name: info
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| OFFICIAL.into()),
            version: info.map(|m| m.version).unwrap_or_else(|| "unknown".into()),
            revision: None,
            pinned: true,
            bundled: true,
        });
    }
    Ok(packages)
}
/// Resolve source selection before opening any package's Agent YAML.
fn selected(registry: &Registry, files: &[PathBuf], bundled: bool) -> Result<Vec<Selected>> {
    let mut result = Vec::new();
    let mut owners = HashMap::new();
    for package in packages(registry, bundled)? {
        let source = package.source.identity();
        let info = manifest::read(&package.root)?;
        for (id, path) in &info.wakeit.agents {
            if registry.agent_sources.get(id).is_some_and(|s| s != &source) {
                continue;
            }
            let agent = manifest::definition(&package.root, id, path)?;
            if let Some(old) = owners.insert(id.clone(), source.clone()) {
                anyhow::bail!(
                    "agent {id} is provided by both {old} and {source}; choose explicitly with plugin install SOURCE --replace-agent {id}"
                );
            }
            let stamp = AgentStamp {
                source: source.clone(),
                root: Some(package.root.clone()),
                version: Some(info.version.clone()),
                revision: package.revision.clone(),
                digest: agent.digest(),
            };
            result.push(Selected { agent, stamp });
        }
    }
    for path in files {
        let agent: AgentConfig = crate::config::read(path)?;
        let source = format!("yaml:{}", path.display());
        if registry
            .agent_sources
            .get(&agent.id)
            .is_some_and(|s| s != &source)
        {
            continue;
        }
        agent.validate()?;
        if let Some(old) = owners.insert(agent.id.clone(), source.clone()) {
            anyhow::bail!("duplicate agent {} from {old} and {source}", agent.id);
        }
        let stamp = AgentStamp {
            source,
            root: None,
            version: None,
            revision: None,
            digest: agent.digest(),
        };
        result.push(Selected { agent, stamp });
    }
    for (id, source) in &registry.agent_sources {
        ensure!(
            result
                .iter()
                .any(|item| &item.agent.id == id && &item.stamp.source == source),
            "selected agent {id} is no longer provided by {source}; choose another source before updating"
        );
    }
    Ok(result)
}
fn validate_selection(registry: &Registry) -> Result<()> {
    selected(registry, &[], true).map(|_| ())
}
/// The returned snapshot is produced from exactly the definitions used by this load.
pub fn load_agents(files: &[PathBuf], bundled: bool) -> Result<LoadedAgents> {
    let mut agents = HashMap::new();
    let mut snapshot = Snapshot::new();
    for Selected { mut agent, stamp } in selected(&registry()?, files, bundled)? {
        if let Some(root) = &stamp.root {
            manifest::resolve(root, &mut agent)?;
        } else {
            crate::launch::normalize_commands(&mut agent)?;
        }
        snapshot.insert(agent.id.clone(), stamp);
        agents.insert(agent.id.clone(), agent);
    }
    Ok(LoadedAgents { agents, snapshot })
}
pub fn agent(id: &str) -> Result<AgentConfig> {
    let registry = registry()?;
    let mut found = None;
    for package in packages(&registry, true)? {
        let source = package.source.identity();
        if registry.agent_sources.get(id).is_some_and(|s| s != &source) {
            continue;
        }
        let info = manifest::read(&package.root)?;
        if let Some(path) = info.wakeit.agents.get(id) {
            ensure!(
                found.is_none(),
                "agent {id} has multiple sources; select one with --replace-agent"
            );
            let mut agent = manifest::definition(&package.root, id, path)?;
            manifest::resolve(&package.root, &mut agent)?;
            found = Some(agent);
        }
    }
    found.with_context(|| format!("agent {id} is not provided by an enabled plugin"))
}
pub fn execute(source: &str, command: &str, args: &[String]) -> Result<()> {
    let identity = Source::parse(source, &std::env::current_dir()?)?.identity();
    let package = packages(&registry()?, true)?
        .into_iter()
        .find(|p| p.source.identity() == identity)
        .context("plugin source is not installed")?;
    let info = manifest::read(&package.root)?;
    let path = info
        .wakeit
        .commands
        .get(command)
        .context("plugin command is not declared")?;
    let executable = manifest::executable(package.root.join(path));
    ensure!(
        executable.is_file(),
        "plugin command is missing: {}",
        executable.display()
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
    let rows=packages(&registry,true)?.into_iter().map(|p| {
        let identity=p.source.identity();
        let mut row=serde_json::json!({"source":p.source.display(),"name":p.name,"version":p.version,"revision":p.revision,"pinned":p.pinned,"bundled":p.bundled,"root":p.root});
        match manifest::read(&p.root) {
            Ok(info)=>{
                let agents=info.wakeit.agents.keys().collect::<Vec<_>>();
                row["name"]=info.name.into();row["version"]=info.version.into();
                row["enabled_agents"]=serde_json::json!(agents.iter().filter(|id|registry.agent_sources.get(**id).is_none_or(|s|s==&identity)).collect::<Vec<_>>());
                row["agents"]=serde_json::json!(agents);
                let mut errors=Vec::new();
                for path in info.wakeit.agents.values().chain(info.wakeit.commands.values()) {
                    if !manifest::executable(p.root.join(path)).is_file() {errors.push(format!("missing resource: {}",path.display()));}
                }
                for runtime in info.wakeit.runtimes {if which::which(&runtime).is_err() {errors.push(format!("missing runtime: {runtime}"));}}
                row["status"]=if errors.is_empty(){"installed".into()}else{"unavailable".into()};row["errors"]=serde_json::json!(errors);
            },
            Err(error)=>{row["status"]="unavailable".into();row["errors"]=serde_json::json!([error.to_string()]);},
        }
        row
    }).collect::<Vec<_>>();
    Ok(
        serde_json::json!({"packages":rows,"removed":registry.removed,"agent_sources":registry.agent_sources,"runners":runner_status(&registry)?}),
    )
}
pub fn record_loaded(
    config: &crate::runner::config::RunnerConfig,
    snapshot: &Snapshot,
) -> Result<()> {
    crate::connection::atomic(
        &config.data_dir.join("plugins-loaded.json"),
        serde_json::to_vec(snapshot)?,
    )
}
fn runner_status(registry: &Registry) -> Result<Vec<serde_json::Value>> {
    let mut rows = Vec::new();
    for path in crate::runner::config::registered_configs()? {
        let Ok(config) = crate::runner::config::read_runner(&path) else {
            continue;
        };
        let desired = selected(registry, &config.agents, config.bundled_agents).map(|entries| {
            entries
                .into_iter()
                .map(|s| (s.agent.id, s.stamp))
                .collect::<Snapshot>()
        });
        let loaded = std::fs::read(config.data_dir.join("plugins-loaded.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Snapshot>(&bytes).ok());
        let running = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(config.data_dir.join("runner.lock"))
            .ok()
            .is_some_and(|file| fs2::FileExt::try_lock_exclusive(&file).is_err());
        let error = desired.as_ref().err().map(ToString::to_string);
        let restart = running && (error.is_some() || loaded.as_ref() != desired.as_ref().ok());
        rows.push(serde_json::json!({"name":config.name,"running":running,"restart_required":restart,"error":error}));
    }
    Ok(rows)
}
pub(crate) use store::unpack as unpack_bundle;
