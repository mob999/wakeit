use chrono::Utc;
use serde_json::json;
use uuid::Uuid;
use wakeit::{config::*, process, protocol::Assignment, runner::journal::Journal};
fn task(cwd: &str) -> TaskConfig {
    TaskConfig {
        dag: None,
        version: 1,
        id: "test".into(),
        name: "test".into(),
        runner: "test".into(),
        agent: "codex".into(),
        cwd: cwd.into(),
        prompt: "hello".into(),
        parameters: Default::default(),
        session: None,
        schedule: None,
        enabled: false,
        execution: Default::default(),
    }
}

#[tokio::test]
async fn journal_limits_large_results_and_preserves_pending_uploads() {
    let dir = tempfile::tempdir().unwrap();
    let runner = Uuid::new_v4();
    let id = Uuid::new_v4();
    let agent: AgentConfig = yaml_serde::from_str(builtin("codex").unwrap()).unwrap();
    let a = Assignment {
        version: 1,
        run_id: id,
        runner_id: runner,
        task: task("."),
        deadline: Utc::now(),
    };
    let journal = Journal::open(dir.path(), runner).await.unwrap();
    journal.insert(&a, &agent).await.unwrap();
    journal
        .emit(
            id,
            "state",
            json!({"state":"succeeded","result":"中文".repeat(200_000)}),
            Some("done"),
        )
        .await
        .unwrap();
    let payload: String = sqlx::query_scalar("SELECT payload FROM pending_events WHERE run_id=?")
        .bind(id.to_string())
        .fetch_one(&journal.pool)
        .await
        .unwrap();
    assert!(payload.len() < 1000 * 1024);
    let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(event["data"]["result"]["truncated"], true);
    sqlx::query("UPDATE local_runs SET started=?")
        .bind((Utc::now() - chrono::Duration::days(100)).to_rfc3339())
        .execute(&journal.pool)
        .await
        .unwrap();
    journal.prune().await.unwrap();
    assert!(
        journal.get(id).await.unwrap().is_some(),
        "unpublished results must survive retention"
    );
    sqlx::query("UPDATE pending_events SET published=1")
        .execute(&journal.pool)
        .await
        .unwrap();
    journal.prune().await.unwrap();
    assert!(journal.get(id).await.unwrap().is_none());
}
#[tokio::test]
async fn logs_preserve_utf8_split_across_pipe_reads() {
    let dir = tempfile::tempdir().unwrap();
    let t = task(dir.path().to_str().unwrap());
    let id = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let agent: AgentConfig = yaml_serde::from_str(builtin("codex").unwrap()).unwrap();
    let journal = Journal::open(&dir.path().join("journal"), runner)
        .await
        .unwrap();
    journal
        .insert(
            &Assignment {
                version: 1,
                run_id: id,
                runner_id: runner,
                task: t.clone(),
                deadline: Utc::now(),
            },
            &agent,
        )
        .await
        .unwrap();
    let spec = CommandSpec { command:"node".into(), args:vec!["-e".into(), "const b=Buffer.from('中文🌏');let i=0;const t=setInterval(()=>{process.stdout.write(b.subarray(i,i+1));if(++i===b.length)clearInterval(t)},15)".into()], ..Default::default() };
    let out = process::execute(
        &spec,
        &t,
        id,
        None,
        std::time::Duration::from_secs(5),
        tokio_util::sync::CancellationToken::new(),
        Some(process::LogSink {
            journal: journal.clone(),
            id,
            stdout: true,
        }),
    )
    .await
    .unwrap();
    assert_eq!(out.stdout, "中文🌏");
    assert_eq!(
        tokio::fs::read_to_string(journal.dir.join("logs").join(format!("{id}.log")))
            .await
            .unwrap(),
        "中文🌏"
    );
}
#[tokio::test]
async fn journal_survives_reopen_and_deduplicates() {
    let dir = tempfile::tempdir().unwrap();
    let id = Uuid::new_v4();
    let runner = Uuid::new_v4();
    let agent: AgentConfig = yaml_serde::from_str(builtin("codex").unwrap()).unwrap();
    let a = Assignment {
        version: 1,
        run_id: id,
        runner_id: runner,
        task: task("."),
        deadline: Utc::now() + chrono::Duration::hours(1),
    };
    let journal = Journal::open(dir.path(), runner).await.unwrap();
    journal.insert(&a, &agent).await.unwrap();
    let claim = journal.get(id).await.unwrap().unwrap().claim;
    let old = Utc::now() - chrono::Duration::hours(2);
    sqlx::query("UPDATE local_runs SET started=? WHERE id=?")
        .bind(old.to_rfc3339())
        .bind(id.to_string())
        .execute(&journal.pool)
        .await
        .unwrap();
    journal.phase(id, "claimed", None).await.unwrap();
    let started = journal.get(id).await.unwrap().unwrap().started;
    assert!(
        started > old + chrono::Duration::hours(1),
        "execution timeout starts at claim, not delivery intent"
    );
    journal
        .emit(id, "state", json!({"state":"running"}), Some("started"))
        .await
        .unwrap();
    journal.pool.close().await;
    let journal = Journal::open(dir.path(), runner).await.unwrap();
    journal.insert(&a, &agent).await.unwrap();
    let r = journal.get(id).await.unwrap().unwrap();
    assert_eq!(r.claim, claim);
    assert_eq!(r.phase, "started");
    assert_eq!(
        r.started, started,
        "recovery must preserve the original execution timeout"
    );
    assert!(journal.pending(id).await.unwrap());
    journal.cancel(id).await.unwrap();
    assert!(journal.cancelled(id).await.unwrap());
}
#[tokio::test]
async fn argv_and_stdin_preserve_untrusted_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut t = task(dir.path().to_str().unwrap());
    t.prompt = "中文\n\"quotes\" ' $() ; & {{job.id}}".into();
    let spec = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), "process.stdin.pipe(process.stdout)".into()],
        stdin: Some("{{task.prompt}}".into()),
        ..Default::default()
    };
    let out = process::execute(
        &spec,
        &t,
        Uuid::new_v4(),
        None,
        std::time::Duration::from_secs(30),
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.code, Some(0));
    assert_eq!(out.stdout, t.prompt);
}
#[tokio::test]
async fn task_timeout_caps_the_command_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let spec = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), "setInterval(()=>{},1000)".into()],
        timeout: Some("1h".into()),
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let out = process::execute(
        &spec,
        &task(dir.path().to_str().unwrap()),
        Uuid::new_v4(),
        None,
        std::time::Duration::from_secs(1),
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.stopped, Some("timed_out"));
    assert!(out.confirmed);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
