//! Synthetic command-backend input for persistence and legacy compatibility tests.
//! This is not an official Agent definition and never launches a native Agent.
use wakeit_core::config::{AgentConfig, AgentMode, CommandSpec, OutputFormat, OutputSpec};

pub fn legacy_agent() -> AgentConfig {
    let agent = AgentConfig {
        version: 1,
        id: "synthetic-legacy".into(),
        name: "Synthetic legacy Agent".into(),
        logo: None,
        mode: AgentMode::Sync,
        submit: CommandSpec {
            command: "synthetic-legacy-command".into(),
            stdin: Some("{{task.prompt}}".into()),
            ..Default::default()
        },
        status: None,
        cancel: None,
        detect: None,
        models: None,
        session: None,
        parameters: Default::default(),
        output: OutputSpec {
            format: OutputFormat::Events,
            ..Default::default()
        },
        poll_interval: "5s".into(),
    };
    agent.validate().unwrap();
    agent
}
