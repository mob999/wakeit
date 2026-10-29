use crate::transport::streams as bus;
use anyhow::Result;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    workspace: Uuid,
    subject: String,
    key: String,
    payload: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO outbox(subject,message_id,payload,workspace_id) VALUES($1,$2,$3,$4) ON CONFLICT(message_id) DO NOTHING").bind(subject).bind(key).bind(payload).bind(workspace).execute(&mut **tx).await?;
    Ok(())
}

pub async fn dispatch(pool: &PgPool, js: &async_nats::jetstream::Context) -> Result<()> {
    let mut tx = pool.begin().await?;
    let rows=sqlx::query("SELECT id,subject,payload,message_id FROM outbox WHERE published_at IS NULL ORDER BY id LIMIT 20 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for row in rows {
        let ack = bus::publish(
            js,
            row.get("subject"),
            row.get("message_id"),
            serde_json::to_vec(&row.get::<Value, _>("payload"))?,
        )
        .await?;
        sqlx::query("UPDATE outbox SET published_at=now(),stream=$2,stream_seq=$3 WHERE id=$1")
            .bind(row.get::<i64, _>("id"))
            .bind(ack.stream)
            .bind(i64::try_from(ack.sequence)?)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    clear_completed_messages(pool, js).await?;
    Ok(())
}

pub async fn clear_completed_messages(
    pool: &PgPool,
    js: &async_nats::jetstream::Context,
) -> Result<()> {
    let rows=sqlx::query("SELECT o.id,o.stream,o.stream_seq FROM outbox o LEFT JOIN runs r ON r.id=(o.payload->>'run_id')::uuid AND r.workspace_id=o.workspace_id WHERE o.published_at IS NOT NULL AND o.stream_seq IS NOT NULL AND o.cleared_at IS NULL AND (r.id IS NULL OR (r.state IN ('succeeded','failed','cancelled','timed_out','skipped','interrupted','warning') AND (r.claim_id IS NULL OR r.finished_at<now()-interval '5 minutes'))) ORDER BY o.id LIMIT 100").fetch_all(pool).await?;
    for row in rows {
        let stream = js.get_stream(row.get::<String, _>("stream")).await?;
        let seq = row.get::<i64, _>("stream_seq") as u64;
        match stream.delete_message(seq).await {
            Ok(_) => {}
            Err(e) => {
                match e.kind() {
                    async_nats::jetstream::stream::DeleteMessageErrorKind::JetStream(error)
                        if error.code() == 404 => {}
                    async_nats::jetstream::stream::DeleteMessageErrorKind::JetStream(error)
                        if error.error_code()
                            == async_nats::jetstream::ErrorCode::STREAM_MESSAGE_DELETE_FAILED =>
                    {
                        // JetStream may return 500/10057 after a WorkQueue ACK (or
                        // another Gateway) already removed this message. Verify it
                        // is absent rather than swallowing an actual storage error.
                        match stream.get_raw_message(seq).await {
                        Err(raw) if matches!(raw.kind(), async_nats::jetstream::stream::RawMessageErrorKind::NoMessageFound) => {},
                        Err(raw) => return Err(raw.into()),
                        Ok(_) => return Err(e.into()),
                    }
                    }
                    _ => return Err(e.into()),
                }
            }
        }
        sqlx::query("UPDATE outbox SET cleared_at=now() WHERE id=$1")
            .bind(row.get::<i64, _>("id"))
            .execute(pool)
            .await?;
    }
    Ok(())
}
