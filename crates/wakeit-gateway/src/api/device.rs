use crate::{auth, protocol, transport::streams as bus};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;
use wakeit_core::config::TaskConfig;

use crate::api::access::device;
use crate::api::http::{ApiJson, Error, Result, true_value};
use crate::state::App;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Enroll {
    token: String,
    name: String,
    public_key: String,
}

pub(crate) async fn enroll(
    State(s): State<App>,
    ApiJson(b): ApiJson<Enroll>,
) -> Result<Json<Value>> {
    wakeit_core::config::validate_id(&b.name).map_err(|e| Error::bad(e.to_string()))?;
    let id = Uuid::new_v4();
    let jwt = s
        .authority
        .runner_jwt(id, &b.public_key)
        .map_err(|e| Error::bad(e.to_string()))?;
    let mut tx = s.pool.begin().await?;
    let enrollment_workspace: Option<Uuid> =
        sqlx::query_scalar("SELECT workspace_id FROM enrollments WHERE hash=$1")
            .bind(auth::credentials::hash(&b.token))
            .fetch_optional(&mut *tx)
            .await?;
    let enrollment_workspace = enrollment_workspace.ok_or_else(|| {
        Error(
            StatusCode::UNAUTHORIZED,
            "enrollment token expired or already used".into(),
        )
    })?;
    crate::store::workspaces::lock(&mut tx, enrollment_workspace).await?;
    let workspace:Option<Uuid>=sqlx::query_scalar("UPDATE enrollments SET used_at=now() WHERE hash=$1 AND used_at IS NULL AND expires_at>now() AND EXISTS(SELECT 1 FROM workspace_members m JOIN users u ON u.id=m.user_id WHERE m.workspace_id=enrollments.workspace_id AND m.user_id=enrollments.created_by AND m.active AND u.active AND m.role IN ('admin','operator')) RETURNING workspace_id").bind(auth::credentials::hash(&b.token)).fetch_optional(&mut *tx).await?;
    if workspace.is_none() {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "enrollment token expired or already used".into(),
        ));
    }
    let workspace = workspace.unwrap();
    let token = auth::credentials::secret();
    sqlx::query(
        "INSERT INTO runners(id,name,token_hash,public_key,workspace_id) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(b.name)
    .bind(auth::credentials::hash(&token))
    .bind(b.public_key)
    .bind(workspace)
    .execute(&mut *tx)
    .await
    .map_err(|_| Error::conflict("runner name or identity already registered"))?;
    bus::provision(&s.js, id, 2).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id":id,"token":token,"jwt":jwt,"nats_url":s.config.nats_public_url}),
    ))
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Hello {
    runtime_id: Uuid,
    agents: Vec<protocol::AgentInfo>,
    #[serde(default)]
    directory_roots: Vec<String>,
    info: Value,
    concurrency: usize,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Heartbeat {
    runtime_id: Uuid,
    #[serde(default)]
    claims: Vec<ClaimPair>,
    info: Value,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct ClaimPair {
    run_id: Uuid,
    claim_id: Uuid,
    #[serde(default = "true_value")]
    monitoring: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct Claim {
    run_id: Uuid,
    claim_id: Uuid,
    runtime_id: Uuid,
    agent_digest: String,
}

pub(crate) async fn hello(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Hello>,
) -> Result<Json<Value>> {
    let id = device(&s, &h).await?;
    if b.concurrency == 0 || b.concurrency > 128 || b.agents.len() > 100 {
        return Err(Error::bad("invalid runner capacity"));
    }
    let n=sqlx::query("UPDATE runners SET runtime_id=$2,last_seen=now(),agents=$3,info=$4,directory_roots=$5 WHERE id=$1 AND (runtime_id IS NULL OR runtime_id=$2 OR last_seen<now()-interval '60 seconds')").bind(id).bind(b.runtime_id).bind(json!(b.agents)).bind(b.info).bind(json!(b.directory_roots)).execute(&s.pool).await?.rows_affected();
    if n == 0 {
        return Err(Error::conflict("another runner process is still active"));
    }
    bus::provision(&s.js, id, b.concurrency).await?;
    Ok(Json(json!({"ok":true})))
}

pub(crate) async fn heartbeat(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Heartbeat>,
) -> Result<Json<Value>> {
    let id = device(&s, &h).await?;
    let n = sqlx::query("UPDATE runners SET last_seen=now(),info=$3 WHERE id=$1 AND runtime_id=$2")
        .bind(id)
        .bind(b.runtime_id)
        .bind(b.info)
        .execute(&s.pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(Error::conflict(
            "runner process no longer owns device lease",
        ));
    }
    let mut closed = Vec::new();
    for c in b.claims {
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM runs WHERE id=$1 AND claim_id=$2 AND runner_id=$3",
        )
        .bind(c.run_id)
        .bind(c.claim_id)
        .bind(id)
        .fetch_optional(&s.pool)
        .await?;
        if state.as_deref().is_some_and(protocol::terminal) {
            closed.push(c.run_id);
        } else if c.monitoring {
            sqlx::query("UPDATE runs SET last_seen=now() WHERE id=$1 AND claim_id=$2 AND runner_id=$3 AND state IN ('running','starting','unknown')").bind(c.run_id).bind(c.claim_id).bind(id).execute(&s.pool).await?;
        }
    }
    Ok(Json(json!({"ok":true,"closed":closed})))
}

pub(crate) async fn claim(
    State(s): State<App>,
    h: HeaderMap,
    ApiJson(b): ApiJson<Claim>,
) -> Result<Json<Value>> {
    let runner = device(&s, &h).await?;
    let mut tx = s.pool.begin().await?;
    let task_row = sqlx::query("SELECT t.id,t.archived_at FROM tasks t JOIN runs r ON r.task_id=t.id AND r.workspace_id=t.workspace_id WHERE r.id=$1 AND r.runner_id=$2 FOR UPDATE OF t").bind(b.run_id).bind(runner).fetch_optional(&mut *tx).await?.ok_or_else(Error::missing)?;
    let graph_parent = crate::store::dag::lock_parent(&mut tx, b.run_id).await?;
    let parent = if let Some(id) = graph_parent {
        Some(
            sqlx::query(
                "SELECT state,started_at,window_started_at,cancel_requested,finished_at,config FROM runs WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?,
        )
    } else {
        None
    };
    let row = sqlx::query("SELECT * FROM runs WHERE id=$1 FOR UPDATE")
        .bind(b.run_id)
        .fetch_one(&mut *tx)
        .await?;
    let owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runners WHERE id=$1 AND runtime_id=$2 AND NOT revoked)",
    )
    .bind(runner)
    .bind(b.runtime_id)
    .fetch_one(&mut *tx)
    .await?;
    if !owner {
        return Err(Error::conflict("runner lease expired"));
    }
    let state: String = row.get("state");
    if row.get::<Option<Uuid>, _>("claim_id") == Some(b.claim_id)
        && row.get::<Option<Uuid>, _>("runtime_id") == Some(b.runtime_id)
        && matches!(state.as_str(), "starting" | "running")
    {
        return Ok(Json(
            json!({"execute":true,"cancel_requested":row.get::<bool,_>("cancel_requested"),"session_title":row.get::<Option<Value>,_>("execution_snapshot").and_then(|v|v.get("session_title").cloned())}),
        ));
    }
    let claim_now: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    if (task_row
        .get::<Option<DateTime<Utc>>, _>("archived_at")
        .is_some()
        && parent
            .as_ref()
            .is_none_or(|p| p.get::<Option<DateTime<Utc>>, _>("started_at").is_none()))
        || parent.as_ref().is_some_and(|p| {
            p.get::<bool, _>("cancel_requested")
                || crate::store::dag_recovery::expired(
                    &serde_json::from_value(p.get("config")).expect("saved graph"),
                    p.get("window_started_at"),
                    claim_now,
                )
                || p.get::<Option<DateTime<Utc>>, _>("finished_at").is_some()
        })
        || state != "queued"
        || row.get::<DateTime<Utc>, _>("scheduled_at") > claim_now
        || row.get::<bool, _>("cancel_requested")
        || row.get::<DateTime<Utc>, _>("deadline") < claim_now
    {
        return Ok(Json(json!({"execute":false,"state":state})));
    }
    let workspace: Uuid = row.get("workspace_id");
    let task: TaskConfig = serde_json::from_value(row.get("config"))?;
    if task.execution.misfire == wakeit_core::config::Misfire::Skip
        && row.get::<String, _>("source") == "schedule"
        && claim_now - row.get::<DateTime<Utc>, _>("scheduled_at") > chrono::Duration::seconds(5)
    {
        sqlx::query(
            "UPDATE runs SET state='skipped',reason='misfire',finished_at=now() WHERE id=$1",
        )
        .bind(b.run_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({"execute":false,"state":"skipped"})));
    }
    if let Some(session) = &task.session {
        let key = format!("session:{}:{}:{}", runner, task.agent, session.id);
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        crate::store::terminals::refresh_session(&s, &mut tx, runner, &task.agent, &session.id)
            .await?;
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE runner_id=$1 AND config->>'agent'=$2 AND COALESCE(session->>'id',config->'session'->>'id')=$3 AND id<>$4 AND state IN ('starting','running','unknown')) OR EXISTS(SELECT 1 FROM terminals WHERE runner_id=$1 AND agent=$2 AND session_id=$3 AND state<>'closed')")
            .bind(runner).bind(&task.agent).bind(&session.id).bind(b.run_id).fetch_one(&mut *tx).await?;
        if busy {
            let state =
                crate::store::dag_recovery::session_conflict_state(row.get("attempt_origin"));
            sqlx::query(
                "UPDATE runs SET state=$2,reason='会话正在使用',finished_at=now() WHERE id=$1",
            )
            .bind(b.run_id)
            .bind(state)
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(Json(json!({"execute":false,"state":state})));
        }
    }
    if graph_parent.is_none() && task.execution.overlap == wakeit_core::config::Overlap::Skip {
        let busy:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE workspace_id=$3 AND task_id=$1 AND id<>$2 AND state IN ('starting','running','unknown'))").bind(&task.id).bind(b.run_id).bind(workspace).fetch_one(&mut *tx).await?;
        if busy {
            return Err(Error::conflict("task has an active run"));
        }
    }
    let mut snapshot: Value = row
        .get::<Option<Value>, _>("execution_snapshot")
        .unwrap_or(json!({}));
    let session_support:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runners n,LATERAL jsonb_array_elements(n.agents) a WHERE n.id=$1 AND a->>'id'=$2 AND a->>'session_supported'='true')").bind(runner).bind(&task.agent).fetch_one(&mut *tx).await?;
    if task.session.is_none() && session_support && snapshot["session_title"].is_null() {
        let timezone: String =
            sqlx::query_scalar("SELECT data->>'timezone' FROM settings WHERE workspace_id=$1")
                .bind(workspace)
                .fetch_one(&mut *tx)
                .await?;
        snapshot["session_title"] = json!(wakeit_core::config::generated_session_title(
            &task.name, claim_now, &timezone
        )?);
        sqlx::query("UPDATE runs SET execution_snapshot=$2 WHERE id=$1")
            .bind(b.run_id)
            .bind(&snapshot)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(parent) = graph_parent {
        sqlx::query(
            "UPDATE runs SET state='running',started_at=COALESCE(started_at,now()),window_started_at=COALESCE(window_started_at,now()) WHERE id=$1",
        )
        .bind(parent)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE runs SET state='starting',claim_id=$2,runtime_id=$3,agent_digest=$4,started_at=now(),last_seen=now() WHERE id=$1").bind(b.run_id).bind(b.claim_id).bind(b.runtime_id).bind(b.agent_digest).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"execute":true,"cancel_requested":false,"session_title":snapshot.get("session_title")}),
    ))
}
