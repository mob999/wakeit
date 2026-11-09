//! Typed Agent registration and acknowledged public output, independent of Core.
use crate::Context;
use anyhow::{Context as _, Result, ensure};
use serde_json::json;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
use wakeit_plugin_protocol::{
    adapter::{
        AgentDescriptor, AgentFact, AgentOperation, AgentRecord, AgentReply, AgentRequest,
        NativeFact,
    },
    sessions::SessionInfo,
    trace::{LogRecord, RecordData, StreamRecord, TraceNode},
};

pub type AgentFuture = Pin<Box<dyn Future<Output = Result<AgentReply>> + Send>>;
pub type AgentHandler = Arc<dyn Fn(AgentRequest, AgentContext) -> AgentFuture + Send + Sync>;

/// A registration has one callback for every statically advertised operation.
#[derive(Clone)]
pub struct Agent {
    pub descriptor: AgentDescriptor,
    handlers: BTreeMap<AgentOperation, AgentHandler>,
    duplicate_operation: bool,
}
impl Agent {
    pub fn new(descriptor: AgentDescriptor) -> Self {
        Self {
            descriptor,
            handlers: BTreeMap::new(),
            duplicate_operation: false,
        }
    }
    pub fn on<F, Fut>(mut self, operation: AgentOperation, handler: F) -> Self
    where
        F: Fn(AgentRequest, AgentContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<AgentReply>> + Send + 'static,
    {
        if self.handlers.contains_key(&operation) {
            self.duplicate_operation = true;
            return self;
        }
        self.handlers.insert(
            operation,
            Arc::new(move |request, context| Box::pin(handler(request, context))),
        );
        self
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.duplicate_operation,
            "Agent operation registered more than once"
        );
        self.descriptor.validate()?;
        ensure!(
            self.descriptor.operations.iter().eq(self.handlers.keys()),
            "Declared and registered Agent operations do not match"
        );
        Ok(())
    }
    pub(crate) async fn invoke(
        &self,
        request: AgentRequest,
        host: Context,
        cancel: tokio::sync::watch::Sender<bool>,
    ) -> Result<AgentReply> {
        self.validate()?;
        request.validate(&self.descriptor)?;
        let handler = self
            .handlers
            .get(&request.operation)
            .context("Agent operation is not registered")?;
        ensure!(
            !*host.cancelled.borrow(),
            "Agent request was cancelled before execution"
        );
        let timeout = (request.context.deadline - chrono::Utc::now())
            .to_std()
            .context("Agent request expired")?;
        let context = AgentContext {
            request: request.clone(),
            host,
            closed: Arc::new(AtomicBool::new(false)),
            output: Arc::new(Mutex::new(OutputState {
                next_sequence: 1,
                closed: false,
            })),
        };
        let mut cancelled = context.host.cancelled.clone();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut close_deadline = None;
        let mut stop = None;
        let mut work = handler(request, context.clone());
        let result: Result<AgentReply> = tokio::select! {
            result = &mut work => result,
            _ = tokio::time::sleep_until(deadline) => {
                let _ = cancel.send(true);
                stop = Some(("timeout", "Agent request deadline exceeded"));
                let end = tokio::time::Instant::now() + Duration::from_millis(1500);
                close_deadline = Some(end);
                match tokio::time::timeout_at(end, &mut work).await {
                    Ok(Ok(reply)) if preserves_stop_evidence(&reply) => Ok(reply),
                    _ => Err(crate::ExtensionError::new("timeout", "Agent request deadline exceeded").into()),
                }
            },
            _ = cancelled.changed() => {
                stop = Some(("cancelled", "Agent request cancelled"));
                let end = tokio::time::Instant::now() + Duration::from_millis(1500);
                close_deadline = Some(end);
                match tokio::time::timeout_at(end, &mut work).await {
                    Ok(Ok(reply)) if preserves_stop_evidence(&reply) => Ok(reply),
                    _ => Err(crate::ExtensionError::new("cancelled", "Agent request cancelled").into()),
                }
            }
        };
        // Drop a timed-out future before waiting on a lock it may itself hold.
        // Otherwise a late ACK cannot advance that no-longer-polled future.
        drop(work);
        context.closed.store(true, Ordering::SeqCst);
        let mut end = close_deadline
            .unwrap_or(deadline.min(tokio::time::Instant::now() + Duration::from_millis(1500)));
        loop {
            tokio::select! {
                biased;
                mut output = context.output.lock() => { output.closed = true; break; },
                _ = cancelled.changed(), if stop.is_none() => {
                    stop = Some(("cancelled", "Agent request cancelled"));
                    end = tokio::time::Instant::now() + Duration::from_millis(1500);
                },
                _ = tokio::time::sleep_until(deadline), if stop.is_none() => {
                    let _ = cancel.send(true);
                    stop = Some(("timeout", "Agent request deadline exceeded"));
                    end = tokio::time::Instant::now() + Duration::from_millis(1500);
                },
                _ = tokio::time::sleep_until(end) => {
                    return Err(crate::ExtensionError::new("uncertain", "Agent output acknowledgement did not finish before close").into());
                }
            }
        }
        if chrono::Utc::now() >= context.request.context.deadline {
            stop = Some(("timeout", "Agent request deadline exceeded"));
        }
        if *context.host.cancelled.borrow() && stop.is_none() {
            stop = Some(("cancelled", "Agent request cancelled"));
        }
        if let Some((code, message)) = stop {
            return match result {
                Ok(reply) if preserves_stop_evidence(&reply) => Ok(reply),
                _ => Err(crate::ExtensionError::new(code, message).into()),
            };
        }

        result
    }
}

