use crate::{
    state::App,
    terminal_protocol::{self as wire, Control},
};
use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;
use uuid::Uuid;
pub(crate) async fn terminal(s: &App, request: Control) -> Result<Value> {
    let reply = format!(
        "{}.terminalrpc.{}",
        crate::protocol::reply_prefix(request.runner_id),
        Uuid::new_v4().simple()
    );
    let mut sub = s
        .client
        .subscribe(reply.clone())
        .await
        .map_err(anyhow::Error::from)?;
    s.client
        .publish_with_reply(
            wire::control_subject(request.runner_id),
            reply,
            serde_json::to_vec(&request)?.into(),
        )
        .await
        .map_err(anyhow::Error::from)?;
    s.client.flush().await.map_err(anyhow::Error::from)?;
    let result = tokio::time::timeout(Duration::from_secs(5), sub.next()).await;
    let _ = sub.unsubscribe().await;
    let message = result
        .map_err(|_| QueryError::Timeout("terminal connection timed out".into()))?
        .ok_or_else(|| QueryError::Unavailable("Runner is offline".into()))?;
    if message.status == Some(async_nats::StatusCode::NO_RESPONDERS) {
        return Err(QueryError::Unavailable("Runner does not support web terminals".into()).into());
    }
    let body: Value = serde_json::from_slice(&message.payload)?;
    if body["ok"] != true {
        return Err(QueryError::Conflict(
            body["error"]
                .as_str()
                .unwrap_or("terminal operation failed")
                .into(),
        )
        .into());
    }
    Ok(body["data"].clone())
}

#[derive(Debug)]
pub(crate) enum QueryError {
    Unavailable(String),
    Timeout(String),
    Conflict(String),
}
impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Timeout(message) | Self::Conflict(message) => {
                f.write_str(message)
            }
        }
    }
}
impl std::error::Error for QueryError {}

pub(crate) async fn subscribe(
    client: &async_nats::Client,
    runner: Uuid,
    request: &crate::protocol::QueryRequest,
) -> Result<async_nats::Subscriber> {
    let reply = format!(
        "{}.{}",
        crate::protocol::reply_prefix(runner),
        request.id.simple()
    );
    let sub = client
        .subscribe(reply.clone())
        .await
        .map_err(anyhow::Error::from)?;
    client
        .publish_with_reply(
            crate::protocol::query_subject(runner),
            reply,
            serde_json::to_vec(request)?.into(),
        )
        .await
        .map_err(anyhow::Error::from)?;
    client.flush().await.map_err(anyhow::Error::from)?;
    Ok(sub)
}
