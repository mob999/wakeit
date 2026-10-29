use crate::protocol::{self, RunEvent};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn ingest(pool: &PgPool, e: &RunEvent, log_limit: usize) -> Result<()> {
    ensure!(e.version == 1, "unsupported event version");
    let mut tx = pool.begin().await?;
    crate::store::dag::lock_parent(&mut tx, e.run_id).await?;
    let row=sqlx::query("SELECT r.* FROM runs r JOIN runners n ON n.id=r.runner_id WHERE r.id=$1 AND r.runner_id=$2 AND r.claim_id=$3 AND NOT n.revoked FOR UPDATE OF r").bind(e.run_id).bind(e.runner_id).bind(e.claim_id).fetch_optional(&mut *tx).await?;
    let Some(row) = row else { return Ok(()) };
    let duplicate: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM events WHERE run_id=$1 AND seq=$2)")
            .bind(e.run_id)
            .bind(e.seq)
            .fetch_one(&mut *tx)
            .await?;
    if duplicate {
        return Ok(());
    }
    let mut data = e.data.clone();
    let mut kind = e.kind.clone();
    let invalid_trace = kind == "trace"
        && crate::trace::StreamRecord::parse(&serde_json::to_vec(
            &json!({"version":1,"type":"trace","data":data}),
        )?)
        .is_err();
    let invalid_log = kind == "log" && !data["text"].is_string();
    if !data.is_object() || invalid_trace || invalid_log {
        tracing::warn!(run_id=%e.run_id, seq=e.seq, "malformed execution content omitted");
        kind = "trace_status".into();
        data = json!({"status":"incomplete","reason":"event_parse_failed"});
    }
    if matches!(kind.as_str(), "log" | "trace") {
        let bytes = if kind == "trace" {
            serde_json::to_vec(&data)?.len() as i64
        } else {
            data["text"].as_str().unwrap_or("").len() as i64
        };
        if row.get::<bool, _>("log_truncated")
            || row.get::<i64, _>("log_bytes") + bytes > log_limit as i64
        {
            if row.get::<bool, _>("log_truncated") {
                return Ok(());
            }
            kind = "trace_status".into();
            data = json!({"status":"truncated","reason":"content_limit"});
            sqlx::query("UPDATE runs SET log_truncated=true WHERE id=$1")
                .bind(e.run_id)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query("UPDATE runs SET log_bytes=log_bytes+$2 WHERE id=$1")
                .bind(e.run_id)
                .bind(bytes)
                .execute(&mut *tx)
                .await?;
        }
    }
    data["event_time"] = json!(e.timestamp);
    sqlx::query("INSERT INTO events(run_id,seq,kind,data,workspace_id) VALUES($1,$2,$3,$4,$5)")
        .bind(e.run_id)
        .bind(e.seq)
        .bind(&kind)
        .bind(&data)
        .bind(row.get::<Uuid, _>("workspace_id"))
        .execute(&mut *tx)
        .await?;
    if kind == "snapshot" {
        sqlx::query(
            "UPDATE runs SET execution_snapshot=$2 || COALESCE(execution_snapshot,'{}'::jsonb) WHERE id=$1 AND (execution_snapshot IS NULL OR NOT execution_snapshot ? 'agent_digest')",
        )
        .bind(e.run_id)
        .bind(&data)
        .execute(&mut *tx)
        .await?;
    }
    if kind == "session" && e.seq > row.get::<i64, _>("session_seq") {
        // Unknown or malformed metadata cannot poison the reliable event consumer.
        if let Ok(mut session) =
            serde_json::from_value::<crate::sessions::RunSession>(e.data.clone())
            && crate::sessions::validate_id(&session.id).is_ok()
            && session.runner_id == e.runner_id
            && row.get::<Value, _>("config")["agent"] == session.agent
            && !session.cwd.is_empty()
        {
            let previous: Option<Value> = row.get("session");
            let config: Value = row.get("config");
            let expected = previous
                .as_ref()
                .and_then(|v| v["id"].as_str())
                .or_else(|| config["session"]["id"].as_str());
            if expected.is_some_and(|id| id != session.id) {
                session.ready = false;
                session.reason = Some("Agent returned a different session ID".into());
                session.id = expected.unwrap().into();
            }
            sqlx::query("UPDATE runs SET session=$2,session_seq=$3 WHERE id=$1")
                .bind(e.run_id)
                .bind(serde_json::to_value(session)?)
                .bind(e.seq)
                .execute(&mut *tx)
                .await?;
        }
    }
    let state: String = row.get("state");
    if !protocol::terminal(&state) && kind == "state" && e.seq > row.get::<i64, _>("last_state_seq")
    {
        let new = data["state"].as_str().context("state missing")?;
        ensure!(
            protocol::terminal(new) || matches!(new, "running" | "unknown"),
            "invalid run state"
        );
        sqlx::query("UPDATE runs SET state=$2,reason=$3,result=COALESCE($4,result),job_id=COALESCE($5,job_id),last_seen=now(),last_state_seq=$7,finished_at=CASE WHEN $6 THEN now() ELSE NULL END WHERE id=$1")
            .bind(e.run_id).bind(new).bind(data["reason"].as_str()).bind(data.get("result")).bind(data["job_id"].as_str()).bind(protocol::terminal(new)).bind(e.seq).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
