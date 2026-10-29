use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use wakeit_core::config;
use wakeit_gateway::{
    auth, config::GatewayConfig, gateway, store, transport::authority as nats_auth,
};
#[derive(Parser)]
#[command(name = "wakeit-gateway", version, about = "Wakeit scheduling service")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Explicitly grant instance account-management access after a database upgrade.
    GrantInstanceAdmin {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        user_id: Uuid,
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    Start {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    Migrate {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    Bootstrap {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        username: String,
        #[arg(long, default_value = "WAKEIT_PASSWORD")]
        password_env: String,
        #[arg(long)]
        env_file: Option<PathBuf>,
    },
    /// Generate system startup files without installing or starting services.
    Service {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        env_file: PathBuf,
        #[arg(long)]
        user: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long, requires = "nats_config")]
        nats_bin: Option<PathBuf>,
        #[arg(long, requires = "nats_bin")]
        nats_config: Option<PathBuf>,
        #[arg(long)]
        postgres_service: Option<String>,
    },
    NatsInit {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 4222)]
        port: u16,
        #[arg(long, default_value_t = 8081)]
        ws_port: u16,
    },
}

fn main() {
    let cli = Cli::parse();
    let env_file = match &cli.command {
        Command::Start { env_file, .. }
        | Command::Migrate { env_file, .. }
        | Command::Bootstrap { env_file, .. }
        | Command::GrantInstanceAdmin { env_file, .. } => env_file.as_deref(),
        _ => None,
    };
    if let Some(path) = env_file {
        let vars = wakeit_gateway::deployment::read_environment(path).unwrap_or_else(|e| {
            eprintln!("wakeit-gateway: {e:#}");
            std::process::exit(1)
        });
        for (name, value) in vars {
            unsafe {
                std::env::set_var(name, value);
            }
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "wakeit_gateway=info,tower_http=warn".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Tokio runtime");
    if let Err(e) = runtime.block_on(run(cli)) {
        eprintln!("wakeit-gateway: {e:#}");
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<()> {
    match &cli.command {
        Command::NatsInit {
            dir,
            host,
            port,
            ws_port,
        } => {
            nats_auth::init(dir, host, *port, *ws_port)?;
            println!("NATS configuration: {}", dir.join("nats.conf").display());
        }
        Command::GrantInstanceAdmin {
            config, user_id, ..
        } => {
            let c = read_gateway(config)?;
            let pool = store::pool(&c.database_url_env).await?;
            let result = sqlx::query("UPDATE users SET instance_admin=true WHERE id=$1 AND active")
                .bind(user_id)
                .execute(&pool)
                .await?;
            anyhow::ensure!(result.rows_affected() == 1, "active account not found");
            println!("Instance administrator designated.");
        }
        Command::Start { config, .. } => gateway::start(read_gateway(config)?).await?,
        Command::Migrate { config, .. } => {
            let c = read_gateway(config)?;
            store::migrate(&store::pool(&c.database_url_env).await?).await?;
            println!("Database migrated.");
        }
        Command::Bootstrap {
            config,
            username,
            password_env,
            ..
        } => {
            let c = read_gateway(config)?;
            let pool = store::pool(&c.database_url_env).await?;
            auth::credentials::bootstrap(
                &pool,
                username,
                &std::env::var(password_env).with_context(|| format!("set {password_env}"))?,
            )
            .await?;
            println!("Workspace administrator created.");
        }
        Command::Service {
            config,
            env_file,
            user,
            output,
            platform,
            nats_bin,
            nats_config,
            postgres_service,
        } => {
            let files = wakeit_gateway::deployment::generate(
                &wakeit_gateway::deployment::ServiceOptions {
                    executable: std::env::current_exe()?,
                    config: config.clone(),
                    env_file: env_file.clone(),
                    user: user.clone(),
                    platform: platform
                        .clone()
                        .unwrap_or_else(|| std::env::consts::OS.into()),
                    nats_bin: nats_bin.clone(),
                    nats_config: nats_config.clone(),
                    postgres_service: postgres_service.clone(),
                },
            )?;
            wakeit_gateway::deployment::write_bundle(output, &files)?;
            display(
                json!({"output":output,"files":files.keys().collect::<Vec<_>>()}),
                cli.json,
            );
        }
    }
    Ok(())
}
fn display(value: Value, _json: bool) {
    println!("{}", value);
}
fn read_gateway(path: &Path) -> Result<GatewayConfig> {
    let mut c: GatewayConfig = config::read(path)?;
    if let Ok(value) = std::env::var("WAKEIT_PUBLIC_URL") {
        c.public_url = value;
    }
    if let Ok(value) = std::env::var("WAKEIT_NATS_PUBLIC_URL") {
        c.nats_public_url = value;
    }
    if let Ok(value) = std::env::var("WAKEIT_NATS_URL") {
        c.nats_url = value;
    }
    if let Ok(value) = std::env::var("WAKEIT_NATS_REPLICAS") {
        c.nats_replicas = value.parse()?;
    }
    if c.nats_auth_dir.is_relative() {
        c.nats_auth_dir = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(c.nats_auth_dir);
    }
    Ok(c)
}
