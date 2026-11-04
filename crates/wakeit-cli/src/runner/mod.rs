pub mod agent_backend;
mod catalog;
pub mod config;
mod execution;
pub mod image_preview;
mod interactive;
pub mod journal;
pub mod queries;
pub mod result_files;
pub(crate) mod service;
pub mod sessions;
mod terminal;

use self::agent_backend::{Agent, AgentOperation, AgentReply};
use crate::runner::config::RunnerConfig;
use crate::{
    bus,
    config::{AgentMode, OutputFormat, TaskConfig, duration},
    process::{self, LogSink},
    protocol::{self, AgentInfo, Assignment},
    runner::journal::{Journal, LocalRun},
};
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
struct Active {
    cancel: CancellationToken,
    stop: CancellationToken,
    reconcile: Arc<tokio::sync::Notify>,
    deadline: tokio::time::Instant,
}
#[derive(Clone)]
struct Runner {
    config: Arc<RunnerConfig>,
    journal: Journal,
    extensions: crate::plugins::extensions::Runtime,
    agents: Arc<HashMap<String, Agent>>,
    runtime: Uuid,
    client: reqwest::Client,
    token: Arc<String>,
    active: Arc<Mutex<HashMap<Uuid, Active>>>,
    shutdown: CancellationToken,
    run_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
    slots: Arc<Semaphore>,
}
impl Runner {
    fn control_target(body: &Value) -> Result<Uuid> {
        ensure!(
            body["version"] == 1 && matches!(body["action"].as_str(), Some("cancel" | "reconcile")),
            "unsupported Runner control"
        );
        body["run_id"]
            .as_str()
            .context("control run_id")?
            .parse()
            .context("invalid control Run identity")
    }
    async fn apply_control(&self, body: &Value) -> Result<()> {
        let id = Self::control_target(body)?;
        if body["action"] == "reconcile" {
            return self.reconcile_run(id).await;
        }
        if let Some(active) = self.active.lock().await.get(&id) {
            active.cancel.cancel();
        }
        self.journal.cancel(id).await
    }
    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let r = self
            .client
            .post(format!(
                "{}/api/v1/device/{path}",
                self.config.gateway.trim_end_matches('/')
            ))
            .bearer_auth(self.token.trim())
            .json(&body)
            .send()
            .await?;
        let status = r.status();
        let value: Value = r.json().await?;
        ensure!(status.is_success(), "gateway {status}: {}", value["error"]);
        Ok(value)
    }
    fn sink(&self, id: Uuid, stdout: bool) -> Option<LogSink> {
        Some(LogSink {
            journal: self.journal.clone(),
            id,
            stdout,
        })
    }
    async fn state(
        &self,
        id: Uuid,
        state: &str,
        reason: Option<&str>,
        result: Option<Value>,
        phase: Option<&str>,
    ) -> Result<()> {
        self.journal
            .emit(
                id,
                "state",
                json!({"state":state,"reason":reason,"result":result}),
                phase,
            )
            .await
    }
    async fn recover(&self) -> Result<()> {
        for id in self.journal.ids().await? {
            let r = self.journal.get(id).await?.context("local run")?;
            if matches!(r.phase.as_str(), "claimed" | "started")
                && (r.agent.mode == AgentMode::Sync || r.job.is_none())
            {
                self.state(
                    id,
                    "unknown",
                    Some("runner restarted; previous command cannot safely be resubmitted"),
                    None,
                    Some("unknown"),
                )
                .await?;
            }
        }
        // Existing recorded sessions can gain native commands from their local snapshots.
        // Runs with no session metadata are deliberately left untouched.
        let saved:Vec<String>=sqlx::query_scalar("SELECT id FROM local_runs WHERE phase='done' AND session IS NOT NULL AND json_extract(session,'$.ready')=1 AND json_extract(session,'$.command') IS NULL LIMIT 100")
            .fetch_all(&self.journal.pool).await?;
        for saved in saved {
            let id: Uuid = saved.parse()?;
            let Some(mut run) = self.journal.get(id).await? else {
                continue;
            };
            run.agent.bind_saved_runner(&self.config.data_dir)?;
            let Some(mut session) = run.session else {
                continue;
            };
            if let Ok(command) = crate::runner::sessions::resolved_continuation_command(
                &run.agent,
                &run.assignment.task,
                id,
                &session.id,
            )
            .await
            {
                session.command = Some(command);
                self.journal.save_session(id, session).await?;
            }
        }
        Ok(())
    }
    async fn heartbeat(&self) -> Result<()> {
        self.run_locks
            .lock()
            .await
            .retain(|_, lock| Arc::strong_count(lock) > 1);
        let mut claims = self.journal.claims().await?;
        {
            let active = self.active.lock().await;
            for claim in &mut claims {
                let id = claim["run_id"]
                    .as_str()
                    .and_then(|s| s.parse::<Uuid>().ok());
                claim["monitoring"] = json!(id.is_some_and(|id| active.contains_key(&id)));
            }
        }
        let active = self.config.concurrency - self.slots.available_permits();
        let response=self.post("heartbeat",json!({"runtime_id":self.runtime,"claims":claims,"info":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION"),"concurrency":self.config.concurrency,"active":active,"web_terminal":1,"unknown":self.journal.unknown_count().await?}})).await?;
        for id in response["closed"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|s| s.parse::<Uuid>().ok())
        {
            if let Some(a) = self.active.lock().await.get(&id) {
                a.stop.cancel();
            }
            self.journal.phase(id, "done", None).await?;
        }
        Ok(())
    }
}
type UtcNow = chrono::Utc;

pub async fn inspect(agent: impl Into<Agent>, cwd: &Path) -> AgentInfo {
    let agent = agent.into();
    let task: TaskConfig = serde_json::from_value(json!({"version":1,"id":"probe","name":"probe","runner":"probe","agent":agent.id,"cwd":cwd,"prompt":"","parameters":{}})).expect("probe task");
    let (available, version) = if let Some(legacy) = agent.legacy() {
        if let Some(detect) = &legacy.detect {
            let out = process::execute(
                detect,
                &task,
                Uuid::new_v4(),
                None,
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await;
            (
                out.as_ref()
                    .is_ok_and(|o| o.code == Some(0) && o.stopped.is_none() && o.confirmed),
                out.ok().map(|o| o.stdout.trim().to_owned()),
            )
        } else {
            (true, None)
        }
    } else {
        let request = agent.request(AgentOperation::Detect, &task, None, Duration::from_secs(10));
        match agent.invoke(request, CancellationToken::new(), None).await {
            Ok(result) if result.confirmed && result.stopped.is_none() => match result.reply {
                AgentReply::Detection(info) => (info.available, info.version),
                _ => (false, None),
            },
            _ => (false, None),
        }
    };
    AgentInfo {
        id: agent.id.clone(),
        name: agent.name.clone(),
        logo: agent.logo.clone(),
        mode: agent.mode.clone(),
        digest: agent.digest(),
        available,
        version,
        cancellable: agent.mode == AgentMode::Sync || agent.supports(AgentOperation::Cancel),
        parameters: agent.parameters.clone(),
        models_supported: agent.supports(AgentOperation::Models),
        session_supported: agent.session_supported(),
        session_list_supported: agent.supports(AgentOperation::SessionList),
        trace_supported: matches!(agent.output.format, OutputFormat::Events),
    }
}
pub async fn start(config: RunnerConfig) -> Result<()> {
    ensure!(
        config.version == 1 && config.concurrency > 0 && config.concurrency <= 128,
        "invalid runner configuration"
    );
    std::fs::create_dir_all(&config.data_dir)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config.data_dir.join("runner.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("another runner owns this data directory")?;
    let loaded = crate::plugins::load_agents(&config.agents, config.bundled_agents)?;
    crate::plugins::record_loaded(&config, &loaded.snapshot)?;
    let mut agents = loaded.agents;
    for (id, agent) in &mut agents {
        agent.bind_runner(&config.data_dir, config.agent_instances.get(id).copied())?;
    }
    let journal = Journal::open(&config.data_dir, config.id).await?;
    let slots = Arc::new(Semaphore::new(config.concurrency));
    let config = Arc::new(config);
    let shutdown = CancellationToken::new();
    let extensions =
        crate::plugins::extensions::Runtime::new(config.clone(), journal.clone(), shutdown.clone())
            .await?;
    let mut info = Vec::new();
    for agent in agents.values() {
        info.push(inspect(agent, &std::env::current_dir()?).await);
    }
    let runner = Runner {
        slots: slots.clone(),
        runtime: Uuid::new_v4(),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?,
        token: Arc::new(std::fs::read_to_string(&config.token_file)?),
        config,
        extensions,
        journal,
        agents: Arc::new(agents),
        active: Arc::new(Mutex::new(HashMap::new())),
        shutdown,
        run_locks: Arc::new(Mutex::new(HashMap::new())),
    };
    let extension_protocol = loop {
        match runner.post("hello",json!({"runtime_id":runner.runtime,"agents":info,"directory_roots":crate::runner::queries::directory_roots(&runner.config.allowed_roots),"concurrency":runner.config.concurrency,"info":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION"),"concurrency":runner.config.concurrency,"active":0,"web_terminal":1}})).await {
            Ok(reply)=>{
                if let Some(jwt)=reply["nats_jwt"].as_str(){
                    let existing=std::fs::read_to_string(&runner.config.credentials_file)?;
                    let seed=existing.lines().find(|line|line.starts_with("SU") && nkeys::KeyPair::from_seed(line).is_ok()).context("Runner NKey seed missing")?;
                    crate::connection::atomic(&runner.config.credentials_file,wakeit_core::credentials::credentials(jwt,seed))?;
                }
                break reply["extension_protocol"].as_u64().unwrap_or(0);
            }
            Err(e)=>{tracing::warn!(error=%e,"waiting for device lease");tokio::time::sleep(Duration::from_secs(5)).await;}
        }
    };
    runner.recover().await?;
    let client = bus::connect(
        &runner.config.nats_url,
        &runner.config.credentials_file,
        Some(format!("_INBOX.r_{}", runner.config.id.simple())),
    )
    .await?;
    let extension_host = runner.extensions.clone();
    let extension_client = client.clone();
    let runtime = runner.runtime;
    let extensions = tokio::spawn(async move {
        if extension_protocol == 1 {
            extension_host.serve(extension_client, runtime).await
        } else {
            Ok(())
        }
    });
    let queries = tokio::spawn(crate::runner::queries::serve(
        client.clone(),
        runner.config.clone(),
        runner.agents.clone(),
        runner.journal.clone(),
        runner.runtime,
        runner.shutdown.clone(),
    ));
    let terminal_runtime = crate::runner::terminal::Runtime::new(
        runner.config.clone(),
        runner.journal.clone(),
        runner.runtime,
        slots,
    )
    .await?;
    let terminals = tokio::spawn(terminal_runtime.serve(client.clone(), runner.shutdown.clone()));
    let js = async_nats::jetstream::new(client);
    let flush = tokio::spawn({
        let r = runner.clone();
        let js = js.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_millis(200));
            loop {
                tokio::select! {_=r.shutdown.cancelled()=>break,_=tick.tick()=>{if let Err(e)=r.journal.flush(&js).await{tracing::warn!(error=%e,"event upload deferred");}}}
            }
        }
    });
    let heartbeats = tokio::spawn({
        let r = runner.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {_=r.shutdown.cancelled()=>break,_=tick.tick()=>{if let Err(e)=r.heartbeat().await{tracing::warn!(error=%e,"heartbeat deferred");}}}
            }
        }
    });
    let controls = tokio::spawn({
        let r = runner.clone();
        let js = js.clone();
        async move {
            loop {
                if r.shutdown.is_cancelled() {
                    break;
                }
                if let Err(e) = control_loop(&r, &js).await {
                    tracing::warn!(error=%e,"control connection retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
    let maintenance = tokio::spawn({
        let r = runner.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tokio::select! {_=r.shutdown.cancelled()=>break,_=tick.tick()=>{if let Err(e)=r.journal.prune().await{tracing::warn!(error=%e,"local retention deferred");}}}
            }
        }
    });
    let work = tokio::spawn({
        let r = runner.clone();
        let js = js.clone();
        async move {
            loop {
                if r.shutdown.is_cancelled() {
                    break;
                }
                if let Err(e) = work_loop(&r, &js).await {
                    tracing::warn!(error=%e,"work connection retrying");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
    tracing::info!(runner=%runner.config.name,"wakeit runner ready");
    wakeit_core::shutdown::shutdown_signal().await;
    // Stop accepting work. Sync commands receive cancellation; async jobs remain recoverable.
    runner.shutdown.cancel();
    for a in runner.active.lock().await.values() {
        a.cancel.cancel();
    }
    let _ = tokio::join!(
        flush,
        heartbeats,
        controls,
        work,
        maintenance,
        queries,
        extensions,
        terminals
    );
    drop(lock);
    Ok(())
}
async fn control_loop(r: &Runner, js: &async_nats::jetstream::Context) -> Result<()> {
    let consumer = js
        .get_stream("WAKEIT_CONTROL")
        .await?
        .get_consumer::<async_nats::jetstream::consumer::pull::Config>(&protocol::consumer(
            r.config.id,
            "control",
        ))
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let mut messages = consumer.messages().await?;
    let slots = Arc::new(Semaphore::new(8));
    let mut observations = tokio::task::JoinSet::new();
    loop {
        while let Some(result) = observations.try_join_next() {
            if let Err(error) = result {
                tracing::warn!(%error,"reconciliation worker stopped");
            }
        }
        let msg = tokio::select! {_=r.shutdown.cancelled()=>break,msg=messages.next()=>match msg{Some(msg)=>msg?,None=>break}};
        let body: Value = serde_json::from_slice(&msg.payload)?;
        let Ok(id) = Runner::control_target(&body) else {
            msg.ack_with(async_nats::jetstream::AckKind::Term)
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            continue;
        };
        if body["action"] == "cancel" {
            r.apply_control(&body).await?;
            msg.double_ack()
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        } else {
            let Ok(permit) = slots.clone().try_acquire_owned() else {
                msg.ack_with(async_nats::jetstream::AckKind::Nak(Some(
                    Duration::from_secs(3),
                )))
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                continue;
            };
            let runner = r.clone();
            observations.spawn(async move {
                let _permit = permit;
                if let Err(error) = runner.apply_control(&body).await {
                    tracing::warn!(run_id=%id,%error,"Run reconciliation was not completed");
                    return;
                }
                let _ = msg.double_ack().await;
            });
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(12), async {
        while observations.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}
async fn work_loop(r: &Runner, js: &async_nats::jetstream::Context) -> Result<()> {
    let consumer = js
        .get_stream("WAKEIT_WORK")
        .await?
        .get_consumer::<async_nats::jetstream::consumer::pull::Config>(&protocol::consumer(
            r.config.id,
            "work",
        ))
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let slots = r.slots.clone();
    let mut jobs = tokio::task::JoinSet::new();
    loop {
        if r.shutdown.is_cancelled() {
            break;
        }
        while let Some(result) = jobs.try_join_next() {
            if let Err(e) = result {
                tracing::error!(error=%e,"execution worker panicked");
            }
        }
        if r.journal.unknown_count().await? + (r.config.concurrency - slots.available_permits())
            >= r.config.concurrency
        {
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }
        let mut messages = match consumer
            .fetch()
            .max_messages(1)
            .expires(Duration::from_secs(2))
            .messages()
            .await
        {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error=%e,"task fetch deferred");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        if let Some(msg) = messages.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error=%e,"task delivery deferred");
                    continue;
                }
            };
            let permit = tokio::select! {_=r.shutdown.cancelled()=>break,p=slots.clone().acquire_owned()=>p?};
            let r = r.clone();
            jobs.spawn(async move {
                if let Err(e) = r.handle(msg, permit).await {
                    tracing::error!(error=%e,"run handling failed; message remains unacknowledged");
                }
            });
        }
    }
    // Workers persist state before acknowledging; give sync cancellation a bounded grace.
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while jobs.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}

pub async fn enroll(
    gateway: &str,
    name: &str,
    token: &str,
    dir: &Path,
    roots: &[PathBuf],
) -> Result<PathBuf> {
    ensure!(
        !dir.join("runner.yaml").exists(),
        "runner configuration already exists"
    );
    std::fs::create_dir_all(dir.join("agents"))?;
    let dir = dir.canonicalize()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let key = nkeys::KeyPair::new_user();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let response = client
        .post(format!(
            "{}/api/v1/device/enroll",
            gateway.trim_end_matches('/')
        ))
        .json(&json!({"token":token,"name":name,"public_key":key.public_key()}))
        .send()
        .await?;
    let status = response.status();
    let b: Value = response.json().await?;
    ensure!(status.is_success(), "enrollment {status}: {}", b["error"]);
    wakeit_core::credentials::private_write(
        &dir.join("device.token"),
        b["token"].as_str().context("device token")?,
    )?;
    wakeit_core::credentials::private_write(
        &dir.join("runner.creds"),
        wakeit_core::credentials::credentials(b["jwt"].as_str().context("jwt")?, &key.seed()?),
    )?;
    let config = RunnerConfig {
        version: 1,
        gateway: gateway.into(),
        id: b["id"].as_str().context("runner id")?.parse()?,
        name: name.into(),
        token_file: dir.join("device.token"),
        credentials_file: dir.join("runner.creds"),
        nats_url: b["nats_url"].as_str().context("nats_url")?.into(),
        data_dir: dir.join("data"),
        concurrency: 2,
        agents: Vec::new(),
        agent_instances: Default::default(),
        bundled_agents: true,
        allowed_roots: roots
            .iter()
            .map(|p| p.canonicalize())
            .collect::<std::io::Result<Vec<_>>>()?,
    };
    wakeit_core::credentials::private_write(
        &dir.join("runner.yaml"),
        yaml_serde::to_string(&config)?,
    )?;
    Ok(dir.join("runner.yaml"))
}
