use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Adapter,
}
#[derive(Subcommand)]
enum Adapter {
    /// Serve the unified plugin SDK protocol (Agent registration and invocation).
    Sdk,
    /// Execute an official Agent adapter. Native arguments remain separate argv values.
    Run {
        id: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    Detect {
        id: String,
    },
    Models {
        id: String,
    },
    Session {
        #[command(subcommand)]
        command: AdapterSession,
    },
}
#[derive(Subcommand)]
enum AdapterSession {
    Rename {
        protocol: String,
        id: String,
        #[arg(allow_hyphen_values = true)]
        title: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
    List {
        protocol: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        query: Option<String>,
    },
    Inspect {
        protocol: String,
        id: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
    },
}
#[tokio::main]
async fn main() {
    if let Err(error) = run(&Cli::parse().command).await {
        eprintln!("wakeit-agent-runtime: {error:#}");
        std::process::exit(1);
    }
}
async fn run(command: &Adapter) -> anyhow::Result<()> {
    match command {
        Adapter::Sdk => wakeit_agent_runtime::official::serve().await?,
        Adapter::Run {
            id,
            model,
            session,
            command,
        } => {
            let mut command = command.clone();
            let code = if id == "kimi-code" {
                wakeit_agent_runtime::kimi_acp::run(&command, model.as_deref(), session.as_deref())
                    .await?
            } else {
                if let Some(session) = session {
                    wakeit_agent_runtime::sessions::inspect(id, session, &std::env::current_dir()?)
                        .await?;
                }
                if let Some(model) = model {
                    let index = command
                        .iter()
                        .position(|arg| arg == "--")
                        .unwrap_or(command.len());
                    command.splice(index..index, ["--model".to_owned(), model.clone()]);
                }
                wakeit_agent_runtime::stream::stream_checked(id, &command, session.as_deref())
                    .await?
            };
            if code != 0 {
                std::process::exit(code);
            }
        }
        Adapter::Detect { id } => {
            println!("{}", wakeit_agent_runtime::discovery::detect(id).await?);
        }
        Adapter::Models { id } => {
            println!(
                "{}",
                serde_json::to_string(&wakeit_agent_runtime::discovery::discover(id).await?)?
            );
        }
        Adapter::Session { command } => match command {
            AdapterSession::Rename {
                protocol,
                id,
                title,
                cwd,
            } => {
                wakeit_agent_runtime::sessions::rename(protocol, id, title, cwd).await?;
            }
            AdapterSession::List {
                protocol,
                cwd,
                cursor,
                limit,
                query,
            } => {
                let list = wakeit_agent_runtime::sessions::list(
                    protocol,
                    cwd,
                    cursor.as_deref(),
                    *limit,
                    query.as_deref(),
                )
                .await?;
                println!("{}", serde_json::to_string(&list)?);
            }
            AdapterSession::Inspect { protocol, id, cwd } => {
                let info = wakeit_agent_runtime::sessions::inspect(protocol, id, cwd).await?;
                println!("{}", serde_json::to_string(&info)?);
            }
        },
    }
    Ok(())
}
