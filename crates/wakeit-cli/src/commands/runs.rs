use super::{args::*, client::Client, display};
use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

pub(super) async fn run(cli: &Cli, command: &Run) -> Result<()> {
    match command {
        Run::Watch { id, timeout } => {
            let c = Client::new(cli)?;
            let code = crate::commands::watch::run_scoped(
                &c.url,
                c.workspace,
                &c.token,
                *id,
                *timeout,
                cli.json,
            )
            .await?;
            std::process::exit(code);
        }
        Run::Resume { id, runner_config } => {
            let code = crate::runner::sessions::resume(*id, runner_config.as_deref()).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
        command => {
            let c = Client::new(cli)?;
            match command {
                Run::Watch { .. } | Run::Resume { .. } => {
                    unreachable!("watch and local continuation have dedicated handlers")
                }
                Run::Get { id } => display(c.get(&format!("/runs/{id}")).await?, true),
                Run::List { task, state } => {
                    let mut url = reqwest::Url::parse(&c.api_url("/runs"))?;
                    if let Some(t) = task {
                        url.query_pairs_mut().append_pair("task", t);
                    }
                    if let Some(s) = state {
                        url.query_pairs_mut().append_pair("state", s);
                    }
                    display(
                        c.get(&format!("/runs?{}", url.query().unwrap_or("")))
                            .await?,
                        cli.json,
                    );
                }
                Run::Cancel { id } => display(
                    c.post(&format!("/runs/{id}/cancel"), json!({})).await?,
                    cli.json,
                ),
                Run::Retry {
                    id,
                    node,
                    resume_stopped,
                } => {
                    let result = if let Some(node) = node {
                        crate::config::validate_id(node)?;
                        let current = c.get(&format!("/runs/{id}/nodes/{node}")).await?;
                        c.post(
                            &format!("/runs/{id}/nodes/{node}/retry"),
                            json!({
                                "execution_id": current["node"]["execution_id"],
                                "resume_stopped": resume_stopped,
                            }),
                        )
                        .await?
                    } else {
                        c.post(&format!("/runs/{id}/retry"), json!({})).await?
                    };
                    display(result, cli.json);
                }
                Run::Resolve { id, state, reason } => display(
                    c.post(
                        &format!("/runs/{id}/resolve"),
                        json!({"state":state,"reason":reason}),
                    )
                    .await?,
                    cli.json,
                ),
                Run::Logs { id, follow } => {
                    let mut cursor = 0;
                    loop {
                        let rows = c.get(&format!("/runs/{id}/events?after={cursor}")).await?;
                        for row in rows.as_array().context("events")? {
                            cursor = row["id"].as_i64().unwrap_or(cursor);
                            if cli.json {
                                println!("{row}");
                            } else if matches!(row["kind"].as_str(), Some("log" | "truncated")) {
                                print!("{}", row["data"]["text"].as_str().unwrap_or(""));
                            } else {
                                println!("{} {}", row["kind"], row["data"]);
                            }
                        }
                        if !follow {
                            break;
                        }
                        let run = c.get(&format!("/runs/{id}")).await?;
                        if run["state"].as_str().is_some_and(crate::protocol::terminal) {
                            let more = c.get(&format!("/runs/{id}/events?after={cursor}")).await?;
                            if more.as_array().is_some_and(Vec::is_empty) {
                                break;
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
    Ok(())
}
