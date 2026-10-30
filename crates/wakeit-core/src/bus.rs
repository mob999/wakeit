use anyhow::Result;
use async_nats::jetstream;
use std::time::Duration;
use std::{error::Error as StdError, fmt};
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
#[derive(Debug)]
pub struct PayloadTooLarge {
    pub size: usize,
    pub max: usize,
}
impl fmt::Display for PayloadTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "encoded NATS payload is {} bytes; maximum is {}",
            self.size, self.max
        )
    }
}
impl StdError for PayloadTooLarge {}

pub fn is_payload_too_large(error: &anyhow::Error) -> bool {
    error.downcast_ref::<PayloadTooLarge>().is_some()
}

fn header_size(message_id: &str) -> usize {
    b"NATS/1.0\r\nNats-Msg-Id: \r\n\r\n".len() + message_id.len()
}

pub fn payload_capacity(js: &jetstream::Context, message_id: &str) -> usize {
    js.client()
        .max_payload()
        .saturating_sub(header_size(message_id))
}

fn validate_payload(message_id: &str, payload: &[u8], max: usize) -> Result<()> {
    let size = payload.len().saturating_add(header_size(message_id));
    if size > max {
        return Err(PayloadTooLarge { size, max }.into());
    }
    Ok(())
}

pub async fn publish(
    js: &jetstream::Context,
    subject: String,
    message_id: String,
    payload: Vec<u8>,
) -> Result<async_nats::jetstream::publish::PublishAck> {
    let max = js.client().max_payload();
    validate_payload(&message_id, &payload, max)?;
    let size = payload.len() + header_size(&message_id);
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", message_id);
    let pending = js
        .publish_with_headers(subject, headers, payload.into())
        .await
        .map_err(|error| -> anyhow::Error {
            if error.kind() == async_nats::jetstream::context::PublishErrorKind::MaxPayloadExceeded
            {
                PayloadTooLarge { size, max }.into()
            } else {
                error.into()
            }
        })?;
    pending.await.map_err(|error| -> anyhow::Error {
        if error.kind() == async_nats::jetstream::context::PublishErrorKind::MaxPayloadExceeded {
            PayloadTooLarge { size, max }.into()
        } else {
            error.into()
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_escaped_envelopes_and_headers_at_both_broker_limits() {
        let payload =
            serde_json::to_vec(&serde_json::json!({"task":{"prompt":"\0".repeat(400_000)}}))
                .unwrap();
        for max in [1024 * 1024, 2 * 1024 * 1024] {
            assert!(is_payload_too_large(
                &validate_payload("start:test", &payload, max).unwrap_err()
            ));
            let fitting = vec![b'x'; max - header_size("start:test")];
            assert!(validate_payload("start:test", &fitting, max).is_ok());
            assert!(is_payload_too_large(
                &validate_payload("longer-message-id", &fitting, max).unwrap_err()
            ));
        }
        assert!(!is_payload_too_large(&anyhow::anyhow!(
            "connection refused"
        )));
    }
}
