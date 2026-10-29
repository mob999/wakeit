use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
use wakeit::{
    adapters::stream::Normalizer,
    config::{self, AgentConfig, TaskConfig},
    protocol::{Assignment, RunEvent},
    runner::journal::Journal,
    runner::sessions::{RunSession, SessionInfo},
    trace::{Lines, RecordData, StreamRecord},
};

const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

async fn fixture(bound: bool) -> (tempfile::TempDir, Journal, Uuid) {
    let directory = tempfile::tempdir().unwrap();
    let runner = Uuid::new_v4();
    let journal = Journal::open(directory.path(), runner).await.unwrap();
    let task: TaskConfig = serde_json::from_value(json!({"version":1,"id":"session-fixture","name":"Session","runner":"runner","agent":"pi","cwd":directory.path(),"prompt":"OK","parameters":{},"session":if bound {json!({"id":ID})}else{Value::Null},"schedule":null,"enabled":false})).unwrap();
    let agent: AgentConfig = yaml_serde::from_str(config::builtin("pi").unwrap()).unwrap();
    let run = Uuid::new_v4();
    journal
        .insert(
            &Assignment {
                version: 1,
                run_id: run,
                runner_id: runner,
                task,
                deadline: Utc::now() + chrono::Duration::hours(1),
            },
            &agent,
        )
        .await
        .unwrap();
    (directory, journal, run)
}
fn info(id: &str, cwd: &str) -> SessionInfo {
    SessionInfo {
        version: 1,
        id: id.into(),
        cwd: cwd.into(),
        persisted: false,
        title: None,
    }
}

