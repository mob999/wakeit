use crate::{
    bus,
    config::{self, AgentConfig, AgentMode, OutputFormat, RunnerConfig, TaskConfig, duration},
    journal::{Journal, LocalRun},
    process::{self, LogSink},
    protocol::{self, AgentInfo, Assignment},
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
            if let Ok(command) = crate::sessions::continuation_command(
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
    async fn handle(&self, msg: async_nats::jetstream::Message) -> Result<()> {
        let assignment: Assignment = serde_json::from_slice(&msg.payload)?;
        ensure!(
            assignment.version == 1 && assignment.runner_id == self.config.id,
            "invalid assignment"
        );
        let id = assignment.run_id;
        let lock = {
            let mut locks = self.run_locks.lock().await;
            locks
                .entry(id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _execution_guard = lock.lock().await;
        if self.journal.get(id).await?.is_none() {
            let Some(agent) = self.agents.get(&assignment.task.agent) else {
                msg.ack_with(async_nats::jetstream::AckKind::Nak(Some(
                    Duration::from_secs(10),
                )))
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                return Ok(());
            };
            self.journal.insert(&assignment, agent).await?;
        }
        let run = self.journal.get(id).await?.context("journal")?;
        let progress_stop = CancellationToken::new();
        let _progress_guard = progress_stop.clone().drop_guard();
        let progress = tokio::spawn({
            let msg = msg.clone();
            let stop = progress_stop.clone();
            async move {
                let mut tick = tokio::time::interval(Duration::from_secs(15));
                loop {
                    tokio::select! {_=stop.cancelled()=>break,_=tick.tick()=>{let _=msg.ack_with(async_nats::jetstream::AckKind::Progress).await;}}
                }
            }
        });
        let result = self.execute_run(run).await;
        if let Err(e) = &result {
            if self
                .journal
                .get(id)
                .await?
                .is_some_and(|r| r.phase == "intent")
            {
                msg.ack_with(async_nats::jetstream::AckKind::Nak(Some(
                    Duration::from_secs(3),
                )))
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                return Ok(());
            }
            tracing::error!(run_id=%id,error=%e,"execution stopped with uncertain state");
            self.state(id, "unknown", Some(&e.to_string()), None, Some("unknown"))
                .await?;
        }
        // Completion is acknowledged only when all events are durably published.
        while self.journal.pending(id).await? && !self.shutdown.is_cancelled() {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        progress_stop.cancel();
        let _ = progress.await;
        if !self.shutdown.is_cancelled() {
            msg.double_ack()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Ok(())
    }
    async fn execute_run(&self, mut run: LocalRun) -> Result<()> {
        let id = run.assignment.run_id;
        if matches!(run.phase.as_str(), "done" | "unknown") {
            return Ok(());
        }
        if run.phase != "intent" && (run.agent.mode == AgentMode::Sync || run.job.is_none()) {
            self.state(
                id,
                "unknown",
                Some("previous command start cannot safely be replayed"),
                None,
                Some("unknown"),
            )
            .await?;
            return Ok(());
        }
        if run.phase == "intent" {
            let reply=self.post("claim",json!({"run_id":id,"claim_id":run.claim,"runtime_id":self.runtime,"agent_digest":run.agent.digest()})).await?;
            if reply["execute"] != true {
                if reply["state"].as_str().is_some_and(protocol::terminal) {
                    self.journal.phase(id, "done", None).await?;
                } else {
                    self.state(
                        id,
                        "unknown",
                        Some("execution claim belongs to an earlier runner process"),
                        None,
                        Some("unknown"),
                    )
                    .await?;
                }
                return Ok(());
            }
            if reply["cancel_requested"] == true {
                self.journal.cancel(id).await?;
            }
            self.journal
                .title(id, reply["session_title"].as_str())
                .await?;
            self.journal.phase(id, "claimed", None).await?;
            run = self.journal.get(id).await?.context("claimed journal")?;
            if let Err(e) =
                process::validate_cwd(&run.assignment.task.cwd, &self.config.allowed_roots)
            {
                self.state(id, "failed", Some(&e.to_string()), None, Some("done"))
                    .await?;
                return Ok(());
            }
        }
        match config::resolve_parameters(&run.agent.parameters, &run.assignment.task.parameters) {
            Ok(values) => run.assignment.task.parameters = values,
            Err(e) => {
                self.state(id, "failed", Some(&e.to_string()), None, Some("done"))
                    .await?;
                return Ok(());
            }
        }
        run.assignment.task.cwd = std::path::Path::new(&run.assignment.task.cwd)
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        self.journal.resolved(&run.assignment).await?;
        self.journal.emit(id,"snapshot",json!({"parameters":run.assignment.task.parameters,"cwd":run.assignment.task.cwd,"agent_digest":run.agent.digest(),"trace_supported":matches!(run.agent.output.format,OutputFormat::Events),"session_supported":run.agent.session.is_some(),"session_title":run.session_title,"model":run.assignment.task.parameters.get("model").map(String::as_str).unwrap_or("Agent 默认")}),None).await?;
        if self.journal.cancelled(id).await? {
            self.state(
                id,
                "cancelled",
                Some("cancelled before command start"),
                None,
                Some("done"),
            )
            .await?;
            return Ok(());
        }
        let _session_guard = if let Some(binding) = &run.assignment.task.session {
            let session = crate::sessions::RunSession {
                id: binding.id.clone(),
                runner_id: self.config.id,
                agent: run.agent.id.clone(),
                cwd: run.assignment.task.cwd.clone(),
                ready: false,
                reason: None,
                title: None,
                command: crate::sessions::continuation_command(
                    &run.agent,
                    &run.assignment.task,
                    id,
                    &binding.id,
                )
                .ok(),
            };
            let guard: Result<_> = async {
                let guard = crate::sessions::lock(&self.config, &session.agent, &session.id)?;
                crate::sessions::ensure_idle(&self.config, &session, Some(id)).await?;
                Ok(guard)
            }
            .await;
            let guard = match guard {
                Ok(guard) => guard,
                Err(error) => {
                    let state = if error.is::<crate::sessions::SessionBusy>() {
                        "skipped"
                    } else {
                        "failed"
                    };
                    self.state(id, state, Some(&format!("{error:#}")), None, Some("done"))
                        .await?;
                    return Ok(());
                }
            };
            match crate::sessions::inspect(&run.agent, &run.assignment.task, id, &binding.id).await
            {
                Ok(info) => {
                    self.journal
                        .save_session(
                            id,
                            crate::sessions::RunSession {
                                cwd: info.cwd,
                                title: info.title,
                                ready: true,
                                ..session
                            },
                        )
                        .await?
                }
                Err(error) => {
                    self.state(
                        id,
                        "failed",
                        Some(&format!("session inspection failed: {error:#}")),
                        None,
                        Some("done"),
                    )
                    .await?;
                    return Ok(());
                }
            }
            Some(guard)
        } else {
            None
        };
        let active = Active {
            cancel: CancellationToken::new(),
            stop: CancellationToken::new(),
        };
        if self.journal.cancelled(id).await? {
            active.cancel.cancel();
        }
        self.active.lock().await.insert(id, active.clone());
        let result = if run.agent.mode == AgentMode::Sync {
            self.sync(&run, &active).await
        } else {
            self.asynchronous(&run, &active).await
        };
        self.active.lock().await.remove(&id);
        result
    }
    async fn sync(&self, run: &LocalRun, active: &Active) -> Result<()> {
        let id = run.assignment.run_id;
        self.journal.phase(id, "started", None).await?;
        self.state(id, "running", None, None, None).await?;
        if active.cancel.is_cancelled() {
            self.state(
                id,
                "cancelled",
                Some("cancelled before command start"),
                None,
                Some("done"),
            )
            .await?;
            return Ok(());
        }
        let session_start_seq: i64 = sqlx::query_scalar("SELECT seq FROM local_runs WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&self.journal.pool)
            .await?;
        let result = process::execute_output_named_format(
            if run.assignment.task.session.is_some() {
                &run.agent
                    .session
                    .as_ref()
                    .context("adapter cannot resume sessions")?
                    .resume
            } else {
                &run.agent.submit
            },
            &run.assignment.task,
            id,
            None,
            duration(
                run.assignment
                    .task
                    .execution
                    .timeout
                    .as_deref()
                    .unwrap_or("1h"),
            )?,
            active.cancel.clone(),
            self.sink(id, true),
            run.agent.output.format.clone(),
            run.session_title.as_deref(),
        )
        .await;
        if run.assignment.task.session.is_some()
            && result.as_ref().is_ok_and(|out| out.stopped.is_none())
        {
            let observed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pending_events WHERE run_id=? AND seq>? AND json_extract(payload,'$.kind')='session')")
                .bind(id.to_string()).bind(session_start_seq).fetch_one(&self.journal.pool).await?;
            if !observed {
                if let Some(mut session) = self.journal.session(id).await? {
                    session.ready = false;
                    session.reason = Some("Agent did not report its session ID".into());
                    self.journal.save_session(id, session).await?;
                }
                self.state(
                    id,
                    "unknown",
                    Some("Agent did not report its session ID"),
                    None,
                    Some("unknown"),
                )
                .await?;
                return Ok(());
            }
        }
        if let Some(mut session) = self.journal.session(id).await? {
            // A recorded session conflict cannot be cleared by a later metadata probe.
            if session.reason.is_some() {
                self.state(
                    id,
                    "unknown",
                    session.reason.as_deref(),
                    None,
                    Some("unknown"),
                )
                .await?;
                return Ok(());
            }
            match crate::sessions::inspect(&run.agent, &run.assignment.task, id, &session.id).await
            {
                Ok(info) => {
                    session.cwd = info.cwd;
                    session.title = info.title;
                    session.ready = true;
                    session.reason = None;
                    if run.assignment.task.session.is_none()
                        && let Some(title) = run.session_title.as_deref()
                    {
                        match crate::sessions::rename(
                            &run.agent,
                            &run.assignment.task,
                            id,
                            &session.id,
                            title,
                        )
                        .await
                        {
                            Ok(info) => session.title = info.title,
                            Err(error) => session.reason = Some(format!("会话命名失败: {error:#}")),
                        }
                    }
                    session.command = crate::sessions::continuation_command(
                        &run.agent,
                        &run.assignment.task,
                        id,
                        &session.id,
                    )
                    .ok();
                }
                Err(error) => {
                    session.ready = false;
                    session.reason = Some(format!("{error:#}"));
                }
            }
            self.journal.save_session(id, session).await?;
        }
        match result {
            Err(e) => {
                let uncertain = e.downcast_ref::<process::CommandStartedError>().is_some();
                self.state(
                    id,
                    if uncertain { "unknown" } else { "failed" },
                    Some(&e.to_string()),
                    None,
                    Some(if uncertain { "unknown" } else { "done" }),
                )
                .await?
            }
            Ok(out) => {
                let state = if !out.confirmed {
                    "unknown"
                } else if let Some(s) = out.stopped {
                    s
                } else if out.code == Some(0) {
                    "succeeded"
                } else {
                    "failed"
                };
                let value = out.result_value(&run.agent.output);
                match value {
                    Ok(value) => {
                        self.journal.emit(id, "state", json!({
                            "state":state,
                            "reason":if !out.confirmed { Some("could not confirm process tree termination") } else if out.code != Some(0) && out.stopped.is_none() { Some("command exited unsuccessfully") } else if out.stdout_truncated { Some("Result exceeded the retained output limit; showing a preview.") } else { None },
                            "result":value,
                            "retryable":out.confirmed && matches!(state,"failed"|"timed_out"),
                        }), Some(if state=="unknown" {"unknown"} else {"done"})).await?;
                    }
                    Err(e) => {
                        self.state(
                            id,
                            if out.stopped.is_some() || !out.confirmed {
                                state
                            } else {
                                "failed"
                            },
                            Some(&format!("invalid or partial output: {e}")),
                            None,
                            Some(if state == "unknown" {
                                "unknown"
                            } else {
                                "done"
                            }),
                        )
                        .await?
                    }
                }
            }
        }
        Ok(())
    }
    async fn asynchronous(&self, run: &LocalRun, active: &Active) -> Result<()> {
        let id = run.assignment.run_id;
        let mut job = run.job.clone();
        if job.is_none() {
            if active.cancel.is_cancelled() {
                self.state(
                    id,
                    "cancelled",
                    Some("cancelled before submit"),
                    None,
                    Some("done"),
                )
                .await?;
                return Ok(());
            }
            self.journal.phase(id, "started", None).await?;
            let output = process::execute(
                &run.agent.submit,
                &run.assignment.task,
                id,
                None,
                Duration::from_secs(60),
                active.cancel.clone(),
                self.sink(id, false),
            )
            .await;
            let output = match output {
                Ok(o) => o,
                Err(e) => {
                    self.state(
                        id,
                        "unknown",
                        Some(&format!("submit could not be confirmed: {e}")),
                        None,
                        Some("unknown"),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let response = serde_json::from_str::<Value>(&output.stdout).ok();
            job = response
                .as_ref()
                .and_then(|v| v["job_id"].as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if output.code != Some(0) || output.stopped.is_some() || job.is_none() {
                self.state(
                    id,
                    "unknown",
                    Some("submit may have succeeded; no reliable job_id received"),
                    None,
                    Some("unknown"),
                )
                .await?;
                return Ok(());
            }
            self.journal.phase(id, "started", job.as_deref()).await?;
        }
        self.journal
            .emit(id, "state", json!({"state":"running","job_id":job}), None)
            .await?;
        let status = run.agent.status.as_ref().context("status required")?;
        let interval = duration(&run.agent.poll_interval)?;
        let timeout = chrono::Duration::from_std(duration(
            run.assignment
                .task
                .execution
                .timeout
                .as_deref()
                .unwrap_or("1h"),
        )?)?;
        let deadline = run.started + timeout;
        let mut cancel_sent = false;
        let mut cancel_since = None;
        let mut failures = 0u32;
        let mut unknown = false;
        loop {
            if active.stop.is_cancelled() {
                self.journal.phase(id, "done", None).await?;
                return Ok(());
            }
            if self.shutdown.is_cancelled() {
                return Ok(());
            }
            let expired = UtcNow::now() >= deadline;
            let cancellation = active.cancel.is_cancelled() || expired;
            if cancellation && !cancel_sent {
                if let Some(cancel) = &run.agent.cancel {
                    let out = process::execute(
                        cancel,
                        &run.assignment.task,
                        id,
                        job.as_deref(),
                        Duration::from_secs(30),
                        CancellationToken::new(),
                        self.sink(id, false),
                    )
                    .await;
                    cancel_sent = out
                        .as_ref()
                        .is_ok_and(|o| o.code == Some(0) && o.stopped.is_none());
                    if cancel_since.is_none() {
                        cancel_since = Some(std::time::Instant::now());
                    }
                } else if !unknown {
                    self.state(
                        id,
                        "unknown",
                        Some("adapter cannot cancel the external job"),
                        None,
                        None,
                    )
                    .await?;
                    unknown = true;
                }
            }
            let output = process::execute(
                status,
                &run.assignment.task,
                id,
                job.as_deref(),
                Duration::from_secs(10),
                CancellationToken::new(),
                None,
            )
            .await;
            match output {
                Ok(o) if o.code == Some(0) && o.stopped.is_none() => {
                    if let Ok(v) = serde_json::from_str::<Value>(&o.stdout) {
                        let state = v["state"].as_str().unwrap_or("");
                        if matches!(state, "succeeded" | "failed" | "cancelled") {
                            let state = if state == "cancelled" && expired {
                                "timed_out"
                            } else {
                                state
                            };
                            self.state(
                                id,
                                state,
                                v["message"].as_str(),
                                v.get("result").cloned(),
                                Some("done"),
                            )
                            .await?;
                            return Ok(());
                        }
                        if matches!(state, "running" | "queued") {
                            if unknown && !cancellation {
                                self.state(id, "running", None, None, None).await?;
                                unknown = false;
                            }
                            if let Some(message) = v["message"].as_str() {
                                self.journal
                                    .emit(id, "progress", json!({"message":message}), None)
                                    .await?;
                            }
                            failures = 0;
                        } else {
                            failures += 1;
                        }
                    } else {
                        failures += 1;
                    }
                }
                _ => failures += 1,
            }
            if (failures >= 5
                || cancel_since.is_some_and(|t| t.elapsed() > Duration::from_secs(60)))
                && !unknown
            {
                self.state(id,"unknown",Some("external job state or cancellation cannot be confirmed; monitoring continues"),None,None).await?;
                unknown = true;
            }
            let wait = interval
                .saturating_mul(1u32 << failures.min(4))
                .min(Duration::from_secs(60));
            if active.cancel.is_cancelled() {
                tokio::select! {_=tokio::time::sleep(wait)=>{},_=active.stop.cancelled()=>{},_=self.shutdown.cancelled()=>return Ok(())}
            } else {
                tokio::select! {_=tokio::time::sleep(wait)=>{},_=active.cancel.cancelled()=>{},_=active.stop.cancelled()=>{},_=self.shutdown.cancelled()=>return Ok(())}
            }
        }
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
    let mut agents = HashMap::new();
    for file in &config.agents {
        let mut agent: AgentConfig = config::read(file)?;
        config::normalize_commands(&mut agent)?;
        agent.validate()?;
        ensure!(!agents.contains_key(&agent.id), "duplicate agent id");
        agents.insert(agent.id.clone(), agent);
    }
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
        match runner.post("hello",json!({"runtime_id":runner.runtime,"agents":info,"directory_roots":crate::query::directory_roots(&runner.config.allowed_roots),"concurrency":runner.config.concurrency,"info":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"version":env!("CARGO_PKG_VERSION"),"concurrency":runner.config.concurrency,"active":0,"web_terminal":1}})).await {Ok(_)=>break,Err(e)=>{tracing::warn!(error=%e,"waiting for device lease");tokio::time::sleep(Duration::from_secs(5)).await;}}
    }
    runner.recover().await?;
    let client = bus::connect(
        &runner.config.nats_url,
        &runner.config.credentials_file,
        Some(format!("_INBOX.r_{}", runner.config.id.simple())),
    )
    .await?;
    let queries = tokio::spawn(crate::query::serve(
        client.clone(),
        runner.config.clone(),
        runner.agents.clone(),
        runner.runtime,
        runner.shutdown.clone(),
    ));
    let terminal_runtime = crate::terminal_runtime::Runtime::new(
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
                let _permit = permit;
                if let Err(e) = r.handle(msg).await {
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
    let mut files = Vec::new();
    for id in ["codex", "claude-code", "pi", "opencode"] {
        let file = dir.join("agents").join(format!("{id}.yaml"));
        let mut agent: AgentConfig = yaml_serde::from_str(config::builtin(id)?)?;
        config::normalize_commands(&mut agent)?;
        std::fs::write(&file, yaml_serde::to_string(&agent)?)?;
        files.push(file);
    }
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
        agents: files,
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
