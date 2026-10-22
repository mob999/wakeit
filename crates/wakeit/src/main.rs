use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use wakeit::{
    auth,
    config::{self, AgentConfig, GatewayConfig, RunnerConfig, TaskConfig},
    gateway, nats_auth, process, runner, store,
};

#[derive(Parser)]
#[command(name = "wakeit", version, about = "Scheduling for agents")]
struct Cli {
    #[arg(long, global = true, env = "WAKEIT_WORKSPACE")]
    workspace: Option<Uuid>,
    #[arg(long, global = true, env = "WAKEIT_GATEWAY", default_value = "")]
    gateway: String,
    #[arg(
        long,
        global = true,
        env = "WAKEIT_TOKEN_FILE",
        default_value = "__wakeit_profile__"
    )]
    token_file: PathBuf,
    #[arg(long, global = true, env = "WAKEIT_TOKEN")]
    token: Option<String>,
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    request_key: Option<String>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Connect this CLI, enroll a Runner, install user autostart, and verify readiness.
    Setup {
        #[arg(long)]
        runner_name: Option<String>,
        #[arg(long = "allow-root")]
        roots: Vec<PathBuf>,
    },
    Connection {
        #[command(subcommand)]
        command: Connection,
    },
    /// Check for or install a newer official release of this executable.
    Update {
        /// Check only; do not download or replace the executable.
        #[arg(long)]
        check: bool,
    },
    Gateway {
        #[command(subcommand)]
        command: Gateway,
    },
    Runner {
        #[command(subcommand)]
        command: Runner,
    },
    Agent {
        #[command(subcommand)]
        command: Agent,
    },
    Task {
        #[command(subcommand)]
        command: Task,
    },
    Run {
        #[command(subcommand)]
        command: Run,
    },
    Login {
        #[arg(long)]
        username: Option<String>,
        #[arg(long, default_value = "WAKEIT_PASSWORD")]
        password_env: String,
    },
    Members {
        #[command(subcommand)]
        command: Members,
    },
    Adapter {
        #[command(subcommand)]
        command: Adapter,
    },
    Schema {
        kind: String,
    },
}
#[derive(Subcommand)]
enum Connection {
    List,
    Use { name: String },
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
#[derive(Subcommand)]
enum Gateway {
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
#[derive(Subcommand)]
enum Runner {
    Start {
        #[arg(long)]
        config: PathBuf,
    },
    Enroll {
        #[arg(long)]
        name: String,
        #[arg(long)]
        token_env: Option<String>,
        #[arg(long)]
        token_file: Option<PathBuf>,
        #[arg(long, default_value = ".wakeit/runner")]
        dir: PathBuf,
        #[arg(long = "allow-root")]
        roots: Vec<PathBuf>,
    },
    List,
    Revoke {
        id: Uuid,
    },
    Enrollment,
    Service {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        platform: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
enum Agent {
    Init {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    Validate {
        #[arg(long)]
        file: PathBuf,
    },
    Test {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(
            long,
            default_value = "Respond with a short hello without modifying files."
        )]
        prompt: String,
    },
    List,
    Doctor {
        #[arg(long)]
        config: PathBuf,
    },
}
#[derive(Subcommand)]
enum Task {
    Archive {
        id: String,
    },
    Restore {
        id: String,
    },
    Apply {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<i64>,
    },
    List {
        #[arg(long)]
        archived: bool,
    },
    Get {
        id: String,
    },
    Trigger {
        id: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
}
#[derive(Subcommand)]
enum Run {
    /// Observe one Run until it ends, streaming output and the final result.
    Watch {
        id: Uuid,
        /// Limit observation time, including requests and reconnection (for example 1h).
        #[arg(long, value_parser = wakeit::watch::timeout)]
        timeout: Option<Duration>,
    },
    /// Continue an existing session on the Runner device without resubmitting its task.
    Resume {
        id: Uuid,
        #[arg(long)]
        runner_config: Option<PathBuf>,
    },
    List {
        #[arg(long)]
        task: Option<String>,
        #[arg(long)]
        state: Option<String>,
    },
    Get {
        id: Uuid,
    },
    Logs {
        id: Uuid,
        #[arg(long)]
        follow: bool,
    },
    Cancel {
        id: Uuid,
    },
    Retry {
        id: Uuid,
        /// Retry one failed DAG node in the same Run; successful nodes are retained.
        #[arg(long)]
        node: Option<String>,
        /// Explicitly resume a cancelled graph or graph timeout after execution has stopped.
        #[arg(long, requires = "node")]
        resume_stopped: bool,
    },
    Resolve {
        id: Uuid,
        #[arg(long)]
        state: String,
        #[arg(long)]
        reason: String,
    },
}
#[derive(Subcommand)]
enum Members {
    List,
    Invite {
        #[arg(long, default_value = "operator")]
        role: String,
    },
}
struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
    key: String,
    workspace: Uuid,
}
impl Client {
    fn new(cli: &Cli) -> Result<Self> {
        let token = cli
            .token
            .clone()
            .or_else(|| std::fs::read_to_string(&cli.token_file).ok())
            .context("login first, or set WAKEIT_TOKEN / WAKEIT_TOKEN_FILE")?;
        Ok(Self {
            workspace: cli
                .workspace
                .context("choose a workspace with --workspace or connection use")?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            url: cli.gateway.trim_end_matches('/').into(),
            token,
            key: cli
                .request_key
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
        })
    }
    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1/workspaces/{}{path}", self.url, self.workspace)
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let mut request = self
            .http
            .request(method, self.api_url(path))
            .bearer_auth(self.token.trim())
            .header("Idempotency-Key", &self.key);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        let value: Value = response.json().await?;
        ensure!(status.is_success(), "{status}: {}", value["error"]);
        Ok(value)
    }
    async fn get(&self, path: &str) -> Result<Value> {
        self.request(reqwest::Method::GET, path, None).await
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.request(reqwest::Method::POST, path, Some(body)).await
    }
}
fn main() {
    let cli = Cli::parse();
    let env_file = match &cli.command {
        Command::Gateway {
            command:
                Gateway::Start { env_file, .. }
                | Gateway::Migrate { env_file, .. }
                | Gateway::Bootstrap { env_file, .. }
                | Gateway::GrantInstanceAdmin { env_file, .. },
        } => env_file.as_deref(),
        _ => None,
    };
    // Load before starting any runtime or worker threads: Rust 2024 requires
    // process environment mutation to be confined to single-threaded startup.
    if let Some(path) = env_file {
        let vars = match wakeit::service::read_environment(path) {
            Ok(vars) => vars,
            Err(error) => {
                eprintln!("wakeit: {error:#}");
                std::process::exit(1);
            }
        };
        for (name, value) in vars {
            // SAFETY: the process is single-threaded here, before Tokio/tracing.
            unsafe {
                std::env::set_var(name, value);
            }
        }
    }
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
    let is_update = matches!(&cli.command, Command::Update { .. });
    let watch_id = match &cli.command {
        Command::Run {
            command: Run::Watch { id, .. },
        } => Some(*id),
        _ => None,
    };
    if let Err(e) = runtime.block_on(run(cli)) {
        if let Some(id) = watch_id {
            let _ = wakeit::watch::setup_failure(id, json_output, &e.to_string());
            std::process::exit(2);
        }
        if json_output && is_update {
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
        cli.gateway = wakeit::device_auth::validate_url(&cli.gateway)?;
    }
    let local_only = matches!(
        &cli.command,
        Command::Gateway { .. }
            | Command::Adapter { .. }
            | Command::Schema { .. }
            | Command::Update { .. }
            | Command::Connection { .. }
            | Command::Run {
                command: Run::Resume { .. }
            }
    ) || matches!(
        &cli.command,
        Command::Runner {
            command: Runner::Start { .. } | Runner::Enroll { .. } | Runner::Service { .. }
        }
    ) || matches!(&cli.command, Command::Agent { command } if !matches!(command, Agent::List));
    let current = if local_only {
        None
    } else {
        wakeit::connection::resolve(None, None)?
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
        wakeit::connection::resolve(Some(&cli.gateway), cli.workspace)?
    };
    if !local_only
        && !matches!(&cli.command, Command::Login { .. })
        && let Some(old) = profile.take()
    {
        profile = Some(wakeit::connection::upgrade_legacy(old, cli.workspace).await?);
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
            cli.token_file = wakeit::connection::directory()?.join("not-authenticated");
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
            wakeit::setup::run(
                &cli.gateway,
                cli.workspace
                    .context("setup requires --workspace or a selected workspace connection")?,
                runner_name.as_deref(),
                roots,
            )
            .await?
        }
        Command::Connection { command } => match command {
            Connection::List => {
                let p = wakeit::connection::load()?;
                display(
                    json!({"current":p.current,"connections":p.connections.iter().map(|(name,p)|json!({"name":name,"gateway":p.gateway,"username":p.username,"user_id":p.user_id,"workspace_id":p.workspace_id,"workspace":p.workspace_name})).collect::<Vec<_>>()}),
                    true,
                );
            }
            Connection::Use { name } => {
                wakeit::connection::use_profile(name)?;
                println!("Current connection: {name}");
            }
        },
        Command::Update { check } => wakeit::update::run(*check, cli.json).await?,
        Command::Adapter {
            command:
                Adapter::Kimi {
                    model,
                    session,
                    title,
                    command,
                },
        } => {
            let code = wakeit::kimi_live::run(
                command,
                model.as_deref(),
                session.as_deref(),
                title.as_deref(),
            )
            .await?;
            std::process::exit(code);
        }
        Command::Adapter {
            command: Adapter::Detect { id },
        } => {
            println!("{}", wakeit::adapters::detect(id).await?);
        }
        Command::Adapter {
            command: Adapter::Models { id },
        } => {
            println!(
                "{}",
                serde_json::to_string(&wakeit::adapters::discover(id).await?)?
            );
        }
        Command::Adapter {
            command: Adapter::Session { command },
        } => match command {
            AdapterSession::Rename {
                protocol,
                id,
                title,
                cwd,
            } => {
                wakeit::native_sessions::rename(protocol, id, title, cwd).await?;
            }
            AdapterSession::List {
                protocol,
                cwd,
                cursor,
                limit,
                query,
            } => {
                let list = wakeit::native_sessions::list(
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
                let info = wakeit::native_sessions::inspect(protocol, id, cwd).await?;
                println!("{}", serde_json::to_string(&info)?);
            }
            AdapterSession::Resume {
                protocol,
                id,
                command,
            } => {
                let code = wakeit::native_sessions::resume(protocol, id, command.clone()).await?;
                if code != 0 {
                    std::process::exit(code);
                }
            }
        },
        Command::Adapter {
            command: Adapter::Stream { protocol, command },
        } => {
            let code = wakeit::stream_adapter::stream(protocol, command).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
        Command::Gateway { command } => match command {
            Gateway::NatsInit {
                dir,
                host,
                port,
                ws_port,
            } => {
                nats_auth::init(dir, host, *port, *ws_port)?;
                println!("NATS configuration: {}", dir.join("nats.conf").display());
            }
            Gateway::GrantInstanceAdmin {
                config, user_id, ..
            } => {
                let c = read_gateway(config)?;
                let pool = store::pool(&c.database_url_env).await?;
                let result =
                    sqlx::query("UPDATE users SET instance_admin=true WHERE id=$1 AND active")
                        .bind(user_id)
                        .execute(&pool)
                        .await?;
                anyhow::ensure!(result.rows_affected() == 1, "active account not found");
                println!("Instance administrator designated.");
            }
            Gateway::Start { config, .. } => gateway::start(read_gateway(config)?).await?,
            Gateway::Migrate { config, .. } => {
                let c = read_gateway(config)?;
                store::migrate(&store::pool(&c.database_url_env).await?).await?;
                println!("Database migrated.");
            }
            Gateway::Bootstrap {
                config,
                username,
                password_env,
                ..
            } => {
                let c = read_gateway(config)?;
                let pool = store::pool(&c.database_url_env).await?;
                auth::bootstrap(
                    &pool,
                    username,
                    &std::env::var(password_env).with_context(|| format!("set {password_env}"))?,
                )
                .await?;
                println!("Workspace administrator created.");
            }
            Gateway::Service {
                config,
                env_file,
                user,
                output,
                platform,
                nats_bin,
                nats_config,
                postgres_service,
            } => {
                let files = wakeit::service::generate(&wakeit::service::ServiceOptions {
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
                })?;
                wakeit::service::write_bundle(output, &files)?;
                display(
                    json!({"output":output,"files":files.keys().collect::<Vec<_>>()}),
                    cli.json,
                );
            }
        },
        Command::Runner { command } => match command {
            Runner::Start { config } => {
                let runner_config = read_runner(config)?;
                wakeit::sessions::register(config)?;
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
                wakeit::sessions::register(&path)?;
                println!(
                    "Runner enrolled. Start with: wakeit runner start --config {}",
                    path.display()
                );
            }
            Runner::List => display(Client::new(&cli)?.get("/runners").await?, cli.json),
            Runner::Enrollment => display(
                Client::new(&cli)?
                    .post("/runners/enrollment", json!({}))
                    .await?,
                true,
            ),
            Runner::Revoke { id } => display(
                Client::new(&cli)?
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
        },
        Command::Agent { command } => match command {
            Agent::Init { id, file } => {
                let file = file
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(format!("{id}.yaml")));
                ensure!(!file.exists(), "file already exists");
                let mut agent: AgentConfig = yaml_serde::from_str(config::builtin(id)?)?;
                config::normalize_commands(&mut agent)?;
                std::fs::write(&file, yaml_serde::to_string(&agent)?)?;
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
                for file in c.agents {
                    let mut a: AgentConfig = config::read(&file)?;
                    config::normalize_commands(&mut a)?;
                    a.validate()?;
                    items.push(runner::inspect(&a, &std::env::current_dir()?).await);
                }
                display(json!(items), cli.json);
            }
            Agent::Test { file, cwd, prompt } => {
                let mut a: AgentConfig = config::read(file)?;
                config::normalize_commands(&mut a)?;
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
                let out = process::execute_output_named(
                    &a.submit,
                    &t,
                    id,
                    None,
                    Duration::from_secs(120),
                    tokio_util::sync::CancellationToken::new(),
                    None,
                    matches!(a.output.format, config::OutputFormat::Events),
                    Some(&session_title),
                )
                .await?;
                ensure!(
                    out.code == Some(0) && out.stopped.is_none(),
                    "agent submit failed: {}",
                    out.stdout
                );
                if a.mode == config::AgentMode::Sync {
                    display(
                        json!({"state":"succeeded","result":out.result.unwrap_or(json!(out.stdout))}),
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
            Agent::List => display(Client::new(&cli)?.get("/agents").await?, cli.json),
        },
        Command::Task { command } => {
            let c = Client::new(&cli)?;
            match command {
                Task::Apply { file, expect } => {
                    let task: TaskConfig = if file.as_os_str() == "-" {
                        use std::io::Read;
                        let mut s = String::new();
                        std::io::stdin().read_to_string(&mut s)?;
                        yaml_serde::from_str(&s)?
                    } else {
                        config::read(file)?
                    };
                    task.validate()?;
                    let expected = if let Some(e) = expect {
                        Some(*e)
                    } else {
                        let response = c
                            .http
                            .get(c.api_url(&format!("/tasks/{}", task.id)))
                            .bearer_auth(c.token.trim())
                            .send()
                            .await?;
                        if response.status() == reqwest::StatusCode::NOT_FOUND {
                            None
                        } else {
                            let status = response.status();
                            let v: Value = response.json().await?;
                            ensure!(status.is_success(), "{status}: {}", v["error"]);
                            Some(v["revision"].as_i64().context("revision")?)
                        }
                    };
                    display(
                        c.request(
                            reqwest::Method::PUT,
                            &format!("/tasks/{}", task.id),
                            Some(json!({"config":task,"expected_revision":expected})),
                        )
                        .await?,
                        cli.json,
                    );
                }
                Task::List { archived } => display(
                    c.get(if *archived {
                        "/tasks?archived=true"
                    } else {
                        "/tasks"
                    })
                    .await?,
                    cli.json,
                ),
                Task::Archive { id } | Task::Restore { id } => {
                    let old = c.get(&format!("/tasks/{id}")).await?;
                    let operation = if matches!(command, Task::Archive { .. }) {
                        "archive"
                    } else {
                        "restore"
                    };
                    display(
                        c.post(
                            &format!("/tasks/{id}/{operation}"),
                            json!({"expected_revision":old["revision"]}),
                        )
                        .await?,
                        cli.json,
                    );
                }
                Task::Get { id } => display(c.get(&format!("/tasks/{id}")).await?, true),
                Task::Trigger { id } => display(
                    c.post(&format!("/tasks/{id}/runs"), json!({})).await?,
                    cli.json,
                ),
                Task::Pause { id } | Task::Resume { id } => {
                    let old = c.get(&format!("/tasks/{id}")).await?;
                    let mut config = old["config"].clone();
                    config["enabled"] = json!(matches!(command, Task::Resume { .. }));
                    display(
                        c.request(
                            reqwest::Method::PUT,
                            &format!("/tasks/{id}"),
                            Some(json!({"config":config,"expected_revision":old["revision"]})),
                        )
                        .await?,
                        cli.json,
                    );
                }
            }
        }
        Command::Run {
            command: Run::Watch { id, timeout },
        } => {
            let c = Client::new(&cli)?;
            let code =
                wakeit::watch::run_scoped(&c.url, c.workspace, &c.token, *id, *timeout, cli.json)
                    .await?;
            std::process::exit(code);
        }
        Command::Run {
            command: Run::Resume { id, runner_config },
        } => {
            let code = wakeit::sessions::resume(*id, runner_config.as_deref()).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
        Command::Run { command } => {
            let c = Client::new(&cli)?;
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
                        wakeit::config::validate_id(node)?;
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
                        if run["state"]
                            .as_str()
                            .is_some_and(wakeit::protocol::terminal)
                        {
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
        Command::Members { command } => {
            let c = Client::new(&cli)?;
            match command {
                Members::List => display(c.get("/members?page=1&page_size=100").await?, cli.json),
                Members::Invite { role } => {
                    display(c.post("/invites", json!({"role":role})).await?, true)
                }
            }
        }
        Command::Login {
            username,
            password_env,
        } => {
            let Some(username) = username else {
                let intent = wakeit::device_auth::Intent {
                    workspace_id: Some(cli.workspace.context(
                        "login requires --workspace or a selected workspace connection",
                    )?),
                    name: std::env::var("HOSTNAME").unwrap_or_else(|_| "wakeit-cli".into()),
                    os: std::env::consts::OS.into(),
                    register_runner: false,
                    allowed_roots: vec![],
                };
                let p = wakeit::setup::login(&cli.gateway, &intent).await?;
                println!("Connected to {} as {}", p.gateway, p.username);
                return Ok(());
            };
            let workspace = cli
                .workspace
                .context("login requires --workspace or a selected workspace connection")?;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?;
            let response=client.post(format!("{}/api/v1/session",cli.gateway)).json(&json!({"username":username,"password":std::env::var(password_env).with_context(||format!("set {password_env}"))?})).send().await?;
            let status = response.status();
            let body: Value = response.json().await?;
            ensure!(status.is_success(), "login {status}: {}", body["error"]);
            let session = body["token"].as_str().context("session token")?;
            let result: Result<wakeit::connection::Profile> = async {
                let response = client
                    .get(format!("{}/api/v1/session", cli.gateway))
                    .bearer_auth(session)
                    .send()
                    .await?;
                ensure!(
                    response.status().is_success(),
                    "account authentication failed"
                );
                let identity: Value = response.json().await?;
                let response = client
                    .get(format!("{}/api/v1/workspaces/{workspace}", cli.gateway))
                    .bearer_auth(session)
                    .send()
                    .await?;
                ensure!(response.status().is_success(), "workspace is unavailable");
                let metadata: Value = response.json().await?;
                let response = client
                    .post(format!(
                        "{}/api/v1/workspaces/{workspace}/tokens",
                        cli.gateway
                    ))
                    .bearer_auth(session)
                    .json(&json!({"name":"wakeit cli"}))
                    .send()
                    .await?;
                ensure!(response.status().is_success(), "API Key creation failed");
                let key: Value = response.json().await?;
                let token = key["token"].as_str().context("API Key missing")?;
                let user = identity["id"]
                    .as_str()
                    .context("account ID missing")?
                    .parse()?;
                let profile = wakeit::connection::save(
                    &cli.gateway,
                    workspace,
                    user,
                    metadata["name"]
                        .as_str()
                        .context("workspace name missing")?,
                    username,
                    token,
                )?;
                if explicit_token_file {
                    wakeit::connection::atomic(&cli.token_file, token)?;
                }
                Ok(profile)
            }
            .await;
            let _ = client
                .delete(format!("{}/api/v1/session", cli.gateway))
                .bearer_auth(session)
                .send()
                .await;
            let profile = result?;
            println!(
                "Connected to {} / {} as {}",
                profile.gateway,
                profile.workspace_name.as_deref().unwrap_or("workspace"),
                profile.username
            );
        }

        Command::Schema { kind } => {
            let value = match kind.as_str() {
                "agent" => serde_json::to_value(schemars::schema_for!(AgentConfig))?,
                "task" => serde_json::to_value(schemars::schema_for!(TaskConfig))?,
                "stream" => {
                    serde_json::to_value(schemars::schema_for!(wakeit::trace::StreamRecord))?
                }
                "openapi" => wakeit::openapi::document(),
                _ => bail!("kind must be agent, task, stream or openapi"),
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
fn read_runner(path: &Path) -> Result<RunnerConfig> {
    wakeit::sessions::read_runner(path)
}

fn service(config: &Path, platform: &str) -> Result<String> {
    let config = config.canonicalize()?;
    let executable = std::env::current_exe()?;
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    Ok(match platform {
        "linux" => format!(
            "[Unit]\nDescription=wakeit runner\nAfter=network-online.target\n[Service]\nExecStart=\"{}\" runner start --config \"{}\"\nRestart=on-failure\nRestartSec=5\nKillMode=control-group\n[Install]\nWantedBy=default.target\n",
            executable
                .display()
                .to_string()
                .replace('%', "%%")
                .replace('"', "\\\""),
            config
                .display()
                .to_string()
                .replace('%', "%%")
                .replace('"', "\\\"")
        ),
        "macos" => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\"><plist version=\"1.0\"><dict><key>Label</key><string>dev.wakeit.runner</string><key>ProgramArguments</key><array><string>{}</string><string>runner</string><string>start</string><string>--config</string><string>{}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/></dict></plist>\n",
            xml(&executable.to_string_lossy()),
            xml(&config.to_string_lossy())
        ),
        "windows" => format!(
            "$action = New-ScheduledTaskAction -Execute '{}' -Argument 'runner start --config \"{}\"'\n$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME\nRegister-ScheduledTask -TaskName 'wakeit runner' -Action $action -Trigger $trigger -Description 'wakeit agent runner'\n",
            executable.display().to_string().replace('\'', "''"),
            config.display().to_string().replace('\'', "''")
        ),
        _ => bail!("platform must be linux, macos or windows"),
    })
}
