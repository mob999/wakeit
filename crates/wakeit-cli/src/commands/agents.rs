use super::{args::*, client::Client, display};
use crate::runner::config::read_runner;
use crate::{
    config::{self, AgentConfig, TaskConfig},
    process, runner,
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

pub(super) async fn agent(cli: &Cli, command: &Agent) -> Result<()> {
    match command {
        Agent::Init { id, file } => {
            let file = file
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("{id}.yaml")));
            ensure!(!file.exists(), "file already exists");
            std::fs::write(&file, yaml_serde::to_string(&crate::plugins::agent(id)?)?)?;
            println!("{}", file.display());
        }
        Agent::Validate { file } => {
            let agent: AgentConfig = config::read(file)?;
            agent.validate()?;
            display(
                json!({"valid":true,"id":agent.id,"digest":agent.digest()}),
                cli.json,
            );
        }
        Agent::Doctor { config } => {
            let c = read_runner(config)?;
            let mut items = vec![];
            for a in crate::plugins::load_agents(&c.agents, c.bundled_agents)?
                .agents
                .values()
            {
                items.push(runner::inspect(a, &std::env::current_dir()?).await);
            }
            display(json!(items), cli.json);
        }
        Agent::Test { file, cwd, prompt } => {
            let mut a: AgentConfig = config::read(file)?;
            crate::launch::normalize_commands(&mut a)?;
            a.validate()?;
            let mut t = TaskConfig {
                dag: None,
                version: 1,
                id: "agent-test".into(),
                name: "Agent test".into(),
                runner: "local".into(),
                agent: a.id.clone(),
                cwd: cwd.canonicalize()?.to_string_lossy().into(),
                prompt: prompt.clone(),
                parameters: Default::default(),
                session: None,
                schedule: None,
                enabled: false,
                execution: Default::default(),
            };
            t.parameters = config::resolve_parameters(&a.parameters, &t.parameters)?;
            let id = Uuid::new_v4();
            let session_title =
                config::generated_session_title(&t.name, chrono::Utc::now(), "UTC")?;
            let out = process::execute_output_named_format(
                &a.submit,
                &t,
                id,
                None,
                Duration::from_secs(120),
                tokio_util::sync::CancellationToken::new(),
                None,
                if a.mode == config::AgentMode::Sync {
                    a.output.format.clone()
                } else {
                    config::OutputFormat::Text
                },
                Some(&session_title),
            )
            .await?;
            ensure!(
                out.code == Some(0) && out.stopped.is_none() && out.confirmed,
                "agent submit failed (exit {:?}, stopped {:?}, confirmed {}): {}{}",
                out.code,
                out.stopped,
                out.confirmed,
                out.stderr,
                out.stdout
            );
            if a.mode == config::AgentMode::Sync {
                display(
                    json!({"state":"succeeded","result":out.result_value(&a.output)?}),
                    cli.json,
                );
            } else {
                let b: Value = serde_json::from_str(&out.stdout)?;
                let job = b["job_id"].as_str().context("submit must return job_id")?;
                let status = process::execute(
                    a.status.as_ref().context("status")?,
                    &t,
                    id,
                    Some(job),
                    Duration::from_secs(10),
                    tokio_util::sync::CancellationToken::new(),
                    None,
                )
                .await?;
                ensure!(status.code == Some(0), "status command failed");
                display(
                    json!({"job_id":job,"status":serde_json::from_str::<Value>(&status.stdout)?}),
                    true,
                );
            }
        }
        Agent::List => display(Client::new(cli)?.get("/agents").await?, cli.json),
    };
    Ok(())
}
