use crate::config::AgentConfig;
use anyhow::Result;

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
        wakeit_core::native_command::normalize_native_command(spec)?;
    }
    if let Some(session) = &mut session {
        for spec in [session.list.as_mut(), session.rename.as_mut()]
            .into_iter()
            .flatten()
        {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            wakeit_core::native_command::normalize_native_command(spec)?;
        }
        for spec in [
            &mut session.inspect,
            &mut session.resume,
            &mut session.interactive,
        ] {
            if spec.command == "wakeit" {
                spec.command = std::env::current_exe()?.to_string_lossy().into_owned();
            }
            wakeit_core::native_command::normalize_native_command(spec)?;
        }
    }
    agent.session = session;
    Ok(())
}
