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
    /// Stream Kimi's native local Server API with durable session metadata.
    Kimi {
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(last = true)]
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
    Stream {
        protocol: String,
        #[arg(last = true, required = true)]
        command: Vec<String>,
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
    Resume {
        protocol: String,
        id: String,
        #[arg(last = true, required = true)]
        command: Vec<String>,
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
        Adapter::Kimi {
            model,
            session,
            title,
            command,
        } => {
            let code = wakeit_agent_runtime::kimi_server::run(
                command,
                model.as_deref(),
                session.as_deref(),
                title.as_deref(),
            )
            .await?;
            std::process::exit(code);
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
            AdapterSession::Resume {
                protocol,
                id,
                command,
            } => {
                let code =
                    wakeit_agent_runtime::sessions::resume(protocol, id, command.clone()).await?;
                if code != 0 {
                    std::process::exit(code);
                }
            }
        },
        Adapter::Stream { protocol, command } => {
            let code = wakeit_agent_runtime::stream::stream(protocol, command).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
    }
    Ok(())
}
