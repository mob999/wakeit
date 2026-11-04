//! Native stdin and acknowledged public output for one SDK execution.
use anyhow::{Result, ensure};
use serde_json::Value;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tokio::{io::AsyncReadExt, sync::Mutex};
use wakeit_plugin_sdk::{
    adapter::NativeFact,
    agent::AgentContext,
    trace::{RecordData, StreamRecord},
};

#[derive(Clone)]
struct ExecutionIo {
    context: AgentContext,
    started: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Value>>>,
    cleanup_deadline: Arc<OnceLock<tokio::time::Instant>>,
}
tokio::task_local! { static CONTEXT: ExecutionIo; }

pub async fn scope<T>(
    context: AgentContext,
    future: impl std::future::Future<Output = T>,
) -> (T, bool, Option<Value>) {
    let state = ExecutionIo {
        context,
        started: Arc::new(AtomicBool::new(false)),
        result: Arc::new(Mutex::new(None)),
        cleanup_deadline: Arc::new(OnceLock::new()),
    };
    let value = CONTEXT.scope(state.clone(), future).await;
    let result = state.result.lock().await.clone();
    (value, state.started.load(Ordering::SeqCst), result)
}
pub fn active() -> bool {
    CONTEXT.try_with(|_| ()).is_ok()
}
pub fn context() -> Option<AgentContext> {
    CONTEXT.try_with(|state| state.context.clone()).ok()
}
pub fn session_title() -> Option<String> {
    context().and_then(|context| context.request.context.session_title)
}
pub fn uses_stdin(agent: &str) -> bool {
    // These native CLIs receive the prompt through argv, not piped input.
    !matches!(agent, "cline" | "cursor" | "qwen-code")
}
pub async fn input() -> Result<Vec<u8>> {
    let bytes = if let Some(context) = context() {
        context
            .request
            .context
            .prompt
            .unwrap_or_default()
            .into_bytes()
    } else {
        let mut bytes = Vec::new();
        tokio::io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        bytes
    };
    ensure!(bytes.len() <= 1024 * 1024, "prompt exceeds 1 MiB");
    Ok(bytes)
}
pub async fn started(pid: u32) -> Result<()> {
    if let Ok(state) = CONTEXT.try_with(Clone::clone) {
        // Set immediately after OS spawn, before the ACK: lost IPC is never
        // mistaken for proof that the native process did not start.
        state.started.store(true, Ordering::SeqCst);
        state.context.fact(NativeFact::Started { pid }).await?;
    }
    Ok(())
}
pub async fn start_failed(message: &str) -> Result<()> {
    if let Some(context) = context() {
        context
            .fact(NativeFact::StartFailed {
                message: message.into(),
            })
            .await?;
    }
    Ok(())
}
pub async fn input_submitted(context: Option<AgentContext>) -> Result<()> {
    if let Some(context) = context {
        context.fact(NativeFact::InputSubmitted).await?;
    }
    Ok(())
}
pub async fn finished(exit_code: Option<i32>) -> Result<()> {
    if let Some(context) = context() {
        context
            .fact(NativeFact::NativeFinished { exit_code })
            .await?;
    }
    Ok(())
}
pub async fn emit(bytes: &[u8]) -> Result<bool> {
    if let Ok(state) = CONTEXT.try_with(Clone::clone) {
        let record = StreamRecord::parse(bytes)?;
        let result = match &record.record {
            RecordData::Result(value) => Some(value.clone()),
            _ => None,
        };
        state.context.record(record).await?;
        if let Some(result) = result {
            *state.result.lock().await = Some(result);
        }
        return Ok(true);
    }
    Ok(false)
}
pub async fn cancelled() {
    if let Some(mut context) = context() {
        context.cancelled().await;
    } else {
        std::future::pending::<()>().await;
    }
}

/// ACP cancellation, child wait and output ACKs share one remaining cleanup budget.
pub fn cleanup_deadline() -> tokio::time::Instant {
    if let Ok(state) = CONTEXT.try_with(Clone::clone)
        && state.context.is_cancelled()
    {
        return *state
            .cleanup_deadline
            .get_or_init(|| tokio::time::Instant::now() + std::time::Duration::from_millis(1200));
    }
    tokio::time::Instant::now() + std::time::Duration::from_secs(2)
}
