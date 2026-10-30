use crate::config::{AgentConfig, CommandArg};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub const FILE: &str = "wakeit-plugin.json";
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub manifest_version: u32,
    pub name: String,
    pub version: String,
    pub wakeit: Resources,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resources {
    pub protocol: u32,
    /// Agent IDs are declared here; package YAML need not repeat them.
    pub agents: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifacts: BTreeMap<String, Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtimes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub checksums_url: Option<String>,
}
pub fn relative(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().is_empty()
            && path
                .components()
                .all(|p| matches!(p, Component::Normal(_) | Component::CurDir)),
        "plugin resource path must stay inside the package: {}",
        path.display()
    );
    Ok(())
}
/// Declaration reading never depends on installed interpreters or Agent programs.
pub fn read(root: &Path) -> Result<Manifest> {
    let manifest: Manifest = serde_json::from_slice(
        &std::fs::read(root.join(FILE)).context("plugin needs wakeit-plugin.json at its root")?,
    )?;
    ensure!(
        manifest.manifest_version == 1 && manifest.wakeit.protocol == 1,
        "unsupported plugin manifest or adapter protocol"
    );
    ensure!(
        !manifest.name.trim().is_empty() && !manifest.wakeit.agents.is_empty(),
        "plugin name and agents are required"
    );
    semver::Version::parse(&manifest.version).context("plugin version must be semver")?;
    for id in manifest.wakeit.agents.keys() {
        crate::config::validate_id(id)?;
    }
    for path in manifest
        .wakeit
        .agents
        .values()
        .chain(manifest.wakeit.commands.values())
    {
        relative(path)?;
    }
    Ok(manifest)
}
/// Parse one selected resource. Its digest describes the same data that is loaded.
pub fn definition(root: &Path, id: &str, path: &Path) -> Result<AgentConfig> {
    let root = root.canonicalize()?;
    let path = root.join(path).canonicalize()?;
    ensure!(
        path.starts_with(&root),
        "plugin agent path escapes its package"
    );
    let mut value: serde_json::Value = yaml_serde::from_slice(&std::fs::read(path)?)?;
    ensure!(
        value.get("id").is_none_or(|v| v.as_str() == Some(id)),
        "Agent YAML id does not match the manifest: {id}"
    );
    value["id"] = id.into();
    let agent: AgentConfig = serde_json::from_value(value)?;
    let mut agent = agent;
    expand_paths(&root, &mut agent)?;
    agent.validate()?;
    Ok(agent)
}
/// Resolve package files while keeping external Agent availability in detect/inspect.
fn expand_paths(root: &Path, agent: &mut AgentConfig) -> Result<()> {
    let root = root.canonicalize()?;
    let root_text = wakeit_core::native_command::external_path(&root);
    for command in agent.commands_mut() {
        let expand = |v: &str| v.replace("{{plugin.root}}", &root_text);
        command.command = expand(&command.command);
        if command.command.starts_with("./") || command.command.starts_with(".\\") {
            relative(Path::new(&command.command[2..]))?;
            command.command = wakeit_core::native_command::external_path(&executable(
                root.join(&command.command[2..]),
            ));
        }

        for arg in &mut command.args {
            match arg {
                CommandArg::Literal(v) => *v = expand(v),
                CommandArg::Group { args, .. } => {
                    for v in args {
                        *v = expand(v)
                    }
                }
            }
        }
        for value in command.env.values_mut() {
            *value = expand(value)
        }
        if let Some(value) = &mut command.stdin {
            *value = expand(value)
        }
        if let Some(value) = &mut command.cwd {
            *value = expand(value)
        }
    }
    Ok(())
}
pub fn resolve(root: &Path, agent: &mut AgentConfig) -> Result<()> {
    let root = root.canonicalize()?;
    let root = PathBuf::from(wakeit_core::native_command::external_path(&root));
    for command in agent.commands_mut() {
        if Path::new(&command.command).starts_with(&root) {
            ensure!(
                Path::new(&command.command).is_file(),
                "plugin executable is unavailable: {}",
                command.command
            );
        }
    }
    crate::launch::normalize_commands(agent)
}
pub fn validate(root: &Path) -> Result<Manifest> {
    let manifest = read(root)?;
    for runtime in &manifest.wakeit.runtimes {
        ensure!(
            which::which(runtime).is_ok(),
            "plugin {} requires runtime {runtime}",
            manifest.name
        );
    }
    for path in manifest.wakeit.commands.values() {
        ensure!(
            executable(root.join(path)).is_file(),
            "plugin command is missing: {}",
            path.display()
        );
    }
    for (id, path) in &manifest.wakeit.agents {
        let mut agent = definition(root, id, path)?;
        resolve(root, &mut agent)?;
    }
    Ok(manifest)
}
pub(super) fn executable(mut path: PathBuf) -> PathBuf {
    if cfg!(windows) && !path.is_file() && path.extension().is_none() {
        path.set_extension("exe");
    }
    path
}