#[tokio::test]
async fn monitoring_failure_is_distinct_from_a_command_that_never_started() {
    let dir = tempfile::tempdir().unwrap();
    let runner = Uuid::new_v4();
    let id = Uuid::new_v4();
    let journal = Journal::open(&dir.path().join("journal"), runner)
        .await
        .unwrap();
    journal.pool.close().await;
    let spec = CommandSpec {
        command: "node".into(),
        args: vec!["-e".into(), "console.log('started')".into()],
        ..Default::default()
    };
    let error = process::execute(
        &spec,
        &task(dir.path().to_str().unwrap()),
        id,
        None,
        std::time::Duration::from_secs(5),
        tokio_util::sync::CancellationToken::new(),
        Some(process::LogSink {
            journal,
            id,
            stdout: true,
        }),
    )
    .await
    .err()
    .unwrap();
    assert!(
        error
            .downcast_ref::<process::CommandStartedError>()
            .is_some()
    );
    let missing = CommandSpec {
        command: "wakeit-test-command-that-does-not-exist".into(),
        ..Default::default()
    };
    let error = process::execute(
        &missing,
        &task(dir.path().to_str().unwrap()),
        id,
        None,
        std::time::Duration::from_secs(5),
        tokio_util::sync::CancellationToken::new(),
        None,
    )
    .await
    .err()
    .unwrap();
    assert!(
        error
            .downcast_ref::<process::CommandStartedError>()
            .is_none()
    );
}
#[cfg(unix)]
#[tokio::test]
async fn cancellation_terminates_descendants() {
    let dir = tempfile::tempdir().unwrap();
    let t = task(dir.path().to_str().unwrap());
    let spec=CommandSpec{command:"node".into(),args:vec!["-e".into(),"const c=require('child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});console.log(c.pid);setInterval(()=>{},1000)".into()],..Default::default()};
    let token = tokio_util::sync::CancellationToken::new();
    let cancel = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        cancel.cancel();
    });
    let out = process::execute(
        &spec,
        &t,
        Uuid::new_v4(),
        None,
        std::time::Duration::from_secs(5),
        token,
        None,
    )
    .await
    .unwrap();
    assert_eq!(out.stopped, Some("cancelled"));
    assert!(out.confirmed);
    let pid: i32 = out.stdout.trim().parse().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let exists = unsafe { libc::kill(pid, 0) } == 0;
    #[cfg(target_os = "linux")]
    let zombie = std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|text| {
        text.lines()
            .any(|line| line.starts_with("State:") && line.contains("Z (zombie)"))
    });
    #[cfg(not(target_os = "linux"))]
    let zombie = false;
    assert!(!exists || zombie, "descendant remains alive");
}