/// A host stop request cannot erase evidence of unfinished remote work or its
/// job identity. It also cannot turn an ordinary late success into Run success.
fn preserves_stop_evidence(reply: &AgentReply) -> bool {
    use crate::adapter::ExecutionOutcome;
    matches!(
        reply,
        AgentReply::Execution(
            ExecutionOutcome::Uncertain { .. }
                | ExecutionOutcome::Cancelled { .. }
                | ExecutionOutcome::Completed {
                    confirmed: false,
                    ..
                }
                | ExecutionOutcome::NotStarted { .. }
                | ExecutionOutcome::Submitted { .. }
                | ExecutionOutcome::Running { .. }
                | ExecutionOutcome::CancelRequested { .. }
        )
    )
}

/// The host binds identity and acknowledges each record only after validation and
/// persistence. Plugin code cannot choose another Run's ID or its Journal seq.
struct OutputState {
    next_sequence: u64,
    closed: bool,
}
#[derive(Clone)]
pub struct AgentContext {
    pub request: AgentRequest,
    pub host: Context,
    output: Arc<Mutex<OutputState>>,
    closed: Arc<AtomicBool>,
}
impl AgentContext {
    pub async fn cancelled(&mut self) {
        if *self.host.cancelled.borrow() {
            return;
        }
        let _ = self.host.cancelled.changed().await;
    }
    pub fn is_cancelled(&self) -> bool {
        *self.host.cancelled.borrow()
    }
    pub async fn record(&self, record: StreamRecord) -> Result<()> {
        ensure!(
            !self.closed.load(Ordering::SeqCst),
            "Agent output is already complete"
        );
        ensure!(
            self.request.operation.is_execution(),
            "Read-only Agent requests cannot emit execution output"
        );
        if self.is_cancelled() {
            ensure!(
                matches!(
                    &record.record,
                    RecordData::Log(_)
                        | RecordData::Trace(crate::trace::TraceNode {
                            status: crate::trace::TraceState::Succeeded
                                | crate::trace::TraceState::Failed
                                | crate::trace::TraceState::Cancelled,
                            ..
                        })
                ),
                "Cancellation permits only existing execution diagnostic/terminal trace output"
            );
        }
        let run = self
            .request
            .context
            .run_id
            .context("Agent output requires a Run")?;
        let encoded = serde_json::to_vec(&record)?;
        // Use the same version, Session ID and frame-size validation as Runner.
        StreamRecord::parse(&encoded)?;
        self.transmit(
            "agent.record",
            serde_json::to_value(AgentRecord {
                request_id: self.request.id,
                run_id: run,
                seq: 0,
                record,
            })?,
        )
        .await
    }
    /// Only observations bound to this existing Run may be sent during cooperative
    /// cancellation. Credentials, process starts and other host APIs stay blocked.
    pub async fn fact(&self, fact: NativeFact) -> Result<()> {
        ensure!(
            !self.closed.load(Ordering::SeqCst),
            "Agent output is already complete"
        );
        ensure!(
            self.request.operation.is_execution(),
            "Native facts require an execution"
        );
        let run = self
            .request
            .context
            .run_id
            .context("Native facts require a Run")?;
        if self.is_cancelled() {
            ensure!(
                matches!(
                    fact,
                    NativeFact::NativeFinished { .. } | NativeFact::StartFailed { .. }
                ),
                "Cancellation cannot start native work or submit new input"
            );
        }
        self.transmit(
            "agent.fact",
            serde_json::to_value(AgentFact {
                request_id: self.request.id,
                run_id: run,
                seq: 0,
                fact,
            })?,
        )
        .await
    }
    /// Sending and acknowledging an observation has its own owned lifetime.
    /// Dropping a native stdin writer cannot rewind an already transmitted seq.
    async fn transmit(&self, method: &'static str, mut value: serde_json::Value) -> Result<()> {
        let output = self.output.clone();
        let closed = self.closed.clone();
        let client = self.host.host.clone();
        tokio::spawn(async move {
            let mut state = output.lock().await;
            ensure!(
                !state.closed && !closed.load(Ordering::SeqCst),
                "Agent output is already complete"
            );
            value["seq"] = json!(state.next_sequence);
            let result = client.call::<serde_json::Value>(method, value).await;
            // Any failed acknowledgement seals this channel. Reusing a sequence
            // after an ambiguous transport failure could rewrite accepted output.
            if result.is_err() {
                state.closed = true;
                closed.store(true, Ordering::SeqCst);
            }
            result?;
            state.next_sequence += 1;
            Ok(())
        })
        .await
        .context("Agent output acknowledgement task stopped")?
    }
    pub async fn trace(&self, node: TraceNode) -> Result<()> {
        self.record(StreamRecord::new(RecordData::Trace(node)))
            .await
    }
    pub async fn log(&self, log: LogRecord) -> Result<()> {
        self.record(StreamRecord::new(RecordData::Log(log))).await
    }
    pub async fn session(&self, session: SessionInfo) -> Result<()> {
        self.record(StreamRecord::new(RecordData::Session(session)))
            .await
    }
    pub async fn result(&self, result: serde_json::Value) -> Result<()> {
        self.record(StreamRecord::new(RecordData::Result(result)))
            .await
    }
    pub async fn config<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        self.host
            .call("config.get", json!({"agent_request_id": self.request.id}))
            .await
    }
    pub async fn credential(&self, slot: &str) -> Result<serde_json::Value> {
        self.host
            .call(
                "credentials.get",
                json!({"slot": slot,"agent_request_id": self.request.id}),
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Client,
        adapter::{AgentContext as RequestContext, ExecutionOutcome},
    };
    use std::collections::HashMap;
    use tokio::sync::{mpsc, watch};
    use uuid::Uuid;

    fn context() -> (
        Context,
        watch::Sender<bool>,
        mpsc::Receiver<serde_json::Value>,
        Client,
    ) {
        let (writes, messages) = mpsc::channel(8);
        let client = Client {
            writes,
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        let (cancel, cancelled) = watch::channel(false);
        (
            Context {
                invocation: None,
                cancelled,
                host: client.clone(),
            },
            cancel,
            messages,
            client,
        )
    }
    fn request(timeout: Duration) -> AgentRequest {
        AgentRequest {
            id: Uuid::new_v4(),
            agent: "synthetic".into(),
            operation: AgentOperation::Execute,
            context: RequestContext {
                cwd: "/synthetic".into(),
                deadline: chrono::Utc::now() + chrono::Duration::from_std(timeout).unwrap(),
                run_id: Some(Uuid::new_v4()),
                task_id: Some("synthetic-task".into()),
                parameters: BTreeMap::new(),
                prompt: Some("synthetic".into()),
                session_id: None,
                session_title: None,
                job_id: None,
            },
            input: serde_json::Value::Null,
        }
    }
    fn agent<F, Fut>(handler: F) -> Agent
    where
        F: Fn(AgentRequest, AgentContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<AgentReply>> + Send + 'static,
    {
        Agent::new(AgentDescriptor {
            name: "Synthetic".into(),
            logo: None,
            parameters: BTreeMap::new(),
            mode: crate::AgentMode::Sync,
            operations: [AgentOperation::Detect, AgentOperation::Execute].into(),
        })
        .on(AgentOperation::Detect, |_, _| async {
            Ok(AgentReply::Detection(crate::adapter::Detection {
                available: true,
                version: None,
                reason: None,
            }))
        })
        .on(AgentOperation::Execute, handler)
    }
    fn completed() -> AgentReply {
        AgentReply::Execution(ExecutionOutcome::Completed {
            exit_code: Some(0),
            result: serde_json::Value::Null,
            confirmed: true,
        })
    }
    async fn ack(client: &Client, message: &serde_json::Value) {
        let id = message["id"].as_str().unwrap().parse::<Uuid>().unwrap();
        let pending = client.pending.lock().await.remove(&id);
        if let Some(pending) = pending {
            let _ = pending.send(Ok(serde_json::Value::Null));
        }
    }

    #[tokio::test]
    async fn failure_waits_for_inflight_output_and_blocks_late_context_clones() {
        let (context, cancel, mut messages, client) = context();
        let published = Arc::new(tokio::sync::Notify::new());
        let callback = agent({
            let published = published.clone();
            move |_, context| {
                let published = published.clone();
                async move {
                    let writer = context.clone();
                    tokio::spawn(async move {
                        let result = writer.result(json!("synthetic result")).await;
                        let late = writer.result(json!("late result")).await;
                        (result, late)
                    });
                    published.notified().await;
                    anyhow::bail!("synthetic failure")
                }
            }
        });
        let execution = tokio::spawn(async move {
            callback
                .invoke(request(Duration::from_secs(5)), context, cancel)
                .await
        });
        let record = messages.recv().await.unwrap();
        assert_eq!(record["method"], "agent.record");
        published.notify_one();
        tokio::task::yield_now().await;
        assert!(!execution.is_finished());
        ack(&client, &record).await;
        assert!(execution.await.unwrap().is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(30), messages.recv())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn cancellation_during_ack_barrier_cannot_return_success() {
        let (context, cancel, mut messages, client) = context();
        let release = Arc::new(tokio::sync::Notify::new());
        let callback = agent({
            let release = release.clone();
            move |_, context| {
                let release = release.clone();
                async move {
                    tokio::spawn(async move { context.result(json!("synthetic result")).await });
                    release.notified().await;
                    Ok(completed())
                }
            }
        });
        let control = cancel.clone();
        let execution = tokio::spawn(async move {
            callback
                .invoke(request(Duration::from_secs(5)), context, cancel)
                .await
        });
        let record = messages.recv().await.unwrap();
        release.notify_one();
        tokio::task::yield_now().await;
        control.send(true).unwrap();
        ack(&client, &record).await;
        let error = execution.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("cancel"));
    }

    #[tokio::test]
    async fn timeout_drops_a_handler_holding_output_and_rejects_queued_writers() {
        let (context, cancel, mut messages, client) = context();
        let callback = agent(|_, context| async move {
            let pending = context.clone();
            tokio::spawn(async move {
                tokio::task::yield_now().await;
                pending.result(json!("late result")).await
            });
            context.result(json!("first result")).await?;
            Ok(completed())
        });
        let started = tokio::time::Instant::now();
        let execution = tokio::spawn(async move {
            callback
                .invoke(request(Duration::from_millis(40)), context, cancel)
                .await
        });
        let record = messages.recv().await.unwrap();
        let error = tokio::time::timeout(Duration::from_secs(2), execution)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(
            error.to_string().contains("deadline") || error.to_string().contains("acknowledgement")
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        ack(&client, &record).await;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), messages.recv())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn cooperative_stop_preserves_native_uncertainty_and_job_identity() {
        for outcome in [
            ExecutionOutcome::Uncertain {
                reason: "Remote cancellation is not confirmed".into(),
            },
            ExecutionOutcome::Cancelled { confirmed: false },
            ExecutionOutcome::Completed {
                exit_code: Some(0),
                result: serde_json::Value::Null,
                confirmed: false,
            },
            ExecutionOutcome::NotStarted {
                message: "No request was submitted".into(),
            },
            ExecutionOutcome::Cancelled { confirmed: true },
            ExecutionOutcome::Submitted {
                job_id: "existing-job".into(),
            },
        ] {
            let (context, cancel, _messages, _client) = context();
            let expected = serde_json::to_value(&outcome).unwrap();
            let ready = Arc::new(tokio::sync::Notify::new());
            let callback = agent({
                let ready = ready.clone();
                move |_, mut context| {
                    let outcome = outcome.clone();
                    let ready = ready.clone();
                    async move {
                        ready.notify_one();
                        context.cancelled().await;
                        Ok(AgentReply::Execution(outcome))
                    }
                }
            });
            let control = cancel.clone();
            let execution = tokio::spawn(async move {
                callback
                    .invoke(request(Duration::from_secs(5)), context, cancel)
                    .await
            });
            ready.notified().await;
            control.send(true).unwrap();
            let actual = execution.await.unwrap().unwrap();
            assert_eq!(serde_json::to_value(actual).unwrap()["value"], expected);
        }
    }
    #[tokio::test]
    async fn aborting_a_fact_caller_cannot_reuse_an_already_transmitted_sequence() {
        let (context, cancel, mut messages, client) = context();
        let drop_caller = Arc::new(tokio::sync::Notify::new());
        let callback = agent({
            let drop_caller = drop_caller.clone();
            move |_, context| {
                let drop_caller = drop_caller.clone();
                async move {
                    let writer = context.clone();
                    let input =
                        tokio::spawn(async move { writer.fact(NativeFact::InputSubmitted).await });
                    drop_caller.notified().await;
                    input.abort();
                    let _ = input.await;
                    context
                        .fact(NativeFact::NativeFinished { exit_code: Some(0) })
                        .await?;
                    Ok(completed())
                }
            }
        });
        let execution = tokio::spawn(async move {
            callback
                .invoke(request(Duration::from_secs(5)), context, cancel)
                .await
        });
        let input = messages.recv().await.unwrap();
        assert_eq!(input["method"], "agent.fact");
        assert_eq!(input["params"]["seq"], 1);
        drop_caller.notify_one();
        assert!(
            tokio::time::timeout(Duration::from_millis(30), messages.recv())
                .await
                .is_err()
        );
        ack(&client, &input).await;
        let finished = messages.recv().await.unwrap();
        assert_eq!(finished["params"]["seq"], 2);
        assert_eq!(finished["params"]["fact"]["event"], "native_finished");
        ack(&client, &finished).await;
        assert!(execution.await.unwrap().is_ok());
    }
}
