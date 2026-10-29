use crate::config::{AgentConfig, CommandSpec};
use anyhow::Result;
#[cfg(windows)]
use anyhow::{Context, ensure};

/// Convert npm's Windows batch launchers into explicit Node entry points.
/// This is generic package-bin discovery, not an agent-specific execution path.
pub fn normalize_commands(agent: &mut AgentConfig) -> Result<()> {
    let mut session = agent.session.take();
    for spec in [
        Some(&mut agent.submit),
        agent.status.as_mut(),
        agent.cancel.as_mut(),
        agent.detect.as_mut(),
        agent.models.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        if spec.command == "wakeit" {
            spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
        }
    }
    for spec in [
        Some(&mut agent.submit),
        agent.status.as_mut(),
        agent.cancel.as_mut(),
        agent.detect.as_mut(),
        agent.models.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        normalize_native_command(spec)?;
    }
    if let Some(session) = &mut session {
        for spec in [session.list.as_mut(), session.rename.as_mut()]
            .into_iter()
            .flatten()
        {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            normalize_native_command(spec)?;
        }
        for spec in [
            &mut session.inspect,
            &mut session.resume,
            &mut session.interactive,
        ] {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            normalize_native_command(spec)?;
        }
    }
    agent.session = session;
    Ok(())
}
/// Resolve npm launchers to an explicit Node interpreter without invoking a shell.
pub fn normalize_native_command(spec: &mut CommandSpec) -> Result<()> {
    #[cfg(windows)]
    {
        let Ok(executable) = which::which(&spec.command) else {
            return Ok(());
        };
        if !executable
            .extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("cmd"))
        {
            return Ok(());
        }
        let parent = executable.parent().context("launcher directory")?;
        let root = if parent.file_name().is_some_and(|n| n == ".bin") {
            parent
                .parent()
                .context("node_modules directory")?
                .to_owned()
        } else {
            parent.join("node_modules")
        };
        let name = executable
            .file_stem()
            .context("command name")?
            .to_string_lossy();
        let mut packages = Vec::new();
        if root.exists() {
            for entry in std::fs::read_dir(root)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().starts_with('@') {
                    for package in std::fs::read_dir(entry.path())? {
                        packages.push(package?.path());
                    }
                } else {
                    packages.push(entry.path());
                }
            }
        }
        let mut found = None;
        for package in packages {
            let Ok(text) = std::fs::read_to_string(package.join("package.json")) else {
                continue;
            };
            let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let bin = metadata["bin"]
                .as_object()
                .and_then(|bins| {
                    bins.iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(&name))
                        .and_then(|(_, v)| v.as_str())
                })
                .or_else(|| {
                    if metadata["name"]
                        .as_str()
                        .and_then(|v| v.rsplit('/').next())
                        .is_some_and(|v| v.eq_ignore_ascii_case(&name))
                    {
                        metadata["bin"].as_str()
                    } else {
                        None
                    }
                });
            if let Some(bin) = bin {
                let path = package.join(bin);
                let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("");
                let mut prefix = [0u8; 160];
                use std::io::Read;
                let read = std::fs::File::open(&path)?.read(&mut prefix)?;
                let node = matches!(extension, "js" | "mjs" | "cjs")
                    || String::from_utf8_lossy(&prefix[..read])
                        .lines()
                        .next()
                        .is_some_and(|v| v.starts_with("#!") && v.contains("node"));
                if node {
                    found = Some(path);
                    break;
                }
            }
        }
        let path=found.context("npm launcher cannot be resolved; configure node.exe and the script entry point explicitly")?;
        spec.command = "node".into();
        spec.args
            .insert(0, path.to_string_lossy().into_owned().into());
    }
    #[cfg(not(windows))]
    let _ = spec;
    Ok(())
}
