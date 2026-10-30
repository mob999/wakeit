use crate::config::AgentConfig;
pub use wakeit_core::native_command::normalize_native_command;
pub fn normalize_commands(agent: &mut AgentConfig) -> anyhow::Result<()> {
    for command in agent.commands_mut() {
        normalize_native_command(command)?;
    }
    Ok(())
}
