use anyhow::Result;
use async_nats::jetstream;
use std::time::Duration;
pub async fn connect(
    url: &str,
    creds: &std::path::Path,
    inbox: Option<String>,
) -> Result<async_nats::Client> {
    let mut options = async_nats::ConnectOptions::with_credentials_file(creds)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    if let Some(prefix) = inbox {
        options = options.custom_inbox_prefix(prefix);
    }
    Ok(options
        .request_timeout(Some(Duration::from_secs(10)))
        .connect(url.split(',').collect::<Vec<_>>())
        .await?)
}
pub async fn publish(
    js: &jetstream::Context,
    subject: String,
    message_id: String,
    payload: Vec<u8>,
) -> Result<async_nats::jetstream::publish::PublishAck> {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", message_id);
    Ok(js
        .publish_with_headers(subject, headers, payload.into())
        .await?
        .await?)
}
