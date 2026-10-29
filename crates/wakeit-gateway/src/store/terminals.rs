use crate::{
    state::App,
    terminal_protocol::Control,
    transport::query::{QueryError, terminal as rpc},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::time::{Duration, Instant};
use uuid::Uuid;
pub(crate) fn request(row: &PgRow, action: &str, data: Value) -> Control {
    Control {
        version: 1,
        terminal_id: row.get("id"),
        workspace_id: row.get("workspace_id"),
        runner_id: row.get("runner_id"),
        runtime_id: row.get("runtime_id"),
        generation: row.get("generation"),
        expires_at: (Utc::now() + chrono::Duration::seconds(5)).timestamp_millis(),
        action: action.into(),
        data,
    }
}

pub(crate) async fn apply_state<'a>(
    db: impl sqlx::PgExecutor<'a>,
    id: Uuid,
    generation: i64,
    state: &Value,
) -> Result<()> {
    let current = state["state"].as_str().unwrap_or("");
    if !["starting", "running", "closing", "closed", "unknown"].contains(&current) {
        return Ok(());
    }
    sqlx::query("UPDATE terminals SET state=$3,reason=$4,exit_code=$5,closed_at=CASE WHEN $3='closed' THEN COALESCE(closed_at,now()) ELSE closed_at END,attached=CASE WHEN $3 IN ('closed','unknown') THEN false ELSE attached END WHERE id=$1 AND generation=$2 AND state<>'closed' AND (state<>'closing' OR $3 IN ('closing','closed','unknown'))")
        .bind(id).bind(generation).bind(current).bind(state["reason"].as_str()).bind(state["exit_code"].as_i64().map(|n|n as i32)).execute(db).await?;
    Ok(())
}

pub(crate) async fn refresh(s: &App, row: &PgRow) -> Result<()> {
    if row.get::<String, _>("state") == "created"
        && row.get::<DateTime<Utc>, _>("lease_until") < Utc::now()
    {
        sqlx::query("UPDATE terminals SET state='closed',closed_at=now(),attached=false WHERE id=$1 AND state='created'").bind(row.get::<Uuid,_>("id")).execute(&s.pool).await?;
        return Ok(());
    }
    if row.get::<String, _>("state") == "closed" {
        return Ok(());
    }
    let mut control = request(row, "status", json!({}));
    let runtime: Option<Uuid> = sqlx::query_scalar("SELECT runtime_id FROM runners WHERE id=$1")
        .bind(control.runner_id)
        .fetch_optional(&s.pool)
        .await?
        .flatten();
    if let Some(runtime) = runtime {
        control.runtime_id = runtime;
    }
    match rpc(s, control).await {
        Ok(value) => {
            let state = value
                .get("state")
                .filter(|v| v.is_object())
                .unwrap_or(&value);
            if state["state"] == "missing"
                && row.get::<DateTime<Utc>, _>("lease_until") < Utc::now()
            {
                apply_state(
                    &s.pool,
                    row.get("id"),
                    row.get("generation"),
                    &json!({"state":"closed"}),
                )
                .await?;
            } else {
                apply_state(&s.pool, row.get("id"), row.get("generation"), state).await?;
            }
        }
        Err(_) => {
            sqlx::query("UPDATE terminals SET state='unknown',attached=false WHERE id=$1 AND lease_until<now() AND state NOT IN ('created','closed')").bind(row.get::<Uuid,_>("id")).execute(&s.pool).await?;
        }
    }
    Ok(())
}

pub(crate) async fn stop(s: &App, row: &PgRow) -> Result<()> {
    let id: Uuid = row.get("id");
    let mut tx = s.pool.begin().await?;
    let state: String = sqlx::query_scalar("SELECT state FROM terminals WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if state == "closed" {
        return Ok(());
    }
    if state == "created" {
        sqlx::query("UPDATE terminals SET state='closed',closed_at=now(),attached=false,ticket_hash=NULL WHERE id=$1").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(());
    }
    sqlx::query("UPDATE terminals SET state='closing',ticket_hash=NULL WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let _ = rpc(s, request(row, "close", json!({}))).await?;
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        refresh(s, row).await?;
        let state: String = sqlx::query_scalar("SELECT state FROM terminals WHERE id=$1")
            .bind(id)
            .fetch_one(&s.pool)
            .await?;
        if matches!(state.as_str(), "closed" | "unknown") {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(QueryError::Timeout("terminal cleanup is pending".into()).into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Reconcile metadata after lost connections or Runner restarts. Never starts a process.
pub(crate) async fn maintain(s: &App) -> Result<()> {
    let rows=sqlx::query("SELECT t.*, EXISTS(SELECT 1 FROM credentials c JOIN users u ON u.id=c.user_id JOIN workspace_members m ON m.user_id=u.id AND m.workspace_id=t.workspace_id JOIN workspaces w ON w.id=m.workspace_id WHERE c.hash=t.credential AND c.user_id=t.user_id AND (c.expires_at IS NULL OR c.expires_at>now()) AND u.active AND m.active AND m.role IN ('admin','operator') AND NOT w.is_demo) AND NOT r.revoked AS authorized FROM terminals t JOIN runners r ON r.id=t.runner_id WHERE t.state<>'closed' AND (t.lease_until<now() OR NOT t.attached) ORDER BY t.created_at LIMIT 100").fetch_all(&s.pool).await?;
    for row in rows {
        if !row.get::<bool, _>("authorized") {
            let _ = stop(s, &row).await;
        }
        refresh(s, &row).await?;
    }
    Ok(())
}

/// Reconcile occupancy while the caller holds the common Session transaction lock.
/// A missed ephemeral close frame must not make the next task skip a free Session.
pub(crate) async fn refresh_session(
    s: &App,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runner: Uuid,
    agent: &str,
    session: &str,
) -> Result<()> {
    let rows=sqlx::query("SELECT * FROM terminals WHERE runner_id=$1 AND agent=$2 AND session_id=$3 AND state<>'closed'").bind(runner).bind(agent).bind(session).fetch_all(&mut **tx).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let runtime: Option<Uuid> = sqlx::query_scalar("SELECT runtime_id FROM runners WHERE id=$1")
        .bind(runner)
        .fetch_one(&mut **tx)
        .await?;
    for row in rows {
        let expired = row.get::<DateTime<Utc>, _>("lease_until") < Utc::now();
        let mut control = request(&row, "status", json!({}));
        if let Some(runtime) = runtime {
            control.runtime_id = runtime;
        }
        let state = if expired && row.get::<String, _>("state") == "created" {
            json!({"state":"closed"})
        } else {
            match rpc(s, control).await {
                Ok(value) => {
                    let state = value
                        .get("state")
                        .filter(|v| v.is_object())
                        .unwrap_or(&value)
                        .clone();
                    if state["state"] == "missing" && expired {
                        json!({"state":"closed"})
                    } else {
                        state
                    }
                }
                Err(_) if expired => json!({"state":"unknown"}),
                Err(_) => continue,
            }
        };
        apply_state(&mut **tx, row.get("id"), row.get("generation"), &state).await?;
    }
    Ok(())
}
