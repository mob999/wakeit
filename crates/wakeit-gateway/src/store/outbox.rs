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
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM outbox WHERE published_at IS NULL AND quarantined_at IS NULL ORDER BY id LIMIT 20")
        .fetch_all(pool).await?;
    for id in ids {
        // Commit each acknowledgement independently. A later failed publish must
        // not roll back markers for messages the broker already accepted.
        let mut tx = pool.begin().await?;
        let row = sqlx::query("SELECT subject,payload,message_id,workspace_id FROM outbox WHERE id=$1 AND published_at IS NULL AND quarantined_at IS NULL FOR UPDATE SKIP LOCKED")
            .bind(id).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { continue };
        let payload: Value = row.get("payload");
        let result = bus::publish(
            js,
            row.get("subject"),
            row.get("message_id"),
            serde_json::to_vec(&payload)?,
        )
        .await;
        match result {
            Ok(ack) => {
                sqlx::query(
                    "UPDATE outbox SET published_at=now(),stream=$2,stream_seq=$3 WHERE id=$1",
                )
                .bind(id)
                .bind(ack.stream)
                .bind(i64::try_from(ack.sequence)?)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
            }
            Err(error) if bus::is_payload_too_large(&error) => {
                tx.rollback().await?;
                quarantine(
                    pool,
                    id,
                    row.get("workspace_id"),
                    &payload,
                    &error.to_string(),
                )
                .await?;
            }
            Err(error) => return Err(error),
        }
    }
    clear_completed_messages(pool, js).await?;
    Ok(())
}

async fn quarantine(
    pool: &PgPool,
    id: i64,
    workspace: Uuid,
    payload: &Value,
    error: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    let run = payload
        .get("run_id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Uuid>().ok());
    let mut execution = None;
    if let Some(run) = run {
        // Match claim/cancel lock order: task -> parent graph -> execution -> outbox.
        sqlx::query("SELECT t.id FROM tasks t JOIN runs r ON r.workspace_id=t.workspace_id AND r.task_id=t.id WHERE r.workspace_id=$1 AND r.id=$2 FOR UPDATE OF t")
            .bind(workspace).bind(run).fetch_optional(&mut *tx).await?;
        crate::store::dag::lock_parent(&mut tx, run).await?;
        execution = sqlx::query(
            "SELECT state,claim_id FROM runs WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(workspace)
        .bind(run)
        .fetch_optional(&mut *tx)
        .await?;
    }
    let reason = format!("Task could not be delivered: {error}");
    let quarantined = sqlx::query("UPDATE outbox SET quarantined_at=now(),quarantine_reason=$2 WHERE id=$1 AND published_at IS NULL AND quarantined_at IS NULL")
        .bind(id).bind(&reason).execute(&mut *tx).await?.rows_affected();
    if quarantined > 0
        && payload.get("task").is_some()
        && let (Some(run), Some(execution)) = (run, execution)
        && execution.get::<String, _>("state") == "queued"
        && execution.get::<Option<Uuid>, _>("claim_id").is_none()
    {
        // An earlier accepted delivery may already own a claim. Never terminate
        // that execution because a later redelivery was too large.
        crate::store::dag::event(
            &mut tx,
            workspace,
            run,
            "state",
            serde_json::json!({"state":"failed","reason":reason,"retryable":false}),
        )
        .await?;
        sqlx::query("UPDATE runs SET state='failed',reason=$2,finished_at=now(),last_state_seq=(SELECT MAX(seq) FROM events WHERE run_id=$1) WHERE id=$1")
            .bind(run).bind(&reason).execute(&mut *tx).await?;
    }
    tx.commit().await?;
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
