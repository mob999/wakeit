mod accounts;
mod agents;
mod args;
mod client;
mod runners;
mod runs;
mod tasks;
mod watch;
use crate::config::{AgentConfig, TaskConfig};
use anyhow::{Context, Result, bail, ensure};
use args::*;
use clap::Parser;
use client::Client;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub fn entry() {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wakeit=info,tower_http=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    let json_output = cli.json;
    let local_install = matches!(
        &cli.command,
        Command::Update { .. } | Command::Skill { .. } | Command::Plugin { .. }
    );
    let watch_id = match &cli.command {
        Command::Run {
            command: Run::Watch { id, .. },
        } => Some(*id),
        _ => None,
    };
    if let Err(e) = runtime.block_on(run(cli)) {
        if let Some(id) = watch_id {
            let _ = crate::commands::watch::setup_failure(id, json_output, &e.to_string());
            std::process::exit(2);
        }
        if json_output && local_install {
            println!("{}", json!({"status":"error", "error":e.to_string()}));
        } else {
            eprintln!("wakeit: {e:#}");
        }
        std::process::exit(1);
    }
}
async fn run(mut cli: Cli) -> Result<()> {
    let explicit_gateway = !cli.gateway.is_empty();
    let explicit_token_file = cli.token_file != Path::new("__wakeit_profile__");
    if explicit_gateway {
        cli.gateway = crate::device_auth::validate_url(&cli.gateway)?;
    }
    let local_only = matches!(
        &cli.command,
        Command::Plugin { .. }
            | Command::Template {
                command: Template::Validate { .. }
            }
            | Command::Update { .. }
            | Command::Skill { .. }
            | Command::Connection { .. }
            | Command::Run {
                command: Run::Resume { .. }
            }
    ) || matches!(&cli.command, Command::Schema { kind } if kind != "openapi")
        || matches!(
            &cli.command,
            Command::Runner {
                command: Runner::Start { .. } | Runner::Enroll { .. } | Runner::Service { .. }
            }
        )
        || matches!(&cli.command, Command::Agent { command } if !matches!(command, Agent::List));
    let current = if local_only {
        None
    } else {
        crate::connection::resolve(None, None)?
    };
    if !explicit_gateway {
        cli.gateway = current
            .as_ref()
            .map(|p| p.gateway.clone())
            .unwrap_or_else(|| "http://127.0.0.1:8080".into());
    }
    let mut profile = if local_only
        || matches!(&cli.command, Command::Login { .. })
        || cli.token.is_some()
        || explicit_token_file
    {
        None
    } else if current.as_ref().is_some_and(|p| {
        p.gateway == cli.gateway && cli.workspace.is_none_or(|w| p.workspace_id == Some(w))
    }) {
        current.clone()
    } else {
        crate::connection::resolve(Some(&cli.gateway), cli.workspace)?
    };
    if !local_only
        && !matches!(&cli.command, Command::Login { .. })
        && let Some(old) = profile.take()
    {
        profile = Some(crate::connection::upgrade_legacy(old, cli.workspace).await?);
    }
    if cli.workspace.is_none() {
        cli.workspace = profile
            .as_ref()
            .or_else(|| current.as_ref().filter(|p| p.gateway == cli.gateway))
            .and_then(|p| p.workspace_id);
    }
    if !explicit_token_file {
        cli.token_file = profile
            .as_ref()
            .map(|p| p.token_file.clone())
            .unwrap_or_else(|| PathBuf::from(".wakeit/client.token"));
        if explicit_gateway && profile.is_none() && cli.token.is_none() {
            cli.token_file = crate::connection::directory()?.join("not-authenticated");
        }
    }
    if !local_only
        && cli.workspace.is_none()
        && !matches!(&cli.command, Command::Login { .. } | Command::Setup { .. })
    {
        let token = cli
            .token
            .clone()
            .or_else(|| std::fs::read_to_string(&cli.token_file).ok())
            .context("login first, or provide --workspace and credentials")?;
        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?
            .get(format!("{}/api/v1/session", cli.gateway))
            .bearer_auth(token.trim())
            .send()
            .await?;
        ensure!(
            response.status().is_success(),
            "could not determine the credential's workspace"
        );
        let metadata: Value = response.json().await?;
        cli.workspace = Some(
            metadata["workspace_id"]
                .as_str()
                .context("choose a workspace with --workspace")?
                .parse()?,
        );
    }
    match &cli.command {
        Command::Setup { runner_name, roots } => {
            crate::setup::run(
                &cli.gateway,
                cli.workspace
                    .context("setup requires --workspace or a selected workspace connection")?,
                runner_name.as_deref(),
                roots,
            )
            .await?
        }
        Command::Update { check } => crate::update::run(*check, cli.json).await?,
        Command::Skill {
            command: Skill::Install { dir },
        } => crate::skill::install(dir.as_deref(), cli.json).await?,
        Command::Connection { command } => accounts::connection(command).await?,
        Command::Runner { command } => runners::runner(&cli, command).await?,
        Command::Agent { command } => agents::agent(&cli, command).await?,
        Command::Task { command } => tasks::task(&cli, command).await?,
        Command::Template { command } => tasks::template(&cli, command).await?,
        Command::Run { command } => runs::run(&cli, command).await?,
        Command::Login {
            username,
            password_env,
        } => accounts::login(&cli, username, password_env, explicit_token_file).await?,
        Command::Members { command } => accounts::members(&cli, command).await?,
        Command::Plugin { command } => {
            let value = match command {
                Plugin::Install { source, replace } => {
                    crate::plugins::install(source, replace).await?
                }
                Plugin::List => crate::plugins::list()?,
                Plugin::Update { source } => crate::plugins::update(source.as_deref()).await?,
                Plugin::Remove { source } => crate::plugins::remove(source)?,
                Plugin::Exec {
                    source,
                    command,
                    args,
                } => {
                    crate::plugins::execute(source, command, args)?;
                    return Ok(());
                }
            };
            display(value, cli.json);
        }
        Command::Schema { kind } => {
            let value = match kind.as_str() {
                "plugin" => {
                    serde_json::to_value(schemars::schema_for!(crate::plugins::manifest::Manifest))?
                }
                "agent" => serde_json::to_value(schemars::schema_for!(AgentConfig))?,
                "task" => serde_json::to_value(schemars::schema_for!(TaskConfig))?,
                "template" => {
                    serde_json::to_value(schemars::schema_for!(crate::task_templates::Definition))?
                }
                "stream" => {
                    serde_json::to_value(schemars::schema_for!(crate::trace::StreamRecord))?
                }
                "openapi" => {
                    let client = Client::new(&cli)?;
                    client
                        .http
                        .get(format!("{}/openapi.json", client.url))
                        .bearer_auth(&client.token)
                        .send()
                        .await?
                        .error_for_status()?
                        .json()
                        .await?
                }
                _ => bail!("kind must be agent, plugin, task, template, stream or openapi"),
            };
            display(value, true)
        }
    }
    Ok(())
}
fn display(value: Value, json: bool) {
    if !json && let Some(rows) = value.as_array() {
        for row in rows {
            let id = row["id"].as_str().unwrap_or("-");
            let name = row["name"]
                .as_str()
                .or_else(|| row["config"]["name"].as_str())
                .or_else(|| row["username"].as_str())
                .unwrap_or("");
            let state = row["state"]
                .as_str()
                .or_else(|| row["role"].as_str())
                .unwrap_or("");
            println!("{id:<38} {name:<28} {state}");
        }
        return;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("JSON serializes")
    );
}
