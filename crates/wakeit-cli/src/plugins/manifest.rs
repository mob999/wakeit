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
    pub agents: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub commands: BTreeMap<String, PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub artifacts: BTreeMap<String, Artifact>,
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
    for file in manifest
        .wakeit
        .agents
        .iter()
        .chain(manifest.wakeit.commands.values())
    {
        relative(file)?;
    }
    for runtime in &manifest.wakeit.runtimes {
        ensure!(
            which::which(runtime).is_ok(),
            "plugin {} requires runtime {runtime}",
            manifest.name
        );
    }
    Ok(manifest)
}
pub fn agents(root: &Path, manifest: &Manifest) -> Result<Vec<AgentConfig>> {
    let root = root.canonicalize()?;
    let mut agents = Vec::new();
    for file in &manifest.wakeit.agents {
        let path = root.join(file).canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "plugin agent path escapes its package"
        );
        let mut agent: AgentConfig = crate::config::read(&path)?;
        for command in agent.commands_mut() {
            let expand = |value: &str| value.replace("{{plugin.root}}", &root.to_string_lossy());
            command.command = expand(&command.command);
            if command.command.starts_with("./") || command.command.starts_with(".\\") {
                let relative_path = Path::new(&command.command[2..]);
                relative(relative_path)?;
                command.command = root.join(relative_path).to_string_lossy().into();
            }
            // Development builds run the official helper beside the CLI. Release
            // bundles contain this exact file at the declared package path.
            #[cfg(debug_assertions)]
            if root
                == Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .canonicalize()?
                && command.command
                    == root
                        .join("plugins/official/bin/wakeit-agent-runtime")
                        .to_string_lossy()
            {
                let exe = std::env::current_exe()?;
                let dir = exe.parent().context("executable directory")?;
                let dir = if dir.file_name().is_some_and(|n| n == "deps") {
                    dir.parent().unwrap()
                } else {
                    dir
                };
                command.command = dir
                    .join(if cfg!(windows) {
                        "wakeit-agent-runtime.exe"
                    } else {
                        "wakeit-agent-runtime"
                    })
                    .to_string_lossy()
                    .into();
            }
            if Path::new(&command.command).is_absolute() {
                command.command = executable(PathBuf::from(&command.command))
                    .to_string_lossy()
                    .into_owned();
            }
            if Path::new(&command.command).is_absolute() {
                ensure!(
                    Path::new(&command.command).is_file(),
                    "plugin executable is unavailable: {}",
                    command.command
                );
            }
            for arg in &mut command.args {
                match arg {
                    CommandArg::Literal(value) => *value = expand(value),
                    CommandArg::Group { args, .. } => {
                        for value in args {
                            *value = expand(value);
                        }
                    }
                }
            }
            for value in command.env.values_mut() {
                *value = expand(value);
            }
            if let Some(value) = &mut command.stdin {
                *value = expand(value);
            }
            if let Some(value) = &mut command.cwd {
                *value = expand(value);
            }
        }
        agent.validate()?;
        agents.push(agent);
    }
    Ok(agents)
}

/// Relative native program names may omit Windows' executable suffix.
pub(super) fn executable(mut path: PathBuf) -> PathBuf {
    if cfg!(windows) && !path.is_file() && path.extension().is_none() {
        path.set_extension("exe");
    }
    path
}