#[test]
fn all_native_protocols_capture_one_id_and_ignore_repeated_late_identity() {
    for (protocol, source) in [
        ("codex", include_str!("fixtures/sessions/codex.jsonl")),
        (
            "claude-code",
            include_str!("fixtures/sessions/claude-code.jsonl"),
        ),
        ("pi", include_str!("fixtures/sessions/pi.jsonl")),
        ("opencode", include_str!("fixtures/sessions/opencode.jsonl")),
    ] {
        let mut normalizer = Normalizer::new(protocol).unwrap();
        let events: Vec<Value> = source
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let mut output = Vec::new();
        for event in events.iter().chain(events.iter()) {
            output.extend(normalizer.accept(event.clone()));
        }
        let sessions: Vec<_> = output
            .iter()
            .filter_map(|r| {
                if let RecordData::Session(s) = &r.record {
                    Some(s)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(sessions.len(), 1, "{protocol}");
        assert_eq!(sessions[0].id, ID, "{protocol}");
        assert!(
            !sessions[0].persisted,
            "native header alone does not prove persistence"
        );
        assert!(!normalizer.failed(), "{protocol}");
        let mut changed = events[0].clone();
        let key = match protocol {
            "codex" => "thread_id",
            "claude-code" => "session_id",
            "pi" => "id",
            _ => "sessionID",
        };
        changed[key] = json!("different-session");
        let mismatch = normalizer.accept(changed);
        assert!(
            mismatch
                .iter()
                .any(|r| matches!(&r.record,RecordData::Session(s) if s.id=="different-session")),
            "{protocol}"
        );
        assert!(normalizer.failed(), "{protocol}");
        assert!(normalizer.finish().is_empty(), "{protocol}");
    }
}

#[test]
fn unified_session_records_survive_fragmented_unicode_and_reject_paths() {
    let record = StreamRecord::new(RecordData::Session(info(ID, "/tmp/中文 项目")));
    let encoded = serde_json::to_string(&record).unwrap() + "\r\n";
    let mut lines = Lines::default();
    let mut decoded = Vec::new();
    for byte in encoded.as_bytes() {
        decoded.extend(lines.push(&[*byte]));
    }
    assert_eq!(decoded.len(), 1);
    assert!(
        matches!(&decoded[0].as_ref().unwrap().record,RecordData::Session(s) if s.cwd=="/tmp/中文 项目" && s.id==ID)
    );
    for invalid in ["../session.jsonl", "-last", ".", "has spaces"] {
        let invalid = StreamRecord::new(RecordData::Session(info(invalid, "/tmp")));
        assert!(StreamRecord::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
}

#[tokio::test]
async fn session_metadata_survives_content_budget_restart_and_uploaded_log_pruning() {
    let (directory, journal, run) = fixture(false).await;
    sqlx::query("UPDATE local_runs SET log_bytes=20971520,truncated=1")
        .execute(&journal.pool)
        .await
        .unwrap();
    assert!(
        !journal
            .content(run, "log", json!({"text":"over budget"}))
            .await
            .unwrap()
    );
    let cwd = directory.path().to_string_lossy().into_owned();
    journal.record_session(run, info(ID, &cwd)).await.unwrap();
    let mut session = journal.session(run).await.unwrap().unwrap();
    assert!(!session.ready);
    session.ready = true;
    journal.save_session(run, session.clone()).await.unwrap();
    journal.record_session(run, info(ID, &cwd)).await.unwrap();
    assert_eq!(journal.session(run).await.unwrap(), Some(session.clone()));
    let events: Vec<String> = sqlx::query_scalar("SELECT payload FROM pending_events ORDER BY seq")
        .fetch_all(&journal.pool)
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    for (offset, event) in events.iter().enumerate() {
        let event: RunEvent = serde_json::from_str(event).unwrap();
        assert_eq!(event.kind, "session");
        assert_eq!(event.seq, offset as i64 + 1);
        assert_eq!(event.data["id"], ID);
    }
    let runner = journal.runner;
    journal.pool.close().await;
    let journal = Journal::open(directory.path(), runner).await.unwrap();
    assert_eq!(journal.session(run).await.unwrap(), Some(session.clone()));
    sqlx::query("UPDATE pending_events SET published=1")
        .execute(&journal.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE local_runs SET phase='done',started=?")
        .bind((Utc::now() - chrono::Duration::days(8)).to_rfc3339())
        .execute(&journal.pool)
        .await
        .unwrap();
    journal.prune().await.unwrap();
    assert_eq!(journal.session(run).await.unwrap(), Some(session));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM pending_events")
            .fetch_one(&journal.pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT seq FROM local_runs")
            .fetch_one(&journal.pool)
            .await
            .unwrap(),
        3
    );
}

#[tokio::test]
async fn different_ids_and_directories_cannot_replace_the_bound_or_verified_identity() {
    let (directory, journal, run) = fixture(true).await;
    let cwd = directory.path().to_string_lossy().into_owned();
    let assigned = journal.get(run).await.unwrap().unwrap();
    assert_eq!(
        assigned
            .assignment
            .task
            .session
            .as_ref()
            .map(|s| s.id.as_str()),
        Some(ID)
    );
    journal
        .record_session(run, info("wrong-session", &cwd))
        .await
        .unwrap();
    let mismatch = journal.session(run).await.unwrap().unwrap();
    assert_eq!(mismatch.id, ID);
    assert!(!mismatch.ready);
    assert!(mismatch.reason.is_some());
    let good = RunSession {
        id: ID.into(),
        runner_id: journal.runner,
        agent: "pi".into(),
        cwd: cwd.clone(),
        ready: true,
        reason: None,
        command: None,
        title: None,
    };
    journal.save_session(run, good).await.unwrap();
    journal
        .record_session(run, info(ID, "/different-directory"))
        .await
        .unwrap();
    let changed = journal.session(run).await.unwrap().unwrap();
    assert_eq!(changed.id, ID);
    assert_eq!(changed.cwd, cwd);
    assert!(!changed.ready);
    assert!(changed.reason.is_some());
}

#[tokio::test]
async fn fresh_sessions_hold_the_cli_lock_across_clones_until_done_or_unknown() {
    for via_event in [false, true] {
        let (directory, journal, run) = fixture(false).await;
        let cwd = directory.path().to_string_lossy().into_owned();
        journal.record_session(run, info(ID, &cwd)).await.unwrap();
        assert!(
            wakeit::runner::sessions::lock_for(directory.path(), journal.runner, "pi", ID).is_err()
        );
        let shared = journal.clone();
        drop(journal);
        assert!(
            wakeit::runner::sessions::lock_for(directory.path(), shared.runner, "pi", ID).is_err()
        );
        if via_event {
            shared
                .emit(run, "state", json!({"state":"succeeded"}), Some("done"))
                .await
                .unwrap();
        } else {
            shared.phase(run, "done", None).await.unwrap();
        }
        let guard =
            wakeit::runner::sessions::lock_for(directory.path(), shared.runner, "pi", ID).unwrap();
        drop(guard);
    }
    let (directory, journal, run) = fixture(false).await;
    journal
        .record_session(run, info(ID, &directory.path().to_string_lossy()))
        .await
        .unwrap();
    journal
        .emit(run, "state", json!({"state":"unknown"}), Some("unknown"))
        .await
        .unwrap();
    let _guard =
        wakeit::runner::sessions::lock_for(directory.path(), journal.runner, "pi", ID).unwrap();
    let config:wakeit::runner::config::RunnerConfig=serde_json::from_value(json!({"version":1,"gateway":"http://localhost:1","id":journal.runner,"name":"fixture","token_file":"unused","nats_url":"nats://localhost:1","credentials_file":"unused","data_dir":directory.path(),"agents":[],"allowed_roots":[directory.path()]})).unwrap();
    assert!(
        wakeit::runner::sessions::ensure_idle(
            &config,
            &journal.session(run).await.unwrap().unwrap(),
            None
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn fresh_session_lock_conflicts_are_durable_and_bound_runs_reuse_the_runner_guard() {
    let (directory, journal, run) = fixture(false).await;
    let guard =
        wakeit::runner::sessions::lock_for(directory.path(), journal.runner, "pi", ID).unwrap();
    journal
        .record_session(run, info(ID, &directory.path().to_string_lossy()))
        .await
        .unwrap();
    let session = journal.session(run).await.unwrap().unwrap();
    assert!(!session.ready);
    assert_eq!(session.reason.as_deref(), Some("会话正在使用"));
    drop(guard);
    // A repeated native header must not erase an already observed conflict.
    journal
        .record_session(run, info(ID, &directory.path().to_string_lossy()))
        .await
        .unwrap();
    assert_eq!(
        journal.session(run).await.unwrap().unwrap().reason,
        session.reason
    );
    let (directory, journal, run) = fixture(true).await;
    let _runner_guard =
        wakeit::runner::sessions::lock_for(directory.path(), journal.runner, "pi", ID).unwrap();
    journal
        .record_session(run, info(ID, &directory.path().to_string_lossy()))
        .await
        .unwrap();
    assert!(
        journal
            .session(run)
            .await
            .unwrap()
            .unwrap()
            .reason
            .is_none()
    );
}
