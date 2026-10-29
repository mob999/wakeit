use super::*;

impl Runner {
    pub(super) async fn handle(&self, msg: async_nats::jetstream::Message) -> Result<()> {
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
        match crate::config::resolve_parameters(
            &run.agent.parameters,
            &run.assignment.task.parameters,
        ) {
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
            let session = crate::runner::sessions::RunSession {
                id: binding.id.clone(),
                runner_id: self.config.id,
                agent: run.agent.id.clone(),
                cwd: run.assignment.task.cwd.clone(),
                ready: false,
                reason: None,
                title: None,
                command: crate::runner::sessions::continuation_command(
                    &run.agent,
                    &run.assignment.task,
                    id,
                    &binding.id,
                )
                .ok(),
            };
            let guard: Result<_> = async {
                let guard =
                    crate::runner::sessions::lock(&self.config, &session.agent, &session.id)?;
                crate::runner::sessions::ensure_idle(&self.config, &session, Some(id)).await?;
                Ok(guard)
            }
            .await;
            let guard = match guard {
                Ok(guard) => guard,
                Err(error) => {
                    let state = if error.is::<crate::runner::sessions::SessionBusy>() {
                        "skipped"
                    } else {
                        "failed"
                    };
                    self.state(id, state, Some(&format!("{error:#}")), None, Some("done"))
                        .await?;
                    return Ok(());
                }
            };
            match crate::runner::sessions::inspect(
                &run.agent,
                &run.assignment.task,
                id,
                &binding.id,
            )
            .await
            {
                Ok(info) => {
                    self.journal
                        .save_session(
                            id,
                            crate::runner::sessions::RunSession {
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
            match crate::runner::sessions::inspect(
                &run.agent,
                &run.assignment.task,
                id,
                &session.id,
            )
            .await
            {
                Ok(info) => {
                    session.cwd = info.cwd;
                    session.title = info.title;
                    session.ready = true;
                    session.reason = None;
                    if run.assignment.task.session.is_none()
                        && let Some(title) = run.session_title.as_deref()
                    {
                        match crate::runner::sessions::rename(
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
                    session.command = crate::runner::sessions::continuation_command(
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