#[tokio::test]
async fn conditional_arguments_and_parameter_overrides_are_literal() {
    let mut agent:AgentConfig=yaml_serde::from_str("version: 1\nid: sample\nname: sample\nmode: sync\nparameters:\n  model: {type: string, default: default-model}\nsubmit:\n  command: node\n  args:\n    - -e\n    - 'console.log(JSON.stringify(process.argv.slice(1)))'\n    - '--'\n    - when_set: model\n      args: ['--model', '{{parameters.model}}']\n").unwrap();
    agent.validate().unwrap();
    let mut t = task(".");
    t.parameters = resolve_parameters(&agent.parameters, &Default::default()).unwrap();
    let run = |t: TaskConfig, spec: CommandSpec| async move {
        // These assertions exercise argv expansion; allow cold Node startup on CI.
        let out = process::execute(
            &spec,
            &t,
            Uuid::new_v4(),
            None,
            std::time::Duration::from_secs(30),
            tokio_util::sync::CancellationToken::new(),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.code, Some(0), "Node command stopped: {:?}", out.stopped);
        out
    };
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &run(t.clone(), agent.submit.clone()).await.stdout
        )
        .unwrap(),
        json!(["--model", "default-model"])
    );
    t.parameters
        .insert("model".into(), "中文 \" $(touch should-not-exist)".into());
    t.parameters = resolve_parameters(&agent.parameters, &t.parameters).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &run(t.clone(), agent.submit.clone()).await.stdout
        )
        .unwrap(),
        json!(["--model", t.parameters["model"]])
    );
    agent.parameters.get_mut("model").unwrap().default = None;
    t.parameters.clear();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&run(t, agent.submit).await.stdout).unwrap(),
        json!([])
    );
}
#[test]
fn directory_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("中文 path");
    std::fs::create_dir(&child).unwrap();
    assert!(process::validate_cwd(&child.to_string_lossy(), &[]).is_err());
    assert!(process::validate_cwd(&child.to_string_lossy(), &[root.path().to_owned()]).is_ok());
    let listing = wakeit::runner::queries::list_directories(
        &[root.path().to_owned()],
        Some(&root.path().to_string_lossy()),
        None,
    )
    .unwrap();
    assert_eq!(listing["directories"][0]["name"], "中文 path");
    std::fs::remove_dir(&child).unwrap();
    assert!(
        wakeit::runner::queries::list_directories(
            &[root.path().to_owned()],
            Some(&child.to_string_lossy()),
            None
        )
        .is_err()
    );
}

#[tokio::test]
async fn agent_logo_configuration_and_runner_manifest() {
    let mut agent: AgentConfig = yaml_serde::from_str(builtin("pi").unwrap()).unwrap();
    agent.detect = None;
    agent.validate().unwrap();
    let info = wakeit::runner::inspect(&agent, std::path::Path::new(".")).await;
    assert_eq!(info.logo.as_deref(), Some("/agent-logos/pi.svg"));
    assert_eq!(
        serde_json::to_value(&info).unwrap()["logo"],
        "/agent-logos/pi.svg"
    );
    for value in ["https://example.com/agent.svg", "/agent-logos/pi.svg"] {
        agent.logo = Some(value.into());
        agent.validate().unwrap();
    }
    for value in [
        "file:///tmp/icon.svg",
        "javascript:alert(1)",
        "//example.com/icon.svg",
        "/\\example.com/icon.svg",
    ] {
        agent.logo = Some(value.into());
        assert!(agent.validate().is_err(), "invalid logo accepted: {value}");
    }
    agent.logo = None;
    agent.validate().unwrap();
    let info = wakeit::runner::inspect(&agent, std::path::Path::new(".")).await;
    assert!(info.logo.is_none());
}
