mod agent;
mod task;

pub use agent::{
    AgentConfig, AgentMode, CommandArg, CommandSpec, OutputFormat, OutputSpec, ParameterSpec,
    SessionAdapter, render, render_named, resolve_parameters, validate_template,
};
pub use task::{
    Execution, Misfire, Overlap, Schedule, SessionBinding, TaskConfig, generated_session_title,
};

use anyhow::{Context, Result, ensure};
use std::{path::Path, time::Duration};

fn version() -> u32 {
    1
}

pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    yaml_serde::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?,
    )
    .context("invalid YAML")
}
pub fn duration(value: &str) -> Result<Duration> {
    let d = humantime::parse_duration(value)?;
    ensure!(
        d.as_secs_f64() >= 1.0 && d.as_secs() <= 366 * 86400,
        "duration must be between 1s and 366d"
    );
    Ok(d)
}
pub use wakeit_plugin_protocol::validate_id;
