//! Integration-test helper for replay and credential-isolation checks.
use anyhow::{Context, Result};
use futures::StreamExt;
use std::{path::PathBuf, time::Duration};
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).context("mode")?;
    let url = args.get(2).context("url")?;
    let file = PathBuf::from(args.get(3).context("credentials")?);
    let id = args.get(4).context("runner id")?.parse::<uuid::Uuid>()?;
    let c = wakeit::bus::connect(url, &file, Some(format!("_INBOX.r_{}", id.simple()))).await?;
    let js = async_nats::jetstream::new(c.clone());
    match mode.as_str() {
        "replay" | "event" => {
            let body = std::fs::read(args.get(5).context("assignment file")?)?;
            wakeit::bus::publish(
                &js,
                if mode == "event" {
                    wakeit::protocol::event_subject(id)
                } else {
                    wakeit::protocol::work_subject(id)
                },
                format!("replay:{}", uuid::Uuid::new_v4()),
                body,
            )
            .await?;
        }
        "forbidden" => {
            let subject = args.get(5).context("forbidden subject")?.clone();
            let result =
                tokio::time::timeout(Duration::from_secs(3), c.request(subject, "{}".into())).await;
            anyhow::ensure!(
                !matches!(result, Ok(Ok(_))),
                "unauthorized request unexpectedly succeeded"
            );
        }
        "short-ack" => {
            let stream = js.get_stream("WAKEIT_WORK").await?;
            let consumer = stream
                .get_consumer::<async_nats::jetstream::consumer::pull::Config>(
                    &wakeit::protocol::consumer(id, "work"),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let mut config = consumer.cached_info().config.clone();
            config.ack_wait = Duration::from_secs(1);
            stream.update_consumer(config).await?;
        }
        "peek" => {
            let stream = js.get_stream("WAKEIT_WORK").await?;
            let consumer = stream
                .get_consumer::<async_nats::jetstream::consumer::pull::Config>(
                    &wakeit::protocol::consumer(id, "work"),
                )
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let mut messages = consumer
                .fetch()
                .max_messages(1)
                .expires(Duration::from_secs(1))
                .messages()
                .await?;
            if let Some(m) = messages.next().await {
                let m = m.map_err(|e| anyhow::anyhow!(e.to_string()))?;
                println!("{}", String::from_utf8_lossy(&m.payload));
            }
        }
        _ => anyhow::bail!("invalid probe mode"),
    }
    Ok(())
}
