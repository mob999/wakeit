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

use crate::runner::config::RunnerConfig;
use crate::{
    bus,
    config::{AgentConfig, AgentMode, OutputFormat, TaskConfig, duration},
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
}
#[derive(Clone)]
struct Runner {
    config: Arc<RunnerConfig>,
    journal: Journal,
    agents: Arc<HashMap<String, AgentConfig>>,
    runtime: Uuid,
    client: reqwest::Client,
    token: Arc<String>,
    active: Arc<Mutex<HashMap<Uuid, Active>>>,
    shutdown: CancellationToken,
    run_locks: Arc<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>>,
    slots: Arc<Semaphore>,
}
impl Runner {
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
            let Some(run) = self.journal.get(id).await? else {
                continue;
            };
            let Some(mut session) = run.session else {
                continue;
            };
            if let Ok(command) = crate::runner::sessions::continuation_command(
                &run.agent,
                &run.assignment.task,
                id,
                &session.id,
            ) {
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

pub async fn inspect(agent: &AgentConfig, cwd: &Path) -> AgentInfo {
    let task = TaskConfig {
        dag: None,
        version: 1,
        id: "probe".into(),
        name: "probe".into(),
        runner: "probe".into(),
        agent: agent.id.clone(),
        cwd: cwd.to_string_lossy().into(),
        prompt: "probe".into(),
        parameters: Default::default(),
        session: None,
        schedule: None,
        enabled: false,
        execution: Default::default(),
    };
    let out = if let Some(detect) = &agent.detect {
        process::execute(
            detect,
            &task,
            Uuid::new_v4(),
            None,
            Duration::from_secs(10),
            CancellationToken::new(),
            None,
        )
        .await
        .ok()
    } else {
        None
    };
    let available = if agent.detect.is_some() {
        out.as_ref()
            .is_some_and(|o| o.code == Some(0) && o.stopped.is_none())
    } else {
        true
    };
    AgentInfo {
        id: agent.id.clone(),
        name: agent.name.clone(),
        logo: agent.logo.clone(),
        mode: agent.mode.clone(),
        digest: agent.digest(),
        available,
        version: out.map(|o| o.stdout.trim().to_owned()),
        cancellable: agent.mode == AgentMode::Sync || agent.cancel.is_some(),
        parameters: agent.parameters.clone(),
        models_supported: agent.models.is_some(),
        session_supported: agent.session.is_some(),
        session_list_supported: agent.session.as_ref().is_some_and(|s| s.list.is_some()),
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
    let agents = loaded.agents;
    let mut info = Vec::new();
    for a in agents.values() {
        info.push(inspect(a, &std::env::current_dir()?).await);
    }
    let journal = Journal::open(&config.data_dir, config.id).await?;
    let slots = Arc::new(Semaphore::new(config.concurrency));
    let runner = Runner {
        slots: slots.clone(),
        runtime: Uuid::new_v4(),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?,
        token: Arc::new(std::fs::read_to_string(&config.token_file)?),
        config: Arc::new(config),
        journal,
        agents: Arc::new(agents),
        active: Arc::new(Mutex::new(HashMap::new())),
        shutdown: CancellationToken::new(),
        run_locks: Arc::new(Mutex::new(HashMap::new())),
    };
    loop {
        match runner.post("hello",json!({"runtime_id":runner.runtime,"agents":info,"directory_roots":crate::runner::queries::directory_roots(&runner.config.allowed_roots),"concurrency":runner.config.concurrency,"info":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION"),"concurrency":runner.config.concurrency,"active":0,"web_terminal":1}})).await {Ok(_)=>break,Err(e)=>{tracing::warn!(error=%e,"waiting for device lease");tokio::time::sleep(Duration::from_secs(5)).await;}}
    }
    runner.recover().await?;
    let client = bus::connect(
        &runner.config.nats_url,
        &runner.config.credentials_file,
        Some(format!("_INBOX.r_{}", runner.config.id.simple())),
    )
    .await?;
    let queries = tokio::spawn(crate::runner::queries::serve(
        client.clone(),
        runner.config.clone(),
        runner.agents.clone(),
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
    loop {
        tokio::select! {_=r.shutdown.cancelled()=>break,msg=messages.next()=>{let Some(msg)=msg else{break};let msg=msg?;let body:Value=serde_json::from_slice(&msg.payload)?;if body["version"]!=1 || body["action"]!="cancel"{msg.ack_with(async_nats::jetstream::AckKind::Term).await.map_err(|e|anyhow::anyhow!(e.to_string()))?;continue}let id:Uuid=body["run_id"].as_str().context("cancel run_id")?.parse()?;r.journal.cancel(id).await?;if let Some(a)=r.active.lock().await.get(&id){a.cancel.cancel();}msg.double_ack().await.map_err(|e|anyhow::anyhow!(e.to_string()))?;}}
    }
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
