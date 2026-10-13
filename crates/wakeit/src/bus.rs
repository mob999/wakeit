use crate::protocol;
use anyhow::Result;
use async_nats::jetstream::{
    self,
    consumer::{AckPolicy, pull},
    stream::{Config, DiscardPolicy, RetentionPolicy, StorageType},
};
use std::time::Duration;
use uuid::Uuid;

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
pub async fn setup(js: &jetstream::Context, replicas: usize) -> Result<()> {
    for (name, subject) in [
        ("WAKEIT_WORK", "wakeit.v1.runner.*.work"),
        ("WAKEIT_CONTROL", "wakeit.v1.runner.*.control"),
        ("WAKEIT_EVENTS", "wakeit.v1.runner.*.events"),
    ] {
        js.get_or_create_stream(Config {
            name: name.into(),
            subjects: vec![subject.into()],
            retention: RetentionPolicy::WorkQueue,
            storage: StorageType::File,
            num_replicas: replicas,
            max_bytes: 1024 * 1024 * 1024,
            discard: DiscardPolicy::New,
            duplicate_window: Duration::from_secs(120),
            ..Default::default()
        })
        .await?;
    }
    Ok(())
}
pub async fn provision(js: &jetstream::Context, id: Uuid, capacity: usize) -> Result<()> {
    for (stream, kind, subject, max) in [
        (
            "WAKEIT_WORK",
            "work",
            protocol::work_subject(id),
            capacity as i64,
        ),
        (
            "WAKEIT_CONTROL",
            "control",
            protocol::control_subject(id),
            100,
        ),
    ] {
        js.get_stream(stream)
            .await?
            .create_consumer(pull::Config {
                durable_name: Some(protocol::consumer(id, kind)),
                filter_subject: subject,
                ack_policy: AckPolicy::Explicit,
                ack_wait: Duration::from_secs(60),
                max_ack_pending: max,
                ..Default::default()
            })
            .await?;
    }
    Ok(())
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
