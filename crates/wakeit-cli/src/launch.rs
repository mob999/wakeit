use crate::config::AgentConfig;
use anyhow::Result;

pub fn normalize_commands(agent: &mut AgentConfig) -> Result<()> {
    for spec in agent.commands_mut() {
        if spec.command == "wakeit" {
            spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
        }
        wakeit_core::native_command::normalize_native_command(spec)?;
    }
    Ok(())
}
