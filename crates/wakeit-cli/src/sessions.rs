//! Native session references and local, user-owned continuation. Never execute commands from Gateway data.
use crate::{
    config::{self, AgentConfig, CommandArg, CommandSpec, RunnerConfig, TaskConfig},
    process,
};
use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use sqlx::Row;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use uuid::Uuid;

pub use wakeit_core::sessions::{RunSession, SessionInfo, validate_id};
/// Export the local adapter's native interactive command, without reading environment values.
pub fn continuation_command(
    agent: &AgentConfig,
    task: &TaskConfig,
    run: Uuid,
    id: &str,
) -> Result<String> {
    validate_id(id)?;
    let spec = &agent
        .session
        .as_ref()
        .context("adapter does not support sessions")?
        .interactive;
    ensure!(
        spec.env.is_empty() && spec.env_from.is_empty(),
        "native continuation requires adapter-specific environment; use the local CLI instead"
    );
    ensure!(
        spec.stdin.is_none(),
        "native continuation cannot export standard input"
    );
    ensure!(
        !serde_json::to_string(spec)?.contains("task.prompt"),
        "native continuation cannot contain the task prompt"
    );
    let mut task = task.clone();
    task.prompt.clear();
    task.parameters = config::resolve_parameters(&agent.parameters, &task.parameters)?;
    task.session = Some(config::SessionBinding { id: id.into() });
    let cwd = config::render(spec.cwd.as_deref().unwrap_or(&task.cwd), &task, run, None)?;
    ensure!(
        Path::new(&cwd).canonicalize()? == Path::new(&task.cwd).canonicalize()?,
        "continuation command changes session working directory"
    );
    let mut argv = vec![spec.command.clone()];
    for arg in &spec.args {
        match arg {
            CommandArg::Literal(value) => argv.push(config::render(value, &task, run, None)?),
            CommandArg::Group { when_set, args }
                if task
                    .parameters
                    .get(when_set)
                    .is_some_and(|value| !value.is_empty()) =>
            {
                for value in args {
                    argv.push(config::render(value, &task, run, None)?);
                }
            }
            CommandArg::Group { .. } => {}
        }
    }
    format_continuation(&cwd, &argv, cfg!(windows))
}
fn quote_posix(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_./:@%+=,-".contains(&byte))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}
fn quote_powershell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
fn windows_shell_path(value: &str) -> String {
    if let Some(path) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{path}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    }
}
fn format_continuation(cwd: &str, argv: &[String], windows: bool) -> Result<String> {
    ensure!(
        !argv.is_empty() && !argv[0].is_empty(),
        "continuation executable is required"
    );
    ensure!(
        !cwd.is_empty()
            && std::iter::once(cwd)
                .chain(argv.iter().map(String::as_str))
                .all(|value| !value.chars().any(char::is_control)),
        "native continuation contains control characters"
    );
    let command = if windows {
        let invocation = argv
            .iter()
            .enumerate()
            .map(|(index, value)| {
                quote_powershell(&if index == 0 {
                    windows_shell_path(value)
                } else {
                    value.clone()
                })
            })
            .collect::<Vec<_>>()
            .join(" ");
        // A terminating directory error stops this script block before invocation.
        format!(
            "& {{ Set-Location -LiteralPath {} -ErrorAction Stop; & {invocation} }}",
            quote_powershell(&windows_shell_path(cwd))
        )
    } else {
        let invocation = argv
            .iter()
            .map(|value| quote_posix(value))
            .collect::<Vec<_>>()
            .join(" ");
        format!("cd -- {} && {invocation}", quote_posix(cwd))
    };
    ensure!(
        command.len() <= 32768,
        "native continuation command is too long"
    );
    Ok(command)
}
pub async fn inspect(
    agent: &AgentConfig,
    task: &TaskConfig,
    run_id: Uuid,
    id: &str,
) -> Result<SessionInfo> {
    validate_id(id)?;
    let spec = &agent
        .session
        .as_ref()
        .context("adapter does not support sessions")?
        .inspect;
    let mut task = task.clone();
    task.prompt.clear();
    task.parameters = config::resolve_parameters(&agent.parameters, &task.parameters)?;
    task.session = Some(config::SessionBinding { id: id.into() });
    let output = process::execute(
        spec,
        &task,
        run_id,
        None,
        Duration::from_secs(25),
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await?;
    ensure!(
        output.code == Some(0) && output.stopped.is_none() && output.confirmed,
        "session inspection failed"
    );
    let info: SessionInfo =
        serde_json::from_str(&output.stdout).context("invalid session inspection response")?;
    validate_id(&info.id)?;
    ensure!(
        info.version == 1 && info.id == id && info.persisted,
        "session does not exist or is not persisted"
    );
    let expected = Path::new(&task.cwd)
        .canonicalize()
        .context("working directory is unavailable")?;
    let actual = Path::new(&info.cwd)
        .canonicalize()
        .context("session working directory is unavailable")?;
    ensure!(
        expected == actual,
        "session belongs to a different working directory"
    );
    Ok(SessionInfo {
        cwd: actual.to_string_lossy().into_owned(),
        ..info
    })
}
/// Apply a fresh session title and verify native persistence. Binding existing sessions never calls this.
pub async fn rename(
    agent: &AgentConfig,
    task: &TaskConfig,
    run: Uuid,
    id: &str,
    title: &str,
) -> Result<SessionInfo> {
    validate_id(id)?;
    ensure!(
        !title.trim().is_empty() && title.len() <= 200 && !title.chars().any(char::is_control),
        "invalid session title"
    );
    let adapter = agent
        .session
        .as_ref()
        .context("adapter does not support sessions")?;
    let mut task = task.clone();
    task.prompt.clear();
    task.parameters = config::resolve_parameters(&agent.parameters, &task.parameters)?;
    task.session = Some(config::SessionBinding { id: id.into() });
    if let Some(spec) = &adapter.rename {
        let output = process::execute_named(
            spec,
            &task,
            run,
            None,
            Duration::from_secs(25),
            tokio_util::sync::CancellationToken::new(),
            None,
            Some(title),
        )
        .await?;
        ensure!(
            output.code == Some(0) && output.stopped.is_none() && output.confirmed,
            "session rename command failed"
        );
    }
    let info = inspect(agent, &task, run, id).await?;
    ensure!(
        info.title.as_deref() == Some(title),
        "Agent did not persist the requested session title"
    );
    Ok(info)
}
fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
pub fn lock(config: &RunnerConfig, agent: &str, id: &str) -> Result<File> {
    lock_for(&config.data_dir, config.id, agent, id)
}
/// Shared by scheduled execution and local interactive continuation.
pub fn lock_for(dir: &Path, runner: Uuid, agent: &str, id: &str) -> Result<File> {
    validate_id(id)?;
    let directory = dir.join("session-locks");
    private_directory(&directory)?;
    let key = hex::encode(Sha256::digest(serde_json::to_vec(&(runner, agent, id))?));
    let file = private_file(&directory.join(format!("{key}.lock")))?;
    fs2::FileExt::try_lock_exclusive(&file).context("session is already in use")?;
    Ok(file)
}
fn registry_directory() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .context("APPDATA is unavailable")?;
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|p| PathBuf::from(p).join("Library/Application Support"))
        .context("user home is unavailable")?;
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .context("user configuration directory is unavailable")?;
    Ok(base.join("wakeit"))
}
pub fn registered_configs() -> Result<Vec<PathBuf>> {
    registered_configs_at(&registry_directory()?)
}
fn registered_configs_at(directory: &Path) -> Result<Vec<PathBuf>> {
    let path = directory.join("runners.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("invalid local Runner registry"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
pub fn register(path: &Path) -> Result<()> {
    register_at(path, &registry_directory()?)
}
fn register_at(path: &Path, directory: &Path) -> Result<()> {
    let path = path
        .canonicalize()
        .context("Runner configuration is unavailable")?;
    private_directory(directory)?;
    let guard = private_file(&directory.join("registry.lock"))?;
    fs2::FileExt::lock_exclusive(&guard)?;
    let mut configs = registered_configs_at(directory)?;
    if !configs.contains(&path) {
        configs.push(path);
        use std::io::Write;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(serde_json::to_string(&configs)?.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(directory.join("runners.json"))?;
    }
    Ok(())
}
pub fn read_runner(path: &Path) -> Result<RunnerConfig> {
    let path = path.canonicalize()?;
    let mut config: RunnerConfig = config::read(&path)?;
    let base = path
        .parent()
        .context("Runner configuration directory is unavailable")?;
    for file in [
        &mut config.token_file,
        &mut config.credentials_file,
        &mut config.data_dir,
    ]
    .into_iter()
    .chain(config.agents.iter_mut())
    .chain(config.allowed_roots.iter_mut())
    {
        if file.is_relative() {
            *file = base.join(&*file);
        }
    }
    Ok(config)
}
async fn local_pool(config: &RunnerConfig) -> Result<sqlx::SqlitePool> {
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(config.data_dir.join("journal.sqlite"))
        .read_only(true)
        .busy_timeout(Duration::from_secs(5));
    Ok(sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?)
}
fn same_session(session: &RunSession, other: &RunSession) -> bool {
    session.runner_id == other.runner_id && session.agent == other.agent && session.id == other.id
}
pub async fn ensure_idle(
    config: &RunnerConfig,
    session: &RunSession,
    except: Option<Uuid>,
) -> Result<()> {
    let occupancy = continuation_marker(config, &session.agent, &session.id)?;
    ensure!(
        !occupancy.try_exists()?,
        "an earlier continuation has not been confirmed stopped; verify all its Agent processes have exited, then remove {}",
        occupancy.display()
    );
    let pool = local_pool(config).await?;
    let rows = sqlx::query("SELECT id,session,assignment FROM local_runs WHERE phase IN ('claimed','started','unknown')").fetch_all(&pool).await?;
    for row in rows {
        let id: String = row.get("id");
        if except.is_some_and(|value| value.to_string() == id) {
            continue;
        }
        let resolved = row
            .get::<Option<String>, _>("session")
            .map(|data| serde_json::from_str::<RunSession>(&data))
            .transpose()?;
        let assignment: crate::protocol::Assignment =
            serde_json::from_str(&row.get::<String, _>("assignment"))?;
        let matches = resolved
            .as_ref()
            .is_some_and(|other| same_session(session, other))
            || (assignment.runner_id == session.runner_id
                && assignment.task.agent == session.agent
                && assignment
                    .task
                    .session
                    .as_ref()
                    .is_some_and(|binding| binding.id == session.id));
        ensure!(
            !matches,
            "session is occupied by an active or unverified run; verify that run has ended first"
        );
    }
    Ok(())
}
fn continuation_marker(config: &RunnerConfig, agent: &str, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    let key = hex::encode(Sha256::digest(serde_json::to_vec(&(config.id, agent, id))?));
    Ok(config
        .data_dir
        .join("session-continuations")
        .join(format!("{key}.json")))
}
pub(crate) fn mark_continuation(
    config: &RunnerConfig,
    run: Uuid,
    session: &RunSession,
) -> Result<PathBuf> {
    use std::io::Write;
    let path = continuation_marker(config, &session.agent, &session.id)?;
    private_directory(path.parent().context("continuation directory")?)?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .context("a previous continuation still occupies this session")?;
    file.write_all(serde_json::to_vec(&serde_json::json!({"version":1,"run_id":run,"session_id":session.id,"agent":session.agent,"pid":std::process::id()}))?.as_slice())?;
    file.sync_all()?;
    Ok(path)
}
pub(crate) struct PreparedContinuation {
    pub task: TaskConfig,
    pub command: CommandSpec,
    pub session: RunSession,
    pub _lock: File,
}
pub(crate) async fn prepare_local(
    config: &RunnerConfig,
    journal: &crate::journal::Journal,
    run: Uuid,
) -> Result<PreparedContinuation> {
    let saved = journal
        .get(run)
        .await?
        .context("run is not available on this device")?;
    let session = saved.session.context("this run has no saved session")?;
    ensure!(
        saved.phase == "done" && session.ready,
        "session is not ready for continuation"
    );
    ensure!(
        config.id == session.runner_id
            && saved.assignment.runner_id == config.id
            && saved.agent.id == session.agent
            && saved.assignment.task.agent == session.agent,
        "saved session does not belong to this Runner and Agent"
    );
    process::validate_cwd(&session.cwd, &config.allowed_roots)?;
    ensure!(
        Path::new(&saved.assignment.task.cwd).canonicalize()?
            == Path::new(&session.cwd).canonicalize()?,
        "saved working directory does not match session"
    );
    let guard = lock(config, &session.agent, &session.id)?;
    ensure_idle(config, &session, Some(run)).await?;
    let mut agent = saved.agent;
    config::normalize_commands(&mut agent)?;
    agent.validate()?;
    let mut task = saved.assignment.task;
    task.cwd = session.cwd.clone();
    task.prompt.clear();
    task.session = Some(config::SessionBinding {
        id: session.id.clone(),
    });
    inspect(&agent, &task, run, &session.id).await?;
    Ok(PreparedContinuation {
        task,
        command: agent
            .session
            .context("adapter has no interactive session command")?
            .interactive,
        session,
        _lock: guard,
    })
}
pub async fn resume(run_id: Uuid, explicit: Option<&Path>) -> Result<i32> {
    let paths = if let Some(path) = explicit {
        vec![path.to_owned()]
    } else {
        registered_configs()?
    };
    let mut found = None;
    for path in paths {
        let config = match read_runner(&path) {
            Ok(config) => config,
            Err(error) if explicit.is_none() => {
                tracing::debug!(error=%error,"ignoring unavailable registered Runner");
                continue;
            }
            Err(error) => return Err(error),
        };
        let pool = match local_pool(&config).await {
            Ok(pool) => pool,
            Err(_) if explicit.is_none() => continue,
            Err(error) => return Err(error),
        };
        let row = sqlx::query("SELECT assignment,agent,phase,session FROM local_runs WHERE id=?")
            .bind(run_id.to_string())
            .fetch_optional(&pool)
            .await?;
        if let Some(row) = row {
            ensure!(
                found.is_none(),
                "run is present in multiple Runner journals; specify --runner-config"
            );
            let assignment: crate::protocol::Assignment =
                serde_json::from_str(&row.get::<String, _>("assignment"))?;
            let agent: AgentConfig = serde_json::from_str(&row.get::<String, _>("agent"))?;
            let session: RunSession = serde_json::from_str(
                row.get::<Option<String>, _>("session")
                    .as_deref()
                    .context("this run has no saved session")?,
            )?;
            ensure!(
                row.get::<String, _>("phase") == "done" && session.ready,
                "session is not ready for continuation"
            );
            found = Some((config, assignment, agent, session));
        }
    }
    let Some((config, assignment, mut agent, session)) = found else {
        bail!(
            "run is not available on this device; run this command on its Runner device or specify --runner-config"
        );
    };
    ensure!(
        config.id == session.runner_id
            && assignment.runner_id == config.id
            && assignment.task.agent == session.agent
            && agent.id == session.agent,
        "saved session does not belong to this Runner and Agent"
    );
    process::validate_cwd(&session.cwd, &config.allowed_roots)?;
    ensure!(
        Path::new(&assignment.task.cwd).canonicalize()?
            == Path::new(&session.cwd).canonicalize()?,
        "saved working directory does not match session"
    );
    let _guard = lock(&config, &session.agent, &session.id)?;
    ensure_idle(&config, &session, Some(run_id)).await?;
    config::normalize_commands(&mut agent)?;
    agent.validate()?;
    let mut task = assignment.task;
    task.cwd = session.cwd.clone();
    task.prompt.clear();
    task.session = Some(config::SessionBinding {
        id: session.id.clone(),
    });
    inspect(&agent, &task, run_id, &session.id).await?;
    let spec = agent
        .session
        .as_ref()
        .context("adapter does not support session continuation")?
        .interactive
        .clone();
    let occupancy = mark_continuation(&config, run_id, &session)?;
    let result = interactive(&spec, &task, run_id).await;
    if result.as_ref().err().is_none_or(|error| {
        error
            .downcast_ref::<process::CommandStartedError>()
            .is_none()
    }) {
        std::fs::remove_file(&occupancy).with_context(|| {
            format!(
                "could not release continuation occupancy {}",
                occupancy.display()
            )
        })?;
        return result;
    }
    result.with_context(|| format!("if continuation termination is uncertain, verify all Agent processes have exited before removing {}", occupancy.display()))
}
async fn interactive(spec: &CommandSpec, task: &TaskConfig, run_id: Uuid) -> Result<i32> {
    let mut command = tokio::process::Command::new(&spec.command);
    for arg in &spec.args {
        match arg {
            CommandArg::Literal(value) => {
                command.arg(config::render(value, task, run_id, None)?);
            }
            CommandArg::Group { when_set, args }
                if task.parameters.get(when_set).is_some_and(|v| !v.is_empty()) =>
            {
                for value in args {
                    command.arg(config::render(value, task, run_id, None)?);
                }
            }
            CommandArg::Group { .. } => {}
        }
    }
    let cwd = config::render(spec.cwd.as_deref().unwrap_or(&task.cwd), task, run_id, None)?;
    ensure!(
        Path::new(&cwd).canonicalize()? == Path::new(&task.cwd).canonicalize()?,
        "continuation command changes session working directory"
    );
    command.current_dir(&cwd);
    for (key, value) in &spec.env {
        command.env(key, config::render(value, task, run_id, None)?);
    }
    for (key, source) in &spec.env_from {
        command.env(
            key,
            std::env::var(source)
                .with_context(|| format!("environment variable {source} missing"))?,
        );
    }
    command
        .env("PWD", &cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    crate::interactive_session::execute(command).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn runner(dir: &Path) -> RunnerConfig {
        RunnerConfig {
            version: 1,
            gateway: "http://127.0.0.1:1".into(),
            id: Uuid::new_v4(),
            name: "fixture".into(),
            token_file: dir.join("device.token"),
            credentials_file: dir.join("runner.creds"),
            nats_url: "nats://127.0.0.1:1".into(),
            data_dir: dir.join("data"),
            concurrency: 2,
            agents: vec![],
            allowed_roots: vec![dir.to_owned()],
        }
    }
    #[test]
    fn session_ids_reject_paths_options_and_control_characters() {
        for id in [
            "",
            "--last",
            "-s",
            "../session",
            "C:\\session",
            "/tmp/session",
            "ses_\n1",
            "ses 1",
            ".",
            "..",
        ] {
            assert!(validate_id(id).is_err(), "accepted {id:?}");
        }
        for id in [
            "ses_abc123",
            "01234567-89ab-cdef-0123-456789abcdef",
            "third.party-id",
        ] {
            validate_id(id).unwrap();
        }
        assert!(validate_id(&"x".repeat(257)).is_err());
    }
    #[test]
    fn session_file_lock_is_exclusive_and_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let config = runner(dir.path());
        let first = lock(&config, "codex", "session-1").unwrap();
        assert!(lock(&config, "codex", "session-1").is_err());
        let other_session = lock(&config, "codex", "session-2").unwrap();
        let other_agent = lock(&config, "pi", "session-1").unwrap();
        drop(first);
        // Parallel native-command tests may briefly inherit this open file description across fork.
        // CLOEXEC releases it at exec; still require bounded release instead of serializing the suite.
        let release_deadline = std::time::Instant::now() + Duration::from_secs(1);
        let _reacquired = loop {
            match lock(&config, "codex", "session-1") {
                Ok(guard) => break guard,
                Err(error) => {
                    assert!(
                        std::time::Instant::now() < release_deadline,
                        "session lock was not released: {error:#}"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        drop((other_session, other_agent));
    }
    #[test]
    fn registration_is_private_idempotent_and_keeps_only_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("runner.yaml");
        std::fs::write(&file, "secret values must not be copied").unwrap();
        let registry = dir.path().join("registry");
        register_at(&file, &registry).unwrap();
        register_at(&file, &registry).unwrap();
        assert_eq!(
            registered_configs_at(&registry).unwrap(),
            vec![file.canonicalize().unwrap()]
        );
        assert!(
            !std::fs::read_to_string(registry.join("runners.json"))
                .unwrap()
                .contains("secret")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(registry.join("runners.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
    #[tokio::test]
    async fn unknown_session_stays_busy_until_verified() {
        let dir = tempfile::tempdir().unwrap();
        let config = runner(dir.path());
        std::fs::create_dir_all(&config.data_dir).unwrap();
        let pool = sqlx::SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(config.data_dir.join("journal.sqlite"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE local_runs(id TEXT PRIMARY KEY,phase TEXT,session TEXT,assignment TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let run_id = Uuid::new_v4();
        let session = RunSession {
            id: "ses_1".into(),
            runner_id: config.id,
            agent: "codex".into(),
            cwd: dir.path().to_string_lossy().into_owned(),
            ready: true,
            reason: None,
            command: None,
            title: None,
        };
        let assignment = serde_json::json!({"version":1,"run_id":run_id,"runner_id":config.id,"deadline":chrono::Utc::now(),"task":{"version":1,"id":"fixture","name":"fixture","runner":"fixture","agent":"codex","cwd":session.cwd,"prompt":"private prompt","parameters":{},"session":{"id":"ses_1"},"schedule":null,"enabled":true,"execution":{"timeout":"1h","start_grace":"1h","overlap":"skip","misfire":"latest"}}});
        sqlx::query("INSERT INTO local_runs VALUES(?,?,?,?)")
            .bind(run_id.to_string())
            .bind("unknown")
            .bind(serde_json::to_string(&session).unwrap())
            .bind(assignment.to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert!(ensure_idle(&config, &session, None).await.is_err());
        assert!(ensure_idle(&config, &session, Some(run_id)).await.is_ok());
        sqlx::query("UPDATE local_runs SET phase='done'")
            .execute(&pool)
            .await
            .unwrap();
        ensure_idle(&config, &session, None).await.unwrap();
        let occupancy = mark_continuation(&config, run_id, &session).unwrap();
        // A killed CLI cannot release its file lock; the durable marker still prevents reuse.
        assert!(ensure_idle(&config, &session, Some(run_id)).await.is_err());
        assert!(mark_continuation(&config, run_id, &session).is_err());
        std::fs::remove_file(occupancy).unwrap();
        ensure_idle(&config, &session, None).await.unwrap();
        pool.close().await;
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn continuation_returns_native_exit_code_and_never_replays_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let task: TaskConfig = serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","runner":"fixture","agent":"codex","cwd":dir.path(),"prompt":"","session":{"id":"ses_1"},"schedule":null})).unwrap();
        let command: CommandSpec = serde_json::from_value(serde_json::json!({"command":"/bin/sh","args":["-c","test -z \"$1\" && exit 7", "fixture", "{{task.prompt}}"]})).unwrap();
        assert_eq!(
            interactive(&command, &task, Uuid::new_v4()).await.unwrap(),
            7
        );
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn inspection_requires_explicit_persisted_session_without_task_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let task: TaskConfig = serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","runner":"fixture","agent":"fixture","cwd":dir.path(),"prompt":"private task instructions","schedule":null})).unwrap();
        let info = SessionInfo {
            version: 1,
            id: "ses_1".into(),
            cwd: dir.path().to_string_lossy().into_owned(),
            persisted: true,
            title: None,
        };
        let check = |info: &SessionInfo| serde_json::json!({"command":"/bin/sh","args":["-c","test -z \"$1\" || exit 9; printf '%s' \"$2\"","fixture","{{task.prompt}}",serde_json::to_string(info).unwrap()]});
        let make_agent = |info: &SessionInfo| -> AgentConfig {
            serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","mode":"sync","submit":{"command":"unused"},"session":{"inspect":check(info),"resume":{"command":"unused"},"interactive":{"command":"unused"}}})).unwrap()
        };
        assert_eq!(
            inspect(&make_agent(&info), &task, Uuid::new_v4(), "ses_1")
                .await
                .unwrap()
                .id,
            "ses_1"
        );
        let mut invalid = info.clone();
        invalid.persisted = false;
        assert!(
            inspect(&make_agent(&invalid), &task, Uuid::new_v4(), "ses_1")
                .await
                .is_err()
        );
        invalid = info.clone();
        invalid.id = "ses_other".into();
        assert!(
            inspect(&make_agent(&invalid), &task, Uuid::new_v4(), "ses_1")
                .await
                .is_err()
        );
        invalid = info;
        invalid.cwd = dir.path().parent().unwrap().to_string_lossy().into_owned();
        assert!(
            inspect(&make_agent(&invalid), &task, Uuid::new_v4(), "ses_1")
                .await
                .is_err()
        );
    }
    #[test]
    #[ignore = "invoked only by the cross-process locking test"]
    fn subprocess_lock_probe() {
        let path =
            std::env::var_os("WAKEIT_TEST_SESSION_LOCK_CONFIG").expect("probe configuration");
        let config: RunnerConfig = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert!(lock(&config, "codex", "session-1").is_err());
    }
    #[test]
    fn another_process_cannot_take_held_session_lock() {
        let dir = tempfile::tempdir().unwrap();
        let config = runner(dir.path());
        let path = dir.path().join("config.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let _guard = lock(&config, "codex", "session-1").unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "sessions::tests::subprocess_lock_probe",
            ])
            .env("WAKEIT_TEST_SESSION_LOCK_CONFIG", path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    #[test]
    fn native_command_export_has_explicit_session_and_no_prompt_or_environment() {
        let dir = tempfile::tempdir().unwrap();
        let task: TaskConfig = serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","runner":"fixture","agent":"fixture","cwd":dir.path(),"prompt":"PRIVATE TASK INSTRUCTIONS","parameters":{"model":"provider/model"},"schedule":null})).unwrap();
        let mut agent: AgentConfig = serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","mode":"sync","parameters":{"model":{"type":"string"}},"submit":{"command":"unused"},"session":{"inspect":{"command":"unused"},"resume":{"command":"unused"},"interactive":{"command":"native-agent","args":["resume","{{session.id}}",{"when_set":"model","args":["--model","{{parameters.model}}"]}]}}})).unwrap();
        let command = continuation_command(&agent, &task, Uuid::new_v4(), "ses_fixture").unwrap();
        assert!(
            command.contains("native-agent")
                && command.contains("ses_fixture")
                && command.contains("provider/model")
        );
        assert!(!command.contains(&task.prompt) && !command.contains("wakeit run resume"));
        agent
            .session
            .as_mut()
            .unwrap()
            .interactive
            .env
            .insert("SECRET".into(), "private-value".into());
        assert!(continuation_command(&agent, &task, Uuid::new_v4(), "ses_fixture").is_err());
        agent.session.as_mut().unwrap().interactive.env.clear();
        agent
            .session
            .as_mut()
            .unwrap()
            .interactive
            .env_from
            .insert("SECRET".into(), "DO_NOT_READ_THIS_ENV".into());
        assert!(continuation_command(&agent, &task, Uuid::new_v4(), "ses_fixture").is_err());
        agent.session.as_mut().unwrap().interactive.env_from.clear();
        agent
            .session
            .as_mut()
            .unwrap()
            .interactive
            .args
            .push(CommandArg::Literal("{{ task.prompt }}".into()));
        assert!(continuation_command(&agent, &task, Uuid::new_v4(), "ses_fixture").is_err());
    }
    #[test]
    fn powershell_export_quotes_literals_and_stops_on_directory_error() {
        let command = format_continuation(
            "C:\\O'Brien\\a $b",
            &[
                "C:\\Tools\\agent.exe".into(),
                "resume".into(),
                "ses_x".into(),
            ],
            true,
        )
        .unwrap();
        assert_eq!(
            command,
            "& { Set-Location -LiteralPath 'C:\\O''Brien\\a $b' -ErrorAction Stop; & 'C:\\Tools\\agent.exe' 'resume' 'ses_x' }"
        );
        assert!(
            format_continuation("/ok", &["agent".into(), "invalid\nargument".into()], false)
                .is_err()
        );
    }
    #[test]
    fn powershell_export_uses_native_drive_and_unc_paths() {
        let command =
            format_continuation(r"\\?\C:\项目", &[r"\\?\C:\Tools\agent.exe".into()], true).unwrap();
        assert!(command.contains(r"'C:\项目'") && command.contains(r"'C:\Tools\agent.exe'"));
        assert!(!command.contains(r"\\?\"));
        let command =
            format_continuation(r"\\?\UNC\server\share\project", &["agent".into()], true).unwrap();
        assert!(command.contains(r"'\\server\share\project'"));
    }
    #[cfg(unix)]
    #[test]
    fn posix_export_preserves_arguments_and_never_executes_after_failed_cd() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().join("项目 O'Brien $folder");
        std::fs::create_dir(&cwd).unwrap();
        let marker = dir.path().join("must-not-exist");
        let value = format!(
            "value '; touch {} # $(whoami) \"quoted\" & end",
            marker.display()
        );
        let argv = vec![
            "/bin/sh".into(),
            "-c".into(),
            "printf '%s' \"$1\"".into(),
            "fixture".into(),
            value.clone(),
        ];
        let command = format_continuation(&cwd.to_string_lossy(), &argv, false).unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
        assert!(!marker.exists());
        let command = format_continuation(
            &dir.path().join("missing").to_string_lossy(),
            &["touch".into(), marker.to_string_lossy().into_owned()],
            false,
        )
        .unwrap();
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(!output.status.success() && !marker.exists());
    }
    #[cfg(windows)]
    #[test]
    fn powershell_failed_cd_does_not_invoke_agent() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("must-not-exist");
        let script = format!(
            "Set-Content -LiteralPath {} -Value invoked",
            quote_powershell(&marker.to_string_lossy())
        );
        let argv = vec![
            "powershell.exe".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script,
        ];
        let command =
            format_continuation(&dir.path().join("missing").to_string_lossy(), &argv, true)
                .unwrap();
        let status = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &command])
            .status()
            .unwrap();
        assert!(!status.success() && !marker.exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn rename_verifies_native_title_and_does_not_expand_title_as_template() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("session.json");
        let info = SessionInfo {
            version: 1,
            id: "ses_fixture".into(),
            cwd: dir.path().to_string_lossy().into_owned(),
            persisted: true,
            title: Some("original".into()),
        };
        std::fs::write(&file, serde_json::to_vec(&info).unwrap()).unwrap();
        let task:TaskConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","runner":"fixture","agent":"fixture","cwd":dir.path(),"prompt":"PRIVATE INSTRUCTIONS","schedule":null})).unwrap();
        let inspect = serde_json::json!({"command":"/bin/cat","args":[file]});
        let mut new = serde_json::to_value(&info).unwrap();
        new["title"] = serde_json::json!("{{session.title}}");
        let rename = serde_json::json!({"command":"/bin/sh","args":["-c","printf '%s' \"$1\" > \"$2\"","fixture",new.to_string(),file]});
        let mut agent:AgentConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"fixture","name":"fixture","mode":"sync","output":{"format":"events"},"submit":{"command":"unused"},"session":{"inspect":inspect,"rename":rename,"resume":{"command":"unused"},"interactive":{"command":"unused"}}})).unwrap();
        let title = "literal {{task.prompt}} 中文-wakeit-20261015-093000123";
        let renamed = super::rename(&agent, &task, Uuid::new_v4(), "ses_fixture", title)
            .await
            .unwrap();
        assert_eq!(renamed.title.as_deref(), Some(title));
        assert!(
            !std::fs::read_to_string(&file)
                .unwrap()
                .contains(&task.prompt)
        );
        agent.session.as_mut().unwrap().rename = None;
        assert!(
            super::rename(&agent, &task, Uuid::new_v4(), "ses_fixture", title)
                .await
                .is_ok()
        );
        assert!(
            super::rename(&agent, &task, Uuid::new_v4(), "ses_fixture", "different")
                .await
                .is_err()
        );
        let unchanged: SessionInfo =
            serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(unchanged.title.as_deref(), Some(title));
    }
}
