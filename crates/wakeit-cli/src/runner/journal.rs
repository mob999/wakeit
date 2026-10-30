use crate::{
    bus,
    config::AgentConfig,
    protocol::{Assignment, RunEvent},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Journal {
    pub pool: SqlitePool,
    pub dir: PathBuf,
    pub runner: Uuid,
    session_locks: Arc<tokio::sync::Mutex<HashMap<Uuid, File>>>,
}
#[derive(Clone)]
pub struct LocalRun {
    pub assignment: Assignment,
    pub agent: AgentConfig,
    pub claim: Uuid,
    pub phase: String,
    pub job: Option<String>,
    pub started: DateTime<Utc>,
    pub session: Option<crate::runner::sessions::RunSession>,
    pub session_title: Option<String>,
}
impl Journal {
    pub async fn open(dir: &Path, runner: Uuid) -> Result<Self> {
        std::fs::create_dir_all(dir.join("logs"))?;
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(dir.join("journal.sqlite"))
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
            .busy_timeout(Duration::from_secs(10));
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS local_runs (id TEXT PRIMARY KEY,assignment TEXT NOT NULL,agent TEXT NOT NULL,claim TEXT NOT NULL,phase TEXT NOT NULL,job TEXT,started TEXT NOT NULL,seq INTEGER NOT NULL DEFAULT 0,log_bytes INTEGER NOT NULL DEFAULT 0,truncated INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS pending_events(run_id TEXT NOT NULL,seq INTEGER NOT NULL,payload TEXT NOT NULL,published INTEGER NOT NULL DEFAULT 0,rejected INTEGER NOT NULL DEFAULT 0,reject_reason TEXT,PRIMARY KEY(run_id,seq)); CREATE TABLE IF NOT EXISTS cancellations(run_id TEXT PRIMARY KEY);").execute(&pool).await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS event_quarantine(run_id TEXT NOT NULL,seq INTEGER NOT NULL,payload TEXT NOT NULL,reason TEXT NOT NULL,PRIMARY KEY(run_id,seq));")
            .execute(&pool).await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS local_terminals(id TEXT PRIMARY KEY,run_id TEXT NOT NULL,runtime_id TEXT NOT NULL,state TEXT NOT NULL,pid INTEGER,marker TEXT,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").execute(&pool).await?;
        let columns = sqlx::query("PRAGMA table_info(local_runs)")
            .fetch_all(&pool)
            .await?;
        let pending_columns = sqlx::query("PRAGMA table_info(pending_events)")
            .fetch_all(&pool)
            .await?;
        if !pending_columns
            .iter()
            .any(|r| r.get::<String, _>("name") == "rejected")
        {
            sqlx::query(
                "ALTER TABLE pending_events ADD COLUMN rejected INTEGER NOT NULL DEFAULT 0",
            )
            .execute(&pool)
            .await?;
        }
        if !pending_columns
            .iter()
            .any(|r| r.get::<String, _>("name") == "reject_reason")
        {
            sqlx::query("ALTER TABLE pending_events ADD COLUMN reject_reason TEXT")
                .execute(&pool)
                .await?;
        }
        if !columns
            .iter()
            .any(|r| r.get::<String, _>("name") == "session")
        {
            sqlx::query("ALTER TABLE local_runs ADD COLUMN session TEXT")
                .execute(&pool)
                .await?;
        }
        if !columns
            .iter()
            .any(|r| r.get::<String, _>("name") == "session_title")
        {
            sqlx::query("ALTER TABLE local_runs ADD COLUMN session_title TEXT")
                .execute(&pool)
                .await?;
        }
        Ok(Self {
            pool,
            dir: dir.into(),
            runner,
            session_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        })
    }
    pub async fn insert(&self, a: &Assignment, agent: &AgentConfig) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO local_runs(id,assignment,agent,claim,phase,started) VALUES(?,?,?,?, 'intent',?)").bind(a.run_id.to_string()).bind(serde_json::to_string(a)?).bind(serde_json::to_string(agent)?).bind(Uuid::new_v4().to_string()).bind(Utc::now().to_rfc3339()).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn title(&self, id: Uuid, title: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE local_runs SET session_title=COALESCE(session_title,?) WHERE id=?")
            .bind(title)
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn resolved(&self, a: &Assignment) -> Result<()> {
        sqlx::query("UPDATE local_runs SET assignment=? WHERE id=?")
            .bind(serde_json::to_string(a)?)
            .bind(a.run_id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn get(&self, id: Uuid) -> Result<Option<LocalRun>> {
        let row = sqlx::query("SELECT * FROM local_runs WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.map(|r| {
            Ok(LocalRun {
                assignment: serde_json::from_str(&r.get::<String, _>("assignment"))?,
                agent: serde_json::from_str(&r.get::<String, _>("agent"))?,
                claim: r.get::<String, _>("claim").parse()?,
                phase: r.get("phase"),
                session_title: r.get("session_title"),
                session: r
                    .get::<Option<String>, _>("session")
                    .map(|s| serde_json::from_str(&s))
                    .transpose()?,
                job: r.get("job"),
                started: r.get::<String, _>("started").parse()?,
            })
        })
        .transpose()
    }
    pub async fn session(&self, id: Uuid) -> Result<Option<crate::runner::sessions::RunSession>> {
        Ok(self.get(id).await?.and_then(|r| r.session))
    }
    /// Session metadata and its reliable event are committed together, outside content budgets.
    pub async fn record_session(
        &self,
        id: Uuid,
        info: crate::runner::sessions::SessionInfo,
    ) -> Result<()> {
        crate::runner::sessions::validate_id(&info.id)?;
        let mut locks = self.session_locks.lock().await;
        let run = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("run is missing"))?;
        let mut value = crate::runner::sessions::RunSession {
            id: info.id.clone(),
            runner_id: self.runner,
            agent: run.agent.id.clone(),
            cwd: info.cwd,
            ready: false,
            reason: None,
            command: None,
            title: None,
        };
        let expected = run
            .session
            .as_ref()
            .map(|s| s.id.clone())
            .or_else(|| run.assignment.task.session.as_ref().map(|s| s.id.clone()));
        let previous_cwd = run
            .session
            .as_ref()
            .map(|s| s.cwd.as_str())
            .unwrap_or(&run.assignment.task.cwd);
        let same_cwd = Path::new(previous_cwd)
            .canonicalize()
            .ok()
            .zip(Path::new(&value.cwd).canonicalize().ok())
            .is_some_and(|(a, b)| a == b);
        if expected.as_ref().is_some_and(|s| s != &info.id) {
            value = run.session.unwrap_or(value);
            value.id = expected.unwrap();
            value.ready = false;
            value.reason = Some("Agent returned a different session ID".into());
        } else if !same_cwd {
            value = run.session.unwrap_or(value);
            value.ready = false;
            value.reason = Some("Agent returned a different session working directory".into());
        } else if let Some(old) = run.session {
            value = old;
        }
        // Bound execution already holds the same guard in Runner. A fresh session
        // becomes addressable only now, so acquire its guard before publishing it.
        if run.assignment.task.session.is_none()
            && !matches!(run.phase.as_str(), "done" | "unknown")
            && value.reason.is_none()
            && !locks.contains_key(&id)
        {
            match crate::runner::sessions::lock_for(&self.dir, self.runner, &value.agent, &value.id)
            {
                Ok(guard) => {
                    locks.insert(id, guard);
                }
                Err(error) if error.is::<crate::runner::sessions::SessionBusy>() => {
                    value.ready = false;
                    value.reason = Some("会话正在使用".into());
                }
                Err(error) => return Err(error),
            }
        }
        self.save_session(id, value).await
    }
    pub async fn save_session(
        &self,
        id: Uuid,
        session: crate::runner::sessions::RunSession,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let row =
            sqlx::query("UPDATE local_runs SET session=?,seq=seq+1 WHERE id=? RETURNING seq,claim")
                .bind(serde_json::to_string(&session)?)
                .bind(id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let event = RunEvent {
            version: 1,
            runner_id: self.runner,
            run_id: id,
            claim_id: row.get::<String, _>("claim").parse()?,
            seq: row.get("seq"),
            kind: "session".into(),
            data: serde_json::to_value(session)?,
            timestamp: Utc::now(),
        };
        sqlx::query("INSERT INTO pending_events(run_id,seq,payload) VALUES(?,?,?)")
            .bind(id.to_string())
            .bind(event.seq)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn ids(&self) -> Result<Vec<Uuid>> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM local_runs WHERE phase<>'done'")
            .fetch_all(&self.pool)
            .await?;
        ids.into_iter().map(|s| Ok(s.parse()?)).collect()
    }
    pub async fn phase(&self, id: Uuid, phase: &str, job: Option<&str>) -> Result<()> {
        let mut locks = self.session_locks.lock().await;
        sqlx::query("UPDATE local_runs SET phase=?1,job=COALESCE(?2,job),started=CASE WHEN ?1='claimed' AND phase='intent' THEN ?4 ELSE started END WHERE id=?3")
            .bind(phase)
            .bind(job)
            .bind(id.to_string())
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await?;
        if matches!(phase, "done" | "unknown") {
            locks.remove(&id);
        }
        Ok(())
    }
    pub async fn cancel(&self, id: Uuid) -> Result<()> {
        sqlx::query("INSERT OR IGNORE INTO cancellations(run_id) VALUES(?)")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn cancelled(&self, id: Uuid) -> Result<bool> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM cancellations WHERE run_id=?")
                .bind(id.to_string())
                .fetch_one(&self.pool)
                .await?
                > 0,
        )
    }
    pub async fn claims(&self) -> Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT id,claim FROM local_runs WHERE phase IN ('claimed','started','unknown')",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |r| json!({"run_id":r.get::<String,_>("id"),"claim_id":r.get::<String,_>("claim")}),
            )
            .collect())
    }
    pub async fn unknown_count(&self) -> Result<usize> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM local_runs WHERE phase='unknown'")
                .fetch_one(&self.pool)
                .await? as usize,
        )
    }
    pub async fn emit(
        &self,
        id: Uuid,
        kind: &str,
        mut data: Value,
        phase: Option<&str>,
    ) -> Result<()> {
        let mut locks = self.session_locks.lock().await;
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("UPDATE local_runs SET seq=seq+1,phase=COALESCE(?,phase) WHERE id=? RETURNING seq,claim").bind(phase).bind(id.to_string()).fetch_one(&mut *tx).await?;
        if kind == "state" && serde_json::to_vec(&data)?.len() > 1000 * 1024 {
            let original = data["result"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| data["result"].to_string());
            let mut end = original.len().min(64 * 1024);
            while !original.is_char_boundary(end) {
                end -= 1;
            }
            data["result"] = json!({"truncated":true,"preview":&original[..end]});
            data["reason"] = json!(
                "Result exceeded the encoded payload limit; showing a 64 KiB preview. See local logs for additional output."
            );
        }
        let event = RunEvent {
            version: 1,
            runner_id: self.runner,
            run_id: id,
            claim_id: row.get::<String, _>("claim").parse()?,
            seq: row.get("seq"),
            kind: kind.into(),
            data,
            timestamp: Utc::now(),
        };
        sqlx::query("INSERT INTO pending_events(run_id,seq,payload) VALUES(?,?,?)")
            .bind(id.to_string())
            .bind(event.seq)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        if phase.is_some_and(|phase| matches!(phase, "done" | "unknown")) {
            locks.remove(&id);
        }
        Ok(())
    }
    /// Trace snapshots and logs share the same durable, bounded content budget.
    pub async fn content(&self, id: Uuid, kind: &str, data: Value) -> Result<bool> {
        let size = if kind == "log" {
            data["text"].as_str().unwrap_or("").len()
        } else {
            serde_json::to_vec(&data)?.len()
        };
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT log_bytes,truncated FROM local_runs WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<i64, _>("truncated") != 0 {
            return Ok(false);
        }
        if row.get::<i64, _>("log_bytes") + size as i64 > 20 * 1024 * 1024 {
            sqlx::query("UPDATE local_runs SET truncated=1 WHERE id=?")
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            self.emit(
                id,
                "trace_status",
                json!({"status":"truncated","reason":"content_limit"}),
                None,
            )
            .await?;
            self.emit(
                id,
                "truncated",
                json!({"text":"Content limit reached.\n"}),
                None,
            )
            .await?;
            return Ok(false);
        }
        sqlx::query("UPDATE local_runs SET log_bytes=log_bytes+? WHERE id=?")
            .bind(size as i64)
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.emit(id, kind, data, None).await?;
        Ok(true)
    }
    pub async fn log(&self, id: Uuid, stream: &str, text: &str) -> Result<()> {
        if self
            .content(id, "log", json!({"stream":stream,"text":text}))
            .await?
        {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("logs").join(format!("{id}.log")))
                .await?;
            file.write_all(text.as_bytes()).await?;
            file.flush().await?;
        }
        Ok(())
    }
    pub async fn pending(&self, id: Uuid) -> Result<bool> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM pending_events WHERE run_id=? AND published=0",
        )
        .bind(id.to_string())
        .fetch_one(&self.pool)
        .await?
            > 0)
    }
    pub async fn flush(&self, js: &async_nats::jetstream::Context) -> Result<()> {
        let rows=sqlx::query("SELECT run_id,seq FROM pending_events e WHERE published=0 AND rejected=0 AND NOT EXISTS(SELECT 1 FROM pending_events previous WHERE previous.run_id=e.run_id AND previous.rejected=1 AND previous.seq<e.seq) ORDER BY run_id,seq LIMIT 100").fetch_all(&self.pool).await?;
        let mut blocked = HashSet::new();
        for row in rows {
            let run: String = row.get("run_id");
            if blocked.contains(&run) {
                continue;
            }
            let seq: i64 = row.get("seq");
            let payload: Option<String> = sqlx::query_scalar("SELECT payload FROM pending_events WHERE run_id=? AND seq=? AND published=0 AND rejected=0")
                .bind(&run).bind(seq).fetch_optional(&self.pool).await?;
            let Some(payload) = payload else { continue };
            let message_id = format!("event:{}:{run}:{seq}", self.runner);
            let result = bus::publish(
                js,
                crate::protocol::event_subject(self.runner),
                message_id.clone(),
                payload.as_bytes().to_vec(),
            )
            .await;
            if let Err(error) = result {
                if !bus::is_payload_too_large(&error) {
                    return Err(error);
                }
                let replacement = self
                    .quarantine_payload(
                        &run,
                        seq,
                        &payload,
                        bus::payload_capacity(js, &message_id),
                        &error.to_string(),
                    )
                    .await?;
                let Some(replacement) = replacement else {
                    blocked.insert(run);
                    continue;
                };
                // The replacement keeps the original event identity/order and is
                // committed locally before attempting reliable publication.
                bus::publish(
                    js,
                    crate::protocol::event_subject(self.runner),
                    message_id,
                    replacement.into_bytes(),
                )
                .await?;
            }
            sqlx::query("UPDATE pending_events SET published=1 WHERE run_id=? AND seq=?")
                .bind(run)
                .bind(seq)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    async fn quarantine_payload(
        &self,
        run: &str,
        seq: i64,
        payload: &str,
        capacity: usize,
        reason: &str,
    ) -> Result<Option<String>> {
        let replacement = repair_event(payload, capacity)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO event_quarantine(run_id,seq,payload,reason) VALUES(?,?,?,?)",
        )
        .bind(run)
        .bind(seq)
        .bind(payload)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        if let Some(replacement) = &replacement {
            sqlx::query(
                "UPDATE pending_events SET payload=?,reject_reason=? WHERE run_id=? AND seq=?",
            )
            .bind(replacement)
            .bind(reason)
            .bind(run)
            .bind(seq)
            .execute(&mut *tx)
            .await?;
        } else {
            // Required identities that cannot fit are retained, never acknowledged
            // as uploaded. Only this Run is blocked; other Runs can still flush.
            sqlx::query(
                "UPDATE pending_events SET rejected=1,reject_reason=? WHERE run_id=? AND seq=?",
            )
            .bind(reason)
            .bind(run)
            .bind(seq)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        tracing::warn!(
            run_id = run,
            seq,
            repaired = replacement.is_some(),
            "oversized event quarantined"
        );
        Ok(replacement)
    }
    pub async fn prune(&self) -> Result<()> {
        let log_cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let run_cutoff = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM local_runs WHERE phase='done' AND started<? AND NOT EXISTS(SELECT 1 FROM pending_events WHERE run_id=local_runs.id AND published=0)")
            .bind(&log_cutoff).fetch_all(&mut *tx).await?;
        for id in &ids {
            sqlx::query("DELETE FROM event_quarantine WHERE run_id=?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            // Keep the execution tombstone after removing uploaded payloads.
            sqlx::query("DELETE FROM pending_events WHERE run_id=? AND published=1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("DELETE FROM cancellations WHERE run_id IN (SELECT id FROM local_runs WHERE phase='done' AND started<? AND NOT EXISTS(SELECT 1 FROM pending_events WHERE run_id=local_runs.id AND published=0))")
            .bind(&run_cutoff).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM local_runs WHERE phase='done' AND started<? AND NOT EXISTS(SELECT 1 FROM pending_events WHERE run_id=local_runs.id AND published=0) AND NOT EXISTS(SELECT 1 FROM local_terminals WHERE run_id=local_runs.id AND state<>'closed')")
            .bind(run_cutoff).execute(&mut *tx).await?;
        tx.commit().await?;
        for id in ids {
            match tokio::fs::remove_file(self.dir.join("logs").join(format!("{id}.log"))).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

fn repair_event(payload: &str, capacity: usize) -> Result<Option<String>> {
    let mut event: RunEvent = serde_json::from_str(payload)?;
    const NOTE: &str =
        "Output exceeded the transport limit; full event retained in the Runner journal.";
    match event.kind.as_str() {
        "state" => {
            let reason = event.data["reason"]
                .as_str()
                .filter(|text| !text.is_empty())
                .map(|text| format!("{}; {NOTE}", text.chars().take(256).collect::<String>()))
                .unwrap_or_else(|| NOTE.into());
            let mut data = json!({"state":event.data["state"],"reason":reason});
            for field in ["retryable", "job_id"] {
                if let Some(value) = event.data.get(field) {
                    data[field] = value.clone();
                }
            }
            if let Some(result) = event.data.get_mut("result") {
                data["result"] = wakeit_core::capture::bounded_value_with_preview(
                    result.take(),
                    capacity / 3,
                    (capacity / 16).min(64 * 1024),
                );
            }
            event.data = data;
        }
        "session" => {
            // Session ID, runner, Agent and cwd must survive. An oversized native
            // continuation command is optional; do not advertise it as ready.
            event
                .data
                .as_object_mut()
                .context("session object")?
                .remove("command");
            event.data.as_object_mut().unwrap().remove("title");
            event.data["ready"] = json!(false);
            event.data["reason"] = json!(NOTE);
        }
        "snapshot" => {
            let original = event.data;
            event.data = json!({"transport_issue":"event_too_large"});
            for field in [
                "cwd",
                "agent_digest",
                "trace_supported",
                "session_supported",
                "model",
            ] {
                if let Some(value) = original.get(field) {
                    event.data[field] = value.clone();
                }
            }
        }
        _ => {
            event.kind = "trace_status".into();
            event.data = json!({"status":"incomplete","reason":"event_too_large"});
        }
    }
    let mut encoded = serde_json::to_string(&event)?;
    if encoded.len() > capacity && event.kind == "state" {
        event.data.as_object_mut().unwrap().remove("result");
        encoded = serde_json::to_string(&event)?;
    }
    Ok((encoded.len() <= capacity).then_some(encoded))
}

#[cfg(test)]
mod delivery_tests {
    use super::*;

    fn event(kind: &str, data: Value) -> RunEvent {
        RunEvent {
            version: 1,
            runner_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            claim_id: Uuid::new_v4(),
            seq: 7,
            kind: kind.into(),
            data,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn bounded_repairs_preserve_lifecycle_identity_at_both_payload_sizes() {
        for capacity in [1024 * 1024 - 200, 2 * 1024 * 1024 - 200] {
            let original = event(
                "state",
                json!({"state":"succeeded","retryable":false,"result":"\0".repeat(500_000)}),
            );
            let bytes = serde_json::to_string(&original).unwrap();
            let repaired = repair_event(&bytes, capacity).unwrap().unwrap();
            assert!(repaired.len() <= capacity);
            let repaired: RunEvent = serde_json::from_str(&repaired).unwrap();
            assert_eq!(
                (repaired.seq, repaired.run_id, repaired.claim_id),
                (original.seq, original.run_id, original.claim_id)
            );
            assert_eq!(repaired.data["state"], "succeeded");
            assert_eq!(repaired.data["retryable"], false);
            assert_eq!(repaired.data["result"]["truncated"], true);
            let original = event(
                "session",
                json!({"id":"session-7","runner_id":original.runner_id,
                "agent":"pi","cwd":"/work","ready":true,"command":"x".repeat(3*1024*1024)}),
            );
            let repaired = repair_event(&serde_json::to_string(&original).unwrap(), capacity)
                .unwrap()
                .unwrap();
            let repaired: RunEvent = serde_json::from_str(&repaired).unwrap();
            for key in ["id", "runner_id", "agent", "cwd"] {
                assert_eq!(repaired.data[key], original.data[key]);
            }
            assert_eq!(repaired.kind, "session");
            assert_eq!(repaired.data["ready"], false);
            assert!(repaired.data["reason"].is_string());
        }
    }

    #[tokio::test]
    async fn repaired_events_keep_original_sequence_and_unrepairable_identities_stay_pending() {
        let directory = tempfile::tempdir().unwrap();
        let journal = Journal::open(directory.path(), Uuid::new_v4())
            .await
            .unwrap();
        let original = event("log", json!({"stream":"stdout","text":"\0".repeat(1000)}));
        let payload = serde_json::to_string(&original).unwrap();
        sqlx::query("INSERT INTO pending_events(run_id,seq,payload) VALUES(?,?,?)")
            .bind(original.run_id.to_string())
            .bind(original.seq)
            .bind(&payload)
            .execute(&journal.pool)
            .await
            .unwrap();
        let replacement = journal
            .quarantine_payload(
                &original.run_id.to_string(),
                original.seq,
                &payload,
                1024,
                "too large",
            )
            .await
            .unwrap()
            .unwrap();
        let repaired: RunEvent = serde_json::from_str(&replacement).unwrap();
        assert_eq!((repaired.seq, repaired.kind.as_str()), (7, "trace_status"));
        let saved: String =
            sqlx::query_scalar("SELECT payload FROM event_quarantine WHERE run_id=?")
                .bind(original.run_id.to_string())
                .fetch_one(&journal.pool)
                .await
                .unwrap();
        assert_eq!(saved, payload);
        let original = event(
            "session",
            json!({"id":"x".repeat(2000),"cwd":"/work","agent":"pi","runner_id":journal.runner}),
        );
        let payload = serde_json::to_string(&original).unwrap();
        sqlx::query("INSERT INTO pending_events(run_id,seq,payload) VALUES(?,?,?)")
            .bind(original.run_id.to_string())
            .bind(original.seq)
            .bind(&payload)
            .execute(&journal.pool)
            .await
            .unwrap();
        assert!(
            journal
                .quarantine_payload(
                    &original.run_id.to_string(),
                    original.seq,
                    &payload,
                    512,
                    "too large"
                )
                .await
                .unwrap()
                .is_none()
        );
        assert!(journal.pending(original.run_id).await.unwrap());
        let row =
            sqlx::query("SELECT rejected,published,payload FROM pending_events WHERE run_id=?")
                .bind(original.run_id.to_string())
                .fetch_one(&journal.pool)
                .await
                .unwrap();
        assert_eq!(row.get::<i64, _>("rejected"), 1);
        assert_eq!(row.get::<i64, _>("published"), 0);
        assert_eq!(row.get::<String, _>("payload"), payload);
    }
}
