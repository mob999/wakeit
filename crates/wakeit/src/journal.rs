use crate::{
    bus,
    config::AgentConfig,
    protocol::{Assignment, RunEvent},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use std::{
    collections::HashMap,
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
    pub session: Option<crate::sessions::RunSession>,
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
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS local_runs (id TEXT PRIMARY KEY,assignment TEXT NOT NULL,agent TEXT NOT NULL,claim TEXT NOT NULL,phase TEXT NOT NULL,job TEXT,started TEXT NOT NULL,seq INTEGER NOT NULL DEFAULT 0,log_bytes INTEGER NOT NULL DEFAULT 0,truncated INTEGER NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS pending_events(run_id TEXT NOT NULL,seq INTEGER NOT NULL,payload TEXT NOT NULL,published INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(run_id,seq)); CREATE TABLE IF NOT EXISTS cancellations(run_id TEXT PRIMARY KEY);").execute(&pool).await?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS local_terminals(id TEXT PRIMARY KEY,run_id TEXT NOT NULL,runtime_id TEXT NOT NULL,state TEXT NOT NULL,pid INTEGER,marker TEXT,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").execute(&pool).await?;
        let columns = sqlx::query("PRAGMA table_info(local_runs)")
            .fetch_all(&pool)
            .await?;
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
    pub async fn session(&self, id: Uuid) -> Result<Option<crate::sessions::RunSession>> {
        Ok(self.get(id).await?.and_then(|r| r.session))
    }
    /// Session metadata and its reliable event are committed together, outside content budgets.
    pub async fn record_session(&self, id: Uuid, info: crate::sessions::SessionInfo) -> Result<()> {
        crate::sessions::validate_id(&info.id)?;
        let mut locks = self.session_locks.lock().await;
        let run = self
            .get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("run is missing"))?;
        let mut value = crate::sessions::RunSession {
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
            match crate::sessions::lock_for(&self.dir, self.runner, &value.agent, &value.id) {
                Ok(guard) => {
                    locks.insert(id, guard);
                }
                Err(error)
                    if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                        error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                    }) =>
                {
                    value.ready = false;
                    value.reason = Some("会话正在使用".into());
                }
                Err(error) => return Err(error),
            }
        }
        self.save_session(id, value).await
    }
    pub async fn save_session(&self, id: Uuid, session: crate::sessions::RunSession) -> Result<()> {
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
        let rows=sqlx::query("SELECT run_id,seq,payload FROM pending_events WHERE published=0 ORDER BY run_id,seq LIMIT 100").fetch_all(&self.pool).await?;
        for r in rows {
            let run: String = r.get("run_id");
            let seq: i64 = r.get("seq");
            let payload: String = r.get("payload");
            bus::publish(
                js,
                crate::protocol::event_subject(self.runner),
                format!("event:{}:{run}:{seq}", self.runner),
                payload.into_bytes(),
            )
            .await?;
            sqlx::query("UPDATE pending_events SET published=1 WHERE run_id=? AND seq=?")
                .bind(run)
                .bind(seq)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
    pub async fn prune(&self) -> Result<()> {
        let log_cutoff = (Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let run_cutoff = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM local_runs WHERE phase='done' AND started<? AND NOT EXISTS(SELECT 1 FROM pending_events WHERE run_id=local_runs.id AND published=0)")
            .bind(&log_cutoff).fetch_all(&mut *tx).await?;
        for id in &ids {
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
