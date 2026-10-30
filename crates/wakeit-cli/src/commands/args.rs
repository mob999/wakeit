use clap::{Parser, Subcommand};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "wakeit", version, about = "Scheduling for agents")]
pub(super) struct Cli {
    #[arg(long, global = true, env = "WAKEIT_WORKSPACE")]
    pub(super) workspace: Option<Uuid>,
    #[arg(long, global = true, env = "WAKEIT_GATEWAY", default_value = "")]
    pub(super) gateway: String,
    #[arg(
        long,
        global = true,
        env = "WAKEIT_TOKEN_FILE",
        default_value = "__wakeit_profile__"
    )]
    pub(super) token_file: PathBuf,
    #[arg(long, global = true, env = "WAKEIT_TOKEN")]
    pub(super) token: Option<String>,
    #[arg(long, global = true)]
    pub(super) json: bool,
    #[arg(long, global = true)]
    pub(super) request_key: Option<String>,
    #[command(subcommand)]
    pub(super) command: Command,
}
#[derive(Subcommand)]
pub(super) enum Command {
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
    /// Install or update the Wakeit Skill for your Agent.
    Skill {
        #[command(subcommand)]
        command: Skill,
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
    Template {
        #[command(subcommand)]
        command: Template,
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
    /// Manage adapter packages on this device.
    Plugin {
        #[command(subcommand)]
        command: Plugin,
    },
    Schema {
        kind: String,
    },
}
#[derive(Subcommand)]
pub(super) enum Connection {
    List,
    Use { name: String },
}
#[derive(Subcommand)]
pub(super) enum Runner {
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
pub(super) enum Agent {
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
#[derive(clap::Args)]
pub(super) struct TemplateValues {
    #[arg(long)]
    pub(super) inputs_file: Option<PathBuf>,
    #[arg(long)]
    pub(super) bindings_file: Option<PathBuf>,
    #[arg(long)]
    pub(super) schedule_file: Option<PathBuf>,
    #[arg(long)]
    pub(super) expect: Option<i64>,
    #[arg(long)]
    pub(super) id: Option<String>,
    #[arg(long)]
    pub(super) name: Option<String>,
}
#[derive(Subcommand)]
pub(super) enum Template {
    List {
        #[arg(long)]
        archived: bool,
    },
    Get {
        id: String,
    },
    Apply {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        expect: Option<i64>,
    },
    Validate {
        #[arg(long)]
        file: PathBuf,
    },
    Preview {
        id: String,
        #[command(flatten)]
        values: TemplateValues,
    },
    Archive {
        id: String,
    },
    Restore {
        id: String,
    },
    FromTask {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
pub(super) enum Task {
    Create {
        #[arg(long)]
        template: String,
        #[command(flatten)]
        values: TemplateValues,
        #[arg(long = "run")]
        run_now: bool,
    },
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
pub(super) enum Run {
    /// Observe one Run until it ends, streaming output and the final result.
    Watch {
        id: Uuid,
        /// Limit observation time, including requests and reconnection (for example 1h).
        #[arg(long, value_parser = crate::commands::watch::timeout)]
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
pub(super) enum Members {
    List,
    Invite {
        #[arg(long, default_value = "operator")]
        role: String,
    },
}

#[derive(Subcommand)]
pub(super) enum Skill {
    /// Download the latest official Wakeit Skill without connecting to a Gateway.
    Install {
        /// Destination skill directory (default: ~/.agents/skills/wakeit).
        #[arg(long)]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub(super) enum Plugin {
    /// Check a package without installing it or loading user plugins.
    Validate {
        #[arg(long)]
        dir: PathBuf,
    },
    Install {
        source: String,
        #[arg(long = "replace-agent")]
        replace: Vec<String>,
    },
    List,
    Update {
        source: Option<String>,
    },
    Remove {
        source: String,
    },
    /// Invoke a command declared by an installed plugin.
    Exec {
        source: String,
        command: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
}
