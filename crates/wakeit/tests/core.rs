use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;
use wakeit::{config::*, journal::Journal, process, protocol::Assignment, schedule};
fn task(cwd: &str) -> TaskConfig {
    TaskConfig {
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
#[test]
fn builtin_templates_validate() {
    for id in ["codex", "claude-code", "pi", "opencode"] {
        let a: AgentConfig = yaml_serde::from_str(builtin(id).unwrap()).unwrap();
        a.validate().unwrap();
        assert_eq!(a.id, id);
        assert!(
            a.logo
                .as_deref()
                .is_some_and(|logo| logo.starts_with("/agent-logos/"))
        );
    }
}
#[test]
fn template_expansion_is_literal() {
    let mut t = task(".");
    t.prompt = "$(touch /tmp/should-not-exist)\n'\" {{job.id}} 中文".into();
    assert_eq!(
        render("{{task.prompt}}", &t, Uuid::new_v4(), None).unwrap(),
        t.prompt
    );
    assert!(validate_template("{{unknown}}").is_err());
    assert!(validate_template("{{run.id").is_err());
}
#[test]
fn rejects_invalid_contracts() {
    let mut a: AgentConfig = yaml_serde::from_str(builtin("codex").unwrap()).unwrap();
    a.mode = AgentMode::Async;
    assert!(a.validate().is_err());
    a.mode = AgentMode::Sync;
    a.version = 2;
    assert!(a.validate().is_err());
}
#[test]
fn cron_timezone() {
    let s = Schedule {
        at: None,
        every: None,
        cron: Some("0 9 * * *".into()),
        timezone: "Asia/Shanghai".into(),
    };
    let after = Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).unwrap();
    assert_eq!(
        schedule::next(&s, after, false).unwrap().unwrap(),
        Utc.with_ymd_and_hms(2026, 10, 12, 1, 0, 0).unwrap()
    );
}
#[test]
fn dst_fold_runs_once() {
    let s = Schedule {
        at: None,
        every: None,
        cron: Some("30 1 * * *".into()),
        timezone: "America/New_York".into(),
    };
    let first = Utc.with_ymd_and_hms(2026, 11, 1, 5, 30, 0).unwrap();
    let next = schedule::next(&s, first, false).unwrap().unwrap();
    assert_eq!(next, Utc.with_ymd_and_hms(2026, 11, 2, 6, 30, 0).unwrap());
}
#[test]
fn dst_gap_is_skipped() {
    let s = Schedule {
        at: None,
        every: None,
        cron: Some("30 2 * * *".into()),
        timezone: "America/New_York".into(),
    };
    let after = Utc.with_ymd_and_hms(2026, 3, 8, 0, 0, 0).unwrap();
    assert_eq!(
        schedule::next(&s, after, false).unwrap().unwrap(),
        Utc.with_ymd_and_hms(2026, 3, 9, 6, 30, 0).unwrap()
    );
}
#[test]
fn one_time_and_interval() {
    let now = Utc::now();
    let mut s = Schedule {
        at: Some(now),
        every: None,
        cron: None,
        timezone: "UTC".into(),
    };
    assert_eq!(schedule::next(&s, now, true).unwrap(), Some(now));
    assert!(schedule::next(&s, now, false).unwrap().is_none());
    s.at = None;
    s.every = Some("30s".into());
    assert_eq!(
        schedule::next(&s, now, false).unwrap().unwrap(),
        now + chrono::Duration::seconds(30)
    );
    s.cron = Some("* * * * *".into());
    assert!(schedule::validate(&s).is_err());
}
#[test]
fn fractional_interval_keeps_the_original_anchor() {
    let first = Utc::now();
    let now = first + chrono::Duration::milliseconds(4900);
    let due = schedule::latest_interval(first, now, "1.5s").unwrap();
    assert_eq!(due, first + chrono::Duration::milliseconds(4500));
}
#[test]
fn task_limit_includes_json_escaping() {
    let mut t = task(".");
    t.prompt = "\n".repeat(600_000);
    assert!(t.validate().is_err());
}
#[test]
fn openapi_references_resolve_in_the_document() {
    let doc = wakeit::openapi::document();
    fn check(value: &serde_json::Value, doc: &serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(serde_json::Value::as_str) {
                    assert!(
                        doc.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                        "broken reference: {reference}"
                    );
                }
                for value in map.values() {
                    check(value, doc);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    check(value, doc);
                }
            }
            _ => {}
        }
    }
    check(&doc, &doc);
    assert!(
        doc["paths"]["/api/v1/tasks/{id}/runs"]["post"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Idempotency-Key")
    );
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
fn parameter_contract_and_directory_boundaries() {
    use std::collections::BTreeMap;
    let specs = BTreeMap::from([(
        "reasoning".into(),
        ParameterSpec {
            kind: "string".into(),
            label: "reasoning".into(),
            default: Some("low".into()),
            choices: vec!["low".into(), "high".into()],
        },
    )]);
    assert_eq!(
        resolve_parameters(&specs, &BTreeMap::new()).unwrap()["reasoning"],
        "low"
    );
    assert!(resolve_parameters(&specs, &BTreeMap::from([("unknown".into(), "x".into())])).is_err());
    assert!(
        resolve_parameters(
            &specs,
            &BTreeMap::from([("reasoning".into(), "invalid".into())])
        )
        .is_err()
    );
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("中文 path");
    std::fs::create_dir(&child).unwrap();
    assert!(process::validate_cwd(&child.to_string_lossy(), &[]).is_err());
    assert!(process::validate_cwd(&child.to_string_lossy(), &[root.path().to_owned()]).is_ok());
    let listing = wakeit::query::list_directories(
        &[root.path().to_owned()],
        Some(&root.path().to_string_lossy()),
        None,
    )
    .unwrap();
    assert_eq!(listing["directories"][0]["name"], "中文 path");
    std::fs::remove_dir(&child).unwrap();
    assert!(
        wakeit::query::list_directories(
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

#[test]
fn session_template_is_literal_and_requires_explicit_binding() {
    let mut task: wakeit::config::TaskConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"session-test","name":"session test","runner":"runner","agent":"agent","cwd":"/project","prompt":"private","schedule":null})).unwrap();
    let id = uuid::Uuid::new_v4();
    assert!(wakeit::config::render("{{session.id}}", &task, id, None).is_err());
    task.session = Some(wakeit::config::SessionBinding {
        id: "ses_example-full-id".into(),
    });
    task.validate().unwrap();
    assert_eq!(
        wakeit::config::render("id={{session.id}}", &task, id, None).unwrap(),
        "id=ses_example-full-id"
    );
    task.session.as_mut().unwrap().id = "../secret".into();
    assert!(task.validate().is_err());
}

#[test]
fn native_session_titles_follow_workspace_time_preserve_unicode_and_render_once() {
    let time = chrono::DateTime::parse_from_rfc3339("2026-10-15T07:02:30.123Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let title =
        wakeit::config::generated_session_title("每日仓库检查", time, "Asia/Shanghai").unwrap();
    assert_eq!(title, "每日仓库检查-wakeit-20261015-150230123");
    assert_eq!(
        wakeit::config::generated_session_title("Report", time, "UTC").unwrap(),
        "Report-wakeit-20261015-070230123"
    );
    let long =
        wakeit::config::generated_session_title(&"中".repeat(200), time, "Asia/Shanghai").unwrap();
    assert!(long.len() <= 200);
    assert!(long.ends_with("-wakeit-20261015-150230123"));
    assert!(wakeit::config::generated_session_title("test", time, "invalid/timezone").is_err());
    let task: wakeit::config::TaskConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"literal-title","name":"literal","runner":"runner","agent":"agent","cwd":"/project","prompt":"SECRET QUERY","schedule":null})).unwrap();
    let literal = "检查{{task.prompt}} \"quoted\" -wakeit-20261015-150230123";
    assert_eq!(
        wakeit::config::render_named(
            "{{session.title}}",
            &task,
            uuid::Uuid::new_v4(),
            None,
            Some(literal)
        )
        .unwrap(),
        literal
    );
    assert!(
        wakeit::config::render("{{session.title}}", &task, uuid::Uuid::new_v4(), None).is_err()
    );
}
