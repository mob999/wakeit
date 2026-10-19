use crate::{
    bus,
    config::{Misfire, Overlap, TaskConfig, duration},
    protocol::{self, Assignment, RunEvent},
    schedule,
};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

pub async fn pool(env: &str) -> Result<PgPool> {
    Ok(sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&std::env::var(env).with_context(|| format!("set {env}"))?)
        .await?)
}
pub async fn migrate(pool: &PgPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
pub async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    workspace: Option<Uuid>,
    action: &str,
    target: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO audit(user_id,action,target,workspace_id) VALUES($1,$2,$3,$4)")
        .bind(user)
        .bind(action)
        .bind(target)
        .bind(workspace)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
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

pub async fn apply_task(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    c: &TaskConfig,
    expected: Option<i64>,
    user: Uuid,
) -> Result<Option<Value>> {
    let mut normalized = c.clone();
    if let Some(schedule) = &mut normalized.schedule
        && schedule.timezone.is_empty()
    {
        let settings: Value = sqlx::query_scalar("SELECT data FROM settings WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(pool)
            .await?;
        schedule.timezone = settings["timezone"]
            .as_str()
            .unwrap_or("Asia/Shanghai")
            .into();
    }
    let c = &normalized;
    c.validate()?;
    let mut tx = pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    crate::workspaces::check_member(&mut tx, credential, workspace, "operator").await?;
    // Serialize creation too; an absent row cannot be protected with FOR UPDATE.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:{}", c.id))
        .execute(&mut *tx)
        .await?;
    let old = sqlx::query(
        "SELECT revision,config,next_at,enabled,archived_at FROM tasks WHERE id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(&c.id)
    .bind(workspace).fetch_optional(&mut *tx)
    .await?;
    ensure!(
        old.as_ref()
            .is_none_or(|r| r.get::<Option<DateTime<Utc>>, _>("archived_at").is_none()),
        "task is archived; restore before editing"
    );
    let current = old.as_ref().map(|r| r.get::<i64, _>("revision"));
    if current != expected && !(current.is_none() && expected == Some(0)) {
        return Ok(None);
    }
    let runner =
        sqlx::query("SELECT id,agents FROM runners WHERE workspace_id=$2 AND (name=$1 OR id::text=$1) AND NOT revoked")
            .bind(&c.runner)
            .bind(workspace).fetch_optional(&mut *tx)
            .await?
            .context("runner does not exist or is revoked")?;
    let agents: Value = runner.get("agents");
    ensure!(
        agents
            .as_array()
            .is_some_and(|a| a.iter().any(|a| a["id"] == c.agent)),
        "agent is not advertised by this runner"
    );
    let declared = agents
        .as_array()
        .and_then(|a| a.iter().find(|a| a["id"] == c.agent))
        .context("agent unavailable")?;
    ensure!(
        c.session.is_none() || declared["session_supported"] == true,
        "agent does not advertise session support"
    );
    let specs = serde_json::from_value(declared.get("parameters").cloned().unwrap_or(json!({})))?;
    crate::config::resolve_parameters(&specs, &c.parameters)?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let config = serde_json::to_value(c)?;
    let unchanged = old.as_ref().is_some_and(|r| {
        r.get::<Value, _>("config")["schedule"] == config["schedule"]
            && r.get::<bool, _>("enabled") == c.enabled
    });
    let next_at = if unchanged {
        old.as_ref()
            .and_then(|r| r.get::<Option<DateTime<Utc>>, _>("next_at"))
    } else if c.enabled {
        match &c.schedule {
            Some(s) => {
                if let Some(at) = s.at {
                    Some(at)
                } else {
                    schedule::next(s, now, false)?
                }
            }
            None => None,
        }
    } else {
        None
    };
    let row:Value=sqlx::query_scalar("INSERT INTO tasks(workspace_id,id,runner_id,config,enabled,next_at,created_by,updated_by) VALUES($7,$1,$2,$3,$4,$5,$6,$6) ON CONFLICT(workspace_id,id) DO UPDATE SET runner_id=excluded.runner_id,config=excluded.config,enabled=excluded.enabled,next_at=excluded.next_at,revision=tasks.revision+1,updated_by=excluded.updated_by,updated_at=now() RETURNING to_jsonb(tasks.*)")
        .bind(&c.id).bind(runner.get::<Uuid,_>("id")).bind(config).bind(c.enabled).bind(next_at).bind(user).bind(workspace).fetch_one(&mut *tx).await?;
    if !c.enabled {
        sqlx::query("UPDATE runs SET state='cancelled',reason='task paused',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND state='queued' AND source='schedule'").bind(&c.id).bind(workspace).execute(&mut *tx).await?;
    }
    audit(&mut tx, user, Some(workspace), "task.apply", &c.id).await?;
    tx.commit().await?;
    Ok(Some(row))
}

pub async fn set_archived(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    id: &str,
    expected: i64,
    archived: bool,
    user: Uuid,
) -> Result<Option<Value>> {
    let mut tx = pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    crate::workspaces::check_member(&mut tx, credential, workspace, "operator").await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{workspace}:{id}"))
        .execute(&mut *tx)
        .await?;
    let Some(row) = sqlx::query(
        "SELECT to_jsonb(t.*) AS item FROM tasks t WHERE workspace_id=$2 AND id=$1 FOR UPDATE",
    )
    .bind(id)
    .bind(workspace)
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let old: Value = row.get("item");
    if old["archived_at"].is_null() == !archived {
        return Ok(Some(old));
    }
    ensure!(
        old["revision"].as_i64() == Some(expected),
        "task changed; reload before saving"
    );
    let result = sqlx::query_scalar("UPDATE tasks SET archived_at=CASE WHEN $2 THEN now() ELSE NULL END,enabled=false,config=jsonb_set(config,'{enabled}','false'),next_at=NULL,revision=revision+1,updated_by=$3,updated_at=now() WHERE workspace_id=$4 AND id=$1 RETURNING to_jsonb(tasks.*)").bind(id).bind(archived).bind(user).bind(workspace).fetch_one(&mut *tx).await?;
    if archived {
        sqlx::query("UPDATE runs SET state='cancelled',cancel_requested=true,reason='task archived',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND state='queued'").bind(id).bind(workspace).execute(&mut *tx).await?;
    }
    audit(
        &mut tx,
        user,
        Some(workspace),
        if archived {
            "task.archive"
        } else {
            "task.restore"
        },
        id,
    )
    .await?;
    tx.commit().await?;
    Ok(Some(result))
}

struct NewRun<'a> {
    workspace: Uuid,
    task: &'a TaskConfig,
    revision: i64,
    runner: Uuid,
    at: DateTime<Utc>,
    source: &'a str,
    retry: Option<Uuid>,
    key: Option<String>,
    state: &'a str,
    reason: Option<&'a str>,
}
async fn create_run(tx: &mut Transaction<'_, Postgres>, run: NewRun<'_>) -> Result<Uuid> {
    let NewRun {
        workspace,
        task,
        revision,
        runner,
        at,
        source,
        retry,
        key,
        state,
        reason,
    } = run;
    if source == "schedule"
        && let Some(id) = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM runs WHERE workspace_id=$3 AND task_id=$1 AND scheduled_at=$2 AND source='schedule'",
        )
        .bind(&task.id)
        .bind(at)
        .bind(workspace).fetch_optional(&mut **tx)
        .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    let deadline = at + chrono::Duration::from_std(duration(&task.execution.start_grace)?)?;
    sqlx::query("INSERT INTO runs(workspace_id,id,task_id,task_revision,runner_id,config,scheduled_at,deadline,source,retry_of,request_key,state,reason,finished_at) VALUES($13,$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,CASE WHEN $11='skipped' THEN now() ELSE NULL END)")
        .bind(id).bind(&task.id).bind(revision).bind(runner).bind(serde_json::to_value(task)?).bind(at).bind(deadline).bind(source).bind(retry).bind(key).bind(state).bind(reason).bind(workspace).execute(&mut **tx).await?;
    if state == "queued" {
        enqueue(
            tx,
            workspace,
            protocol::work_subject(runner),
            format!("start:{id}"),
            serde_json::to_value(Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: task.clone(),
                deadline,
            })?,
        )
        .await?;
    }
    Ok(id)
}
pub async fn trigger(
    pool: &PgPool,
    workspace: Uuid,
    credential: &str,
    task_id: &str,
    user: Uuid,
    retry: Option<Uuid>,
    key: &str,
) -> Result<Uuid> {
    let mut tx = pool.begin().await?;
    crate::workspaces::lock(&mut tx, workspace).await?;
    crate::workspaces::check_member(&mut tx, credential, workspace, "operator").await?;
    let request_key = format!(
        "{user}:{}:{key}",
        retry
            .map(|i| i.to_string())
            .unwrap_or_else(|| task_id.into())
    );
    let row = sqlx::query("SELECT * FROM tasks WHERE workspace_id=$2 AND id=$1 FOR UPDATE")
        .bind(task_id)
        .bind(workspace)
        .fetch_optional(&mut *tx)
        .await?
        .context("task not found")?;
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM runs WHERE workspace_id=$2 AND request_key=$1",
    )
    .bind(&request_key)
    .bind(workspace)
    .fetch_optional(&mut *tx)
    .await?
    {
        return Ok(id);
    }
    ensure!(
        row.get::<Option<DateTime<Utc>>, _>("archived_at").is_none(),
        "task is archived; restore before triggering"
    );
    let task: TaskConfig = serde_json::from_value(row.get("config"))?;
    let revoked: bool = sqlx::query_scalar("SELECT revoked FROM runners WHERE id=$1")
        .bind(row.get::<Uuid, _>("runner_id"))
        .fetch_one(&mut *tx)
        .await?;
    ensure!(!revoked, "assigned runner has been revoked");
    let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$2 AND task_id=$1 AND state IN ('queued','starting','running','unknown'))").bind(task_id).bind(workspace).fetch_one(&mut *tx).await?;
    ensure!(
        !busy || task.execution.overlap == Overlap::Allow,
        "task already has an active run"
    );
    if let Some(previous) = retry {
        let state: String = sqlx::query_scalar(
            "SELECT state FROM runs WHERE workspace_id=$3 AND id=$1 AND task_id=$2",
        )
        .bind(previous)
        .bind(task_id)
        .bind(workspace)
        .fetch_one(&mut *tx)
        .await?;
        ensure!(
            protocol::terminal(&state),
            "resolve the previous run before retrying"
        );
    }
    let now = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let id = create_run(
        &mut tx,
        NewRun {
            workspace,
            task: &task,
            revision: row.get("revision"),
            runner: row.get("runner_id"),
            at: now,
            source: if retry.is_some() { "retry" } else { "manual" },
            retry,
            key: Some(request_key),
            state: "queued",
            reason: None,
        },
    )
    .await?;
    audit(
        &mut tx,
        user,
        Some(workspace),
        "run.trigger",
        &id.to_string(),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn tick(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await?;
    let now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let rows=sqlx::query("SELECT * FROM tasks WHERE enabled AND archived_at IS NULL AND next_at<=now() AND EXISTS(SELECT 1 FROM workspaces w WHERE w.id=tasks.workspace_id AND NOT w.is_demo) ORDER BY next_at LIMIT 50 FOR UPDATE SKIP LOCKED").fetch_all(&mut *tx).await?;
    for row in rows {
        let workspace: Uuid = row.get("workspace_id");
        let task: TaskConfig = serde_json::from_value(row.get("config"))?;
        let Some(s) = &task.schedule else { continue };
        let first: DateTime<Utc> = row.get("next_at");
        let due = if let Some(e) = &s.every {
            schedule::latest_interval(first, now, e)?
        } else if s.cron.is_some() {
            schedule::previous_cron(s, now)?.max(first)
        } else {
            first
        };
        let next = if s.every.is_some() {
            schedule::next(s, due, false)?
        } else {
            schedule::next(s, now, false)?
        };
        sqlx::query("UPDATE tasks SET next_at=$2 WHERE workspace_id=$3 AND id=$1")
            .bind(&task.id)
            .bind(next)
            .bind(workspace)
            .execute(&mut *tx)
            .await?;
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$2 AND task_id=$1 AND state IN ('starting','running','unknown'))").bind(&task.id).bind(workspace).fetch_one(&mut *tx).await?;
        let late = (now - due).to_std().unwrap_or_default();
        let reason = if busy && task.execution.overlap == Overlap::Skip {
            Some("overlap")
        } else if late > duration(&task.execution.start_grace)? {
            Some("start deadline elapsed")
        } else if task.execution.misfire == Misfire::Skip && late.as_secs() > 5 {
            Some("misfire")
        } else {
            None
        };
        // A task row lock is also taken by claim, so superseding cannot race a start.
        if task.execution.misfire == Misfire::Latest {
            sqlx::query("UPDATE runs SET state='skipped',reason='superseded by latest occurrence',finished_at=now() WHERE workspace_id=$2 AND task_id=$1 AND source='schedule' AND state='queued'").bind(&task.id).bind(workspace).execute(&mut *tx).await?;
        }
        create_run(
            &mut tx,
            NewRun {
                workspace,
                task: &task,
                revision: row.get("revision"),
                runner: row.get("runner_id"),
                at: due,
                source: "schedule",
                retry: None,
                key: None,
                state: if reason.is_some() {
                    "skipped"
                } else {
                    "queued"
                },
                reason,
            },
        )
        .await?;
    }
    tx.commit().await?;
    sqlx::query("UPDATE runs SET state='skipped',reason='start deadline elapsed',finished_at=now() WHERE state='queued' AND deadline<now()").execute(pool).await?;
    sqlx::query("UPDATE runs SET state='skipped',reason='misfire',finished_at=now() WHERE state='queued' AND source='schedule' AND config->'execution'->>'misfire'='skip' AND scheduled_at<now()-interval '5 seconds'").execute(pool).await?;
    // A missing heartbeat is ambiguous, not permission to start another execution.
    sqlx::query("UPDATE runs SET state='unknown',reason='runner heartbeat lost' WHERE state IN ('starting','running') AND COALESCE(last_seen,started_at)<now()-interval '90 seconds'").execute(pool).await?;
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
pub async fn ingest(pool: &PgPool, e: &RunEvent, log_limit: usize) -> Result<()> {
    ensure!(e.version == 1, "unsupported event version");
    let mut tx = pool.begin().await?;
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
        data = json!({"status":"incomplete"});
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
            data = json!({"status":"truncated"});
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
pub async fn clear_completed_messages(
    pool: &PgPool,
    js: &async_nats::jetstream::Context,
) -> Result<()> {
    let rows=sqlx::query("SELECT o.id,o.stream,o.stream_seq FROM outbox o LEFT JOIN runs r ON r.id=(o.payload->>'run_id')::uuid AND r.workspace_id=o.workspace_id WHERE o.published_at IS NOT NULL AND o.stream_seq IS NOT NULL AND o.cleared_at IS NULL AND (r.id IS NULL OR (r.state IN ('succeeded','failed','cancelled','timed_out','skipped','interrupted') AND (r.claim_id IS NULL OR r.finished_at<now()-interval '5 minutes'))) ORDER BY o.id LIMIT 100").fetch_all(pool).await?;
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
