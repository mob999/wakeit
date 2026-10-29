use super::{args::*, client::Client, display};
use crate::runner;
use crate::runner::config::read_runner;
use crate::runner::service::render as service;
use anyhow::{Result, bail};
use serde_json::json;

pub(super) async fn runner(cli: &Cli, command: &Runner) -> Result<()> {
    match command {
        Runner::Start { config } => {
            let runner_config = read_runner(config)?;
            crate::runner::config::register(config)?;
            runner::start(runner_config).await?;
        }
        Runner::Enroll {
            name,
            token_env,
            token_file,
            dir,
            roots,
        } => {
            let token = if let Some(env) = token_env {
                std::env::var(env)?
            } else if let Some(file) = token_file {
                std::fs::read_to_string(file)?
            } else {
                bail!("provide --token-env or --token-file with a one-time enrollment token")
            };
            let path = runner::enroll(&cli.gateway, name, token.trim(), dir, roots).await?;
            crate::runner::config::register(&path)?;
            println!(
                "Runner enrolled. Start with: wakeit runner start --config {}",
                path.display()
            );
        }
        Runner::List => display(Client::new(cli)?.get("/runners").await?, cli.json),
        Runner::Enrollment => display(
            Client::new(cli)?
                .post("/runners/enrollment", json!({}))
                .await?,
            true,
        ),
        Runner::Revoke { id } => display(
            Client::new(cli)?
                .post(&format!("/runners/{id}/revoke"), json!({}))
                .await?,
            cli.json,
        ),
        Runner::Service {
            config,
            platform,
            output,
        } => {
            let text = service(config, platform.as_deref().unwrap_or(std::env::consts::OS))?;
            if let Some(file) = output {
                std::fs::write(file, text)?;
            } else {
                print!("{text}");
            }
        }
    };
    Ok(())
}
