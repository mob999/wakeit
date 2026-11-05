use super::*;
use futures::FutureExt;

impl Runner {
    pub(super) async fn handle(
        &self,
        msg: async_nats::jetstream::Message,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<()> {
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
        // The native process has ended and its final state is durable locally.
        // Event publication has its own persistent retry loop; it must not keep
        // either an execution permit or the work consumer's ACK window occupied.
        drop(permit);
        progress_stop.cancel();
        let _ = progress.await;
        if !self.shutdown.is_cancelled() {
            msg.double_ack()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Ok(())
    }
    fn active_for(run: &LocalRun) -> Result<Active> {
        let timeout = chrono::Duration::from_std(duration(
            run.assignment
                .task
                .execution
                .timeout
                .as_deref()
                .unwrap_or("1h"),
        )?)?;
        let remaining = (run.started + timeout - chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
        Ok(Active {
            cancel: CancellationToken::new(),
            stop: CancellationToken::new(),
            reconcile: Arc::new(tokio::sync::Notify::new()),
            deadline: tokio::time::Instant::now() + remaining,
        })
    }
    async fn execute_run(&self, mut run: LocalRun) -> Result<()> {
        let id = run.assignment.run_id;
        if run.phase == "done" {
            return Ok(());
        }
        if run.phase == "unknown" {
            return self.reconcile_saved(run).await;
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
        run.agent.bind_saved_runner(&self.config.data_dir)?;
        if let Err(error) = run.agent.pin_configuration().await {
            let uncertain = run.job.is_some();
            self.state(
                id,
                if uncertain { "unknown" } else { "failed" },
                Some(&error.to_string()),
                None,
                Some(if uncertain { "unknown" } else { "done" }),
            )
            .await?;
            return Ok(());
        }
        self.journal.save_agent(id, &run.agent).await?;
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
        process::validate_cwd(&run.assignment.task.cwd, &self.config.allowed_roots)?;
        if run.phase == "started" && run.agent.mode == AgentMode::Async && run.job.is_some() {
            let active = Self::active_for(&run)?;
            if self.journal.cancelled(id).await? {
                active.cancel.cancel();
            }
            self.active.lock().await.insert(id, active.clone());
            let result = self.asynchronous(&run, &active, true).await;
            self.active.lock().await.remove(&id);
            return result;
        }
        self.journal.resolved(&run.assignment).await?;
        self.journal.emit(id,"snapshot",json!({"parameters":run.assignment.task.parameters,"cwd":run.assignment.task.cwd,"agent_digest":run.agent.digest(),"trace_supported":matches!(run.agent.output.format,OutputFormat::Events),"session_supported":run.agent.session_supported(),"session_title":run.session_title,"model":run.assignment.task.parameters.get("model").map(String::as_str).unwrap_or("Agent 默认")}),None).await?;
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
                command: crate::runner::sessions::resolved_continuation_command(
                    &run.agent,
                    &run.assignment.task,
                    id,
                    &binding.id,
                )
                .await
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
        let active = Self::active_for(&run)?;
        if self.journal.cancelled(id).await? {
            active.cancel.cancel();
        }
        self.active.lock().await.insert(id, active.clone());
        if run.assignment.task.session.is_none()
            && let Some(title) = &run.session_title
        {
            let proposal = self
                .extensions
                .hooks(
                    "session.title.prepare",
                    json!({"title":title,"run_id":id,"task_id":run.assignment.task.id}),
                    active.cancel.clone(),
                )
                .await;
            match proposal {
                Ok(value) => {
                    let title = value["title"]
                        .as_str()
                        .filter(|s| {
                            !s.trim().is_empty()
                                && s.len() <= 200
                                && !s.chars().any(char::is_control)
                        })
                        .context("invalid plugin session title");
                    match title {
                        Ok(title) => {
                            sqlx::query("UPDATE local_runs SET session_title=? WHERE id=?")
                                .bind(title)
                                .bind(id.to_string())
                                .execute(&self.journal.pool)
                                .await?;
                            run.session_title = Some(title.to_owned());
                        }
                        Err(error) => {
                            self.state(id, "failed", Some(&error.to_string()), None, Some("done"))
                                .await?;
                            self.active.lock().await.remove(&id);
                            return Ok(());
                        }
                    }
                }
                Err(error) => {
                    self.state(
                        id,
                        "failed",
                        Some(&format!("session.title.prepare: {error}")),
                        None,
                        Some("done"),
                    )
                    .await?;
                    self.active.lock().await.remove(&id);
                    return Ok(());
                }
            }
        }

        let budget = active
            .deadline
            .saturating_duration_since(tokio::time::Instant::now());
        let mut proposal = json!({"prompt":run.assignment.task.prompt,"parameters":run.assignment.task.parameters,"run_id":id,"task_id":run.assignment.task.id,"runner_id":self.config.id});
        if let Some(binding) = &run.assignment.task.session {
            let stages = ["session.bind.prepare", "session.continue.prepare"];
            for stage in stages {
                proposal["session_id"] = json!(binding.id);
                match self
                    .extensions
                    .hooks(stage, proposal.clone(), active.cancel.clone())
                    .await
                {
                    Ok(value) => proposal = value,
                    Err(error) => {
                        self.state(
                            id,
                            "failed",
                            Some(&format!("{stage}: {error}")),
                            None,
                            Some("done"),
                        )
                        .await?;
                        self.active.lock().await.remove(&id);
                        return Ok(());
                    }
                }
            }
        }
        let prepared = tokio::time::timeout(
            budget,
            self.extensions
                .hooks("execution.prepare", proposal, active.cancel.clone()),
        )
        .await;
        let prepared = match prepared {
            Ok(Ok(value)) => value,
            other => {
                let reason = match other {
                    Ok(Err(e)) => e.to_string(),
                    _ => "execution preparation timed out".into(),
                };
                self.state(
                    id,
                    if active.cancel.is_cancelled() {
                        "cancelled"
                    } else {
                        "failed"
                    },
                    Some(&format!("execution.prepare: {reason}")),
                    None,
                    Some("done"),
                )
                .await?;
                self.active.lock().await.remove(&id);
                return Ok(());
            }
        };
        let resolved: Result<(String, std::collections::BTreeMap<String, String>)> = async {
            let prompt: String = serde_json::from_value(prepared["prompt"].clone())?;
            ensure!(prompt.len() <= 1024 * 1024, "Hook prompt exceeds 1 MiB");
            let parameters = crate::config::resolve_parameters(
                &run.agent.parameters,
                &serde_json::from_value(prepared["parameters"].clone())?,
            )?;
            Ok((prompt, parameters))
        }
        .await;
        match resolved {
            Ok((prompt, parameters)) => {
                run.assignment.task.prompt = prompt;
                run.assignment.task.parameters = parameters;
            }
            Err(e) => {
                self.state(
                    id,
                    "failed",
                    Some(&format!("execution.prepare: {e}")),
                    None,
                    Some("done"),
                )
                .await?;
                self.active.lock().await.remove(&id);
                return Ok(());
            }
        }
        if tokio::time::Instant::now() >= active.deadline {
            self.state(
                id,
                "timed_out",
                Some("execution preparation exceeded its deadline"),
                None,
                Some("done"),
            )
            .await?;
            self.active.lock().await.remove(&id);
            return Ok(());
        }
        self.journal.resolved(&run.assignment).await?;
        self.journal.emit(id,"snapshot",json!({"effective_prompt":run.assignment.task.prompt,"parameters":run.assignment.task.parameters,"plugin_prepared":true}),None).await?;
        let result = if run.agent.mode == AgentMode::Sync {
            self.sync(&run, &active).await
        } else {
            self.asynchronous(&run, &active, false).await
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
        let result = run
            .agent
            .execute(
                &run.assignment.task,
                id,
                active
                    .deadline
                    .saturating_duration_since(tokio::time::Instant::now()),
                active.cancel.clone(),
                self.sink(id, true),
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
                        && run.agent.supports(AgentOperation::SessionRename)
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
                    session.command = crate::runner::sessions::resolved_continuation_command(
                        &run.agent,
                        &run.assignment.task,
                        id,
                        &session.id,
                    )
                    .await
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
                    if uncertain {
                        "unknown"
                    } else if active.cancel.is_cancelled() {
                        "cancelled"
                    } else if tokio::time::Instant::now() >= active.deadline {
                        "timed_out"
                    } else {
                        "failed"
                    },
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
                let value = match value {
                    Ok(value) if state == "succeeded" => {
                        self.publish_result(run, active, value).await
                    }
                    other => other,
                };
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
    async fn publish_result(&self, run: &LocalRun, active: &Active, value: Value) -> Result<Value> {
        let id = run.assignment.run_id;
        let native_ref = self.journal.extension_result(id, "native", &value).await?;
        let result = tokio::time::timeout_at(
            active.deadline,
            self.extensions.hooks(
                "result.publish.prepare",
                json!({"result":value,"run_id":id,"task_id":run.assignment.task.id}),
                active.cancel.clone(),
            ),
        )
        .await;
        match result {
            Ok(Ok(processed)) => {
                let effective = processed["result"].clone();
                self.journal
                    .extension_result(id, "effective", &effective)
                    .await?;
                Ok(effective)
            }
            other => {
                self.journal
                    .emit(
                        id,
                        "snapshot",
                        json!({"failure_phase":"postprocess","native_result_ref":native_ref}),
                        None,
                    )
                    .await?;
                let error = match other {
                    Ok(Err(e)) => e.to_string(),
                    _ => "result processing deadline exceeded".into(),
                };
                anyhow::bail!("result.publish.prepare: {error}")
            }
        }
    }
    pub(super) async fn reconcile_run(&self, id: Uuid) -> Result<()> {
        let run = self
            .journal
            .get(id)
            .await?
            .context("Run is not available on this Runner")?;
        ensure!(
            run.assignment.runner_id == self.config.id,
            "Run belongs to another Runner"
        );
        if run.phase == "done" {
            return Ok(());
        }
        if let Some(active) = self.active.lock().await.get(&id).cloned() {
            ensure!(
                run.agent.mode == AgentMode::Async && run.job.is_some(),
                "Run is still under native supervision"
            );
            ensure!(
                run.agent.supports(AgentOperation::Reconcile),
                "saved Agent backend does not support reconciliation"
            );
            active.reconcile.notify_one();
            return Ok(());
        }
        let lock = self
            .run_locks
            .lock()
            .await
            .entry(id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        let _guard = lock
            .try_lock()
            .context("Run is still under execution supervision")?;
        let run = self
            .journal
            .get(id)
            .await?
            .context("Run disappeared during reconciliation")?;
        if run.phase == "done" {
            return Ok(());
        }
        self.reconcile_saved(run).await
    }
    async fn reconciliation_observation(&self, run: &LocalRun) -> Result<Value> {
        let outcome = run
            .agent
            .reconcile(run, self.shutdown.child_token(), None)
            .await?;
        use super::agent_backend::ExecutionOutcome;
        let value = match outcome {
            ExecutionOutcome::Completed {
                exit_code,
                result,
                confirmed: true,
            } => {
                json!({"state":if exit_code==Some(0){"succeeded"}else{"failed"},"result":if exit_code==Some(0){wakeit_core::capture::bounded_value(result,crate::trace::RECORD_LIMIT-4096)}else{Value::Null}})
            }
            ExecutionOutcome::Cancelled { confirmed: true } => json!({"state":"cancelled"}),
            ExecutionOutcome::NotStarted { message } => json!({"state":"failed","message":message}),
            ExecutionOutcome::Running { job_id } | ExecutionOutcome::Submitted { job_id } => {
                json!({"state":"running","job_id":job_id})
            }
            ExecutionOutcome::Uncertain { reason } => json!({"state":"unknown","message":reason}),
            ExecutionOutcome::Completed {
                confirmed: false, ..
            }
            | ExecutionOutcome::Cancelled { confirmed: false } => {
                json!({"state":"unknown","message":"Agent could not confirm the reconciled execution outcome"})
            }
            ExecutionOutcome::CancelRequested { .. } => {
                json!({"state":"unknown","message":"external cancellation was requested but has not been confirmed"})
            }
        };
        Ok(value)
    }
    async fn finish_observation(
        &self,
        run: &LocalRun,
        active: &Active,
        value: &Value,
    ) -> Result<bool> {
        let id = run.assignment.run_id;
        let state = value["state"].as_str().unwrap_or("");
        if !matches!(state, "succeeded" | "failed" | "cancelled") {
            return Ok(false);
        }
        let timeout = chrono::Duration::from_std(duration(
            run.assignment
                .task
                .execution
                .timeout
                .as_deref()
                .unwrap_or("1h"),
        )?)?;
        let state = if state == "cancelled" && UtcNow::now() >= run.started + timeout {
            "timed_out"
        } else {
            state
        };
        let native = value.get("result").cloned().unwrap_or(Value::Null);
        let result = if state == "succeeded" {
            self.publish_result(run, active, native).await
        } else {
            Ok(native)
        };
        match result {
            Ok(result) => {
                self.state(
                    id,
                    state,
                    value["message"].as_str(),
                    Some(result),
                    Some("done"),
                )
                .await?
            }
            Err(error) => {
                self.state(id, "failed", Some(&error.to_string()), None, Some("done"))
                    .await?
            }
        }
        Ok(true)
    }
    async fn reconcile_saved(&self, mut run: LocalRun) -> Result<()> {
        let id = run.assignment.run_id;
        ensure!(
            run.assignment.runner_id == self.config.id,
            "Run belongs to another Runner"
        );
        let observation: Result<Value> = async {
            run.agent.bind_saved_runner(&self.config.data_dir)?;
            run.agent.pin_configuration().await?;
            process::validate_cwd(&run.assignment.task.cwd, &self.config.allowed_roots)?;
            if let Some(reason) = run
                .session
                .as_ref()
                .and_then(|session| session.reason.as_ref())
            {
                anyhow::bail!("saved session remains unverified: {reason}");
            }
            let value = self.reconciliation_observation(&run).await?;
            if value["state"] == "running"
                && run.agent.mode == AgentMode::Async
                && run.job.is_some()
            {
                let output = run
                    .agent
                    .job_command(
                        AgentOperation::Poll,
                        &run.assignment.task,
                        id,
                        run.job.as_deref(),
                        Duration::from_secs(10),
                        self.shutdown.child_token(),
                        None,
                    )
                    .await?;
                ensure!(
                    output.confirmed && output.stopped.is_none(),
                    "job status cleanup could not be confirmed"
                );
                return serde_json::from_str(&output.stdout)
                    .context("invalid reconciled job status");
            }
            Ok(value)
        }
        .await;
        // Verification/postprocessing has a bounded operation budget, never a renewed execution budget.
        let active = Active {
            cancel: self.shutdown.child_token(),
            stop: CancellationToken::new(),
            reconcile: Arc::new(tokio::sync::Notify::new()),
            deadline: tokio::time::Instant::now() + Duration::from_secs(10),
        };
        match observation {
            Ok(value) => {
                if !self.finish_observation(&run, &active, &value).await? {
                    self.state(
                        id,
                        "unknown",
                        Some(value["message"].as_str().unwrap_or(
                            "external job is still running; its final outcome remains unverified",
                        )),
                        None,
                        Some("unknown"),
                    )
                    .await?;
                }
            }
            Err(error) => {
                self.state(
                    id,
                    "unknown",
                    Some(&format!("reconciliation could not be confirmed: {error}")),
                    None,
                    Some("unknown"),
                )
                .await?
            }
        }
        self.journal
            .extension_event("execution.reconciled", json!({"run_id":id}))
            .await?;
        Ok(())
    }
    fn submit_budget(active: &Active) -> Option<Duration> {
        let remaining = active
            .deadline
            .saturating_duration_since(tokio::time::Instant::now());
        (!remaining.is_zero()).then_some(remaining.min(Duration::from_secs(60)))
    }
    async fn asynchronous(
        &self,
        run: &LocalRun,
        active: &Active,
        mut reconcile_requested: bool,
    ) -> Result<()> {
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
            let Some(submit_budget) = Self::submit_budget(active) else {
                self.state(
                    id,
                    "timed_out",
                    Some("execution deadline expired before submit"),
                    None,
                    Some("done"),
                )
                .await?;
                return Ok(());
            };
            let output = run
                .agent
                .job_command(
                    AgentOperation::Submit,
                    &run.assignment.task,
                    id,
                    None,
                    submit_budget,
                    active.cancel.clone(),
                    self.sink(id, false),
                )
                .await;
            let output = match output {
                Ok(o) => o,
                Err(e) => {
                    let not_started = e.is::<super::agent_backend::NativeNotStarted>();
                    self.state(
                        id,
                        if not_started { "failed" } else { "unknown" },
                        Some(&format!("submit could not be confirmed: {e}")),
                        None,
                        Some(if not_started { "done" } else { "unknown" }),
                    )
                    .await?;
                    return Ok(());
                }
            };
            let response = serde_json::from_str::<Value>(&output.stdout).ok();
            job = response
                .as_ref()
                .and_then(|v| v["job_id"].as_str())
                .filter(|s| !s.is_empty() && s.len() <= 4096)
                .map(str::to_owned);
            if let Some(job) = &job {
                // Keep a reliable native handle even if local SDK teardown is uncertain.
                self.journal.phase(id, "started", Some(job)).await?;
            }
            if output.code != Some(0)
                || output.stopped.is_some()
                || !output.confirmed
                || job.is_none()
            {
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
        }
        self.journal
            .emit(id, "state", json!({"state":"running","job_id":job}), None)
            .await?;
        ensure!(run.agent.supports(AgentOperation::Poll), "status required");
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
                if run.agent.supports(AgentOperation::Cancel) {
                    let out = run
                        .agent
                        .job_command(
                            AgentOperation::Cancel,
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
                        .is_ok_and(|o| o.code == Some(0) && o.stopped.is_none() && o.confirmed);
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
            reconcile_requested |= active.reconcile.notified().now_or_never().is_some();
            let reconciling = reconcile_requested && run.agent.supports(AgentOperation::Reconcile);
            reconcile_requested = false;
            let output = if reconciling {
                self.reconciliation_observation(run)
                    .await
                    .map(process::Output::job)
            } else {
                run.agent
                    .job_command(
                        AgentOperation::Poll,
                        &run.assignment.task,
                        id,
                        job.as_deref(),
                        Duration::from_secs(10),
                        self.shutdown.child_token(),
                        None,
                    )
                    .await
            };
            if reconciling && let Err(error) = &output {
                self.state(
                    id,
                    "unknown",
                    Some(&format!("reconciliation could not be confirmed: {error}")),
                    None,
                    None,
                )
                .await?;
                unknown = true;
            }
            match output {
                Ok(o) if o.code == Some(0) && o.stopped.is_none() && o.confirmed => {
                    if let Ok(v) = serde_json::from_str::<Value>(&o.stdout) {
                        let state = v["state"].as_str().unwrap_or("");
                        if self.finish_observation(run, active, &v).await? {
                            return Ok(());
                        }
                        if state == "unknown" {
                            self.state(id, "unknown", v["message"].as_str(), None, None)
                                .await?;
                            unknown = true;
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
                tokio::select! {_=tokio::time::sleep(wait)=>{},_=active.stop.cancelled()=>{},_=active.reconcile.notified()=>{reconcile_requested=true;},_=self.shutdown.cancelled()=>return Ok(())}
            } else {
                tokio::select! {_=tokio::time::sleep(wait)=>{},_=active.cancel.cancelled()=>{},_=active.stop.cancelled()=>{},_=active.reconcile.notified()=>{reconcile_requested=true;},_=self.shutdown.cancelled()=>return Ok(())}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn reconciliation_fixture(
        mode: &str,
        phase: &str,
        job: Option<&str>,
    ) -> (tempfile::TempDir, Runner, LocalRun) {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/agent-bridge");
        std::fs::create_dir_all(&directory).unwrap();
        let root = tempfile::tempdir_in(directory).unwrap();
        let agent = crate::runner::agent_backend::tests::fixture(root.path(), mode);
        let id = Uuid::new_v4();
        let runner = Uuid::new_v4();
        let token_file = root.path().join("synthetic.token");
        std::fs::write(&token_file, "synthetic-unused-token").unwrap();
        let config = Arc::new(crate::runner::config::RunnerConfig {
            version: 1,
            id: runner,
            name: "synthetic".into(),
            gateway: "http://127.0.0.1:1".into(),
            nats_url: "nats://127.0.0.1:1".into(),
            token_file,
            credentials_file: root.path().join("unused.creds"),
            data_dir: root.path().join("runtime"),
            concurrency: 2,
            agents: vec![],
            agent_instances: Default::default(),
            bundled_agents: false,
            allowed_roots: vec![root.path().canonicalize().unwrap()],
        });
        let journal = Journal::open(&config.data_dir, runner).await.unwrap();
        let shutdown = CancellationToken::new();
        let extensions = crate::plugins::extensions::Runtime::new(
            config.clone(),
            journal.clone(),
            shutdown.clone(),
        )
        .await
        .unwrap();
        let task:TaskConfig=serde_json::from_value(json!({"version":1,"id":"saved-task","name":"Saved synthetic","runner":"synthetic","agent":"synthetic","cwd":root.path(),"prompt":"saved instructions must not be resubmitted","execution":{"timeout":"2m"}})).unwrap();
        let assignment = Assignment {
            version: 1,
            run_id: id,
            runner_id: runner,
            task,
            deadline: UtcNow::now() + chrono::Duration::minutes(1),
        };
        journal.insert(&assignment, &agent).await.unwrap();
        journal.phase(id, phase, job).await.unwrap();
        sqlx::query("UPDATE local_runs SET started=? WHERE id=?")
            .bind((UtcNow::now() - chrono::Duration::seconds(30)).to_rfc3339())
            .bind(id.to_string())
            .execute(&journal.pool)
            .await
            .unwrap();
        let run = journal.get(id).await.unwrap().unwrap();
        let runner = Runner {
            config,
            journal,
            extensions,
            agents: Arc::new(HashMap::new()),
            runtime: Uuid::new_v4(),
            client: reqwest::Client::new(),
            token: Arc::new("synthetic-unused-token".into()),
            active: Arc::new(Mutex::new(HashMap::new())),
            shutdown,
            run_locks: Arc::new(Mutex::new(HashMap::new())),
            slots: Arc::new(Semaphore::new(2)),
        };
        (root, runner, run)
    }
    async fn last_state(runner: &Runner, id: Uuid) -> Value {
        let payload:String=sqlx::query_scalar("SELECT payload FROM pending_events WHERE run_id=? AND json_extract(payload,'$.kind')='state' ORDER BY seq DESC LIMIT 1").bind(id.to_string()).fetch_one(&runner.journal.pool).await.unwrap();
        serde_json::from_str::<Value>(&payload).unwrap()["data"].clone()
    }
    async fn assert_saved_execution_unchanged(runner: &Runner, before: &LocalRun) {
        let saved = runner
            .journal
            .get(before.assignment.run_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.started, before.started);
        assert_eq!(saved.claim, before.claim);
        assert_eq!(saved.job, before.job);
        assert_eq!(
            serde_json::to_value(&saved.assignment).unwrap(),
            serde_json::to_value(&before.assignment).unwrap()
        );
        assert_eq!(saved.agent.digest(), before.agent.digest());
        let prepared:i64=sqlx::query_scalar("SELECT count(*) FROM extension_events WHERE event_type IN ('execution.claimed','execution.prepared','execution.started','execution.input_submitted')").fetch_one(&runner.journal.pool).await.unwrap();
        assert_eq!(prepared, 0);
    }
    fn assert_reconcile_requests(root: &std::path::Path, before: &LocalRun) {
        let mut count = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_name().to_string_lossy().starts_with("request-") {
                continue;
            }
            let request: Value =
                serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
            assert!(matches!(
                request["operation"].as_str(),
                Some("reconcile" | "poll")
            ));
            assert!(request["context"].get("prompt").is_none());
            assert_eq!(
                request["context"]["run_id"],
                json!(before.assignment.run_id)
            );
            assert_eq!(
                request["context"]["task_id"],
                json!(before.assignment.task.id)
            );
            if let Some(job) = &before.job {
                assert_eq!(request["context"]["job_id"], json!(job));
            }
            if request["operation"] == "reconcile" {
                assert_eq!(request["input"]["started_at"], json!(before.started));
                assert_eq!(
                    request["input"]["execution_deadline"],
                    json!(before.started + chrono::Duration::minutes(2))
                );
            }
            count += 1;
        }
        assert!(
            count > 0,
            "the registered Reconcile must actually be invoked"
        );
    }
    #[tokio::test]
    async fn v1_reconcile_control_gate_reaches_the_saved_backend_and_rejects_invalid_requests() {
        let (root, runner, run) =
            reconciliation_fixture("async_reconcile_done", "unknown", Some("synthetic-job")).await;
        for body in [
            json!({"version":2,"action":"reconcile","run_id":run.assignment.run_id}),
            json!({"version":1,"action":"execute","run_id":run.assignment.run_id}),
            json!({"version":1,"action":"reconcile","run_id":"not-a-uuid"}),
        ] {
            assert!(runner.apply_control(&body).await.is_err());
        }
        let body:Value=serde_json::from_slice(&serde_json::to_vec(&json!({"version":1,"action":"reconcile","run_id":run.assignment.run_id,"agent":"not-the-saved-agent","job_id":"not-the-saved-job"})).unwrap()).unwrap();
        let mut foreign = run.assignment.clone();
        foreign.runner_id = Uuid::new_v4();
        runner.journal.resolved(&foreign).await.unwrap();
        assert!(runner.apply_control(&body).await.is_err());
        assert!(!root.path().join("operations").exists());
        runner.journal.resolved(&run.assignment).await.unwrap();
        runner.apply_control(&body).await.unwrap();
        assert_eq!(
            last_state(&runner, run.assignment.run_id).await["state"],
            "succeeded"
        );
        runner.apply_control(&body).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("operations")).unwrap(),
            "reconcile\n"
        );
        assert_saved_execution_unchanged(&runner, &run).await;
        assert_reconcile_requests(root.path(), &run);
    }
    #[tokio::test]
    async fn unknown_run_reconciliation_uses_saved_backend_and_is_idempotent_without_resubmission()
    {
        let (root, runner, run) =
            reconciliation_fixture("async_reconcile_done", "unknown", Some("synthetic-job")).await;
        runner.reconcile_run(run.assignment.run_id).await.unwrap();
        let state = last_state(&runner, run.assignment.run_id).await;
        assert_eq!(state["state"], "succeeded");
        assert_eq!(state["result"], "reconciled synthetic report");
        assert_eq!(
            runner
                .journal
                .get(run.assignment.run_id)
                .await
                .unwrap()
                .unwrap()
                .phase,
            "done"
        );
        runner.reconcile_run(run.assignment.run_id).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("operations")).unwrap(),
            "reconcile\n"
        );
        assert_saved_execution_unchanged(&runner, &run).await;
        assert_reconcile_requests(root.path(), &run);
        let results: i64 =
            sqlx::query_scalar("SELECT count(*) FROM extension_results WHERE run_id=?")
                .bind(run.assignment.run_id.to_string())
                .fetch_one(&runner.journal.pool)
                .await
                .unwrap();
        assert_eq!(
            results, 2,
            "reconciled result must traverse native/effective publication"
        );
    }
    #[tokio::test]
    async fn recovered_job_and_active_user_check_reconcile_before_poll_without_new_prepare_or_deadline()
     {
        for explicit in [false, true] {
            let (root, runner, mut run) = reconciliation_fixture(
                "async_reconcile_poll_done",
                "started",
                Some("synthetic-job"),
            )
            .await;
            run.agent.poll_interval = "1s".into();
            if explicit {
                let active = Runner::active_for(&run).unwrap();
                runner
                    .active
                    .lock()
                    .await
                    .insert(run.assignment.run_id, active.clone());
                runner.reconcile_run(run.assignment.run_id).await.unwrap();
                runner.asynchronous(&run, &active, false).await.unwrap();
                runner.active.lock().await.remove(&run.assignment.run_id);
            } else {
                runner.execute_run(run.clone()).await.unwrap();
            }
            assert_eq!(
                std::fs::read_to_string(root.path().join("operations")).unwrap(),
                "reconcile\npoll\n"
            );
            let state = last_state(&runner, run.assignment.run_id).await;
            assert_eq!(state["state"], "succeeded");
            assert_eq!(state["result"], "polled synthetic report");
            assert_saved_execution_unchanged(&runner, &run).await;
            assert_reconcile_requests(root.path(), &run);
        }
    }
    #[tokio::test]
    async fn reconciliation_preserves_unknown_for_sync_uncertainty_wrong_job_and_unconfirmed_outcomes()
     {
        for mode in [
            "sync_reconcile_uncertain",
            "async_reconcile_uncertain",
            "async_reconcile_unconfirmed",
            "async_reconcile_conflict",
            "async",
        ] {
            let job = mode.starts_with("async").then_some("synthetic-job");
            let (root, runner, run) = reconciliation_fixture(mode, "unknown", job).await;
            runner.reconcile_run(run.assignment.run_id).await.unwrap();
            let state = last_state(&runner, run.assignment.run_id).await;
            assert_eq!(state["state"], "unknown");
            assert!(
                state["reason"]
                    .as_str()
                    .is_some_and(|reason| !reason.is_empty())
            );
            let calls = std::fs::read_to_string(root.path().join("operations")).unwrap();
            assert_eq!(
                calls,
                if mode == "async" {
                    "reconcile\npoll\n"
                } else {
                    "reconcile\n"
                }
            );
            assert_saved_execution_unchanged(&runner, &run).await;
        }
        let (_root, runner, run) = reconciliation_fixture("query", "unknown", None).await;
        runner.reconcile_run(run.assignment.run_id).await.unwrap();
        let state = last_state(&runner, run.assignment.run_id).await;
        assert_eq!(state["state"], "unknown");
        assert!(
            state["reason"]
                .as_str()
                .unwrap()
                .contains("does not support reconciliation")
        );
        assert_saved_execution_unchanged(&runner, &run).await;
    }
    #[test]
    fn submit_budget_never_renews_an_expired_run_or_exceeds_its_remaining_time() {
        let active = |remaining| Active {
            cancel: CancellationToken::new(),
            stop: CancellationToken::new(),
            reconcile: Arc::new(tokio::sync::Notify::new()),
            deadline: tokio::time::Instant::now() + remaining,
        };
        assert!(Runner::submit_budget(&active(Duration::ZERO)).is_none());
        let short = Runner::submit_budget(&active(Duration::from_millis(250))).unwrap();
        assert!(short > Duration::ZERO && short <= Duration::from_millis(250));
        assert_eq!(
            Runner::submit_budget(&active(Duration::from_secs(120))),
            Some(Duration::from_secs(60))
        );
    }
    #[test]
    fn recovering_a_job_does_not_restart_its_timeout_budget() {
        let task: TaskConfig = serde_json::from_value(json!({"version":1,"id":"synthetic-task","name":"Synthetic","runner":"synthetic","agent":"synthetic","cwd":".","prompt":"synthetic instructions","execution":{"timeout":"1m"}})).unwrap();
        let agent: crate::config::AgentConfig = serde_json::from_value(json!({"version":1,"id":"synthetic","name":"Synthetic","mode":"async","submit":{"command":"unused"},"status":{"command":"unused"}})).unwrap();
        let run = LocalRun {
            assignment: Assignment {
                version: 1,
                run_id: Uuid::new_v4(),
                runner_id: Uuid::new_v4(),
                task,
                deadline: chrono::Utc::now() + chrono::Duration::minutes(1),
            },
            agent: agent.into(),
            claim: Uuid::new_v4(),
            phase: "started".into(),
            job: Some("synthetic-job".into()),
            started: chrono::Utc::now() - chrono::Duration::minutes(2),
            session: None,
            session_title: None,
        };
        assert!(Runner::active_for(&run).unwrap().deadline <= tokio::time::Instant::now());
        assert!(Runner::active_for(&run).unwrap().deadline <= tokio::time::Instant::now());
        assert_eq!(run.job.as_deref(), Some("synthetic-job"));
    }
}
