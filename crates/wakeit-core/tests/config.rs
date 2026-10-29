use chrono::{TimeZone, Utc};
use uuid::Uuid;
use wakeit_core::{config::*, schedule};
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

#[test]
fn builtin_templates_validate() {
    for &(id, _) in BUILTIN_AGENTS {
        let a: AgentConfig = yaml_serde::from_str(builtin(id).unwrap()).unwrap();
        a.validate().unwrap();
        assert_eq!(a.id, id);
        if let Some(logo) = a.logo.as_deref() {
            assert!(logo.starts_with("/agent-logos/"));
            let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../web/public")
                .join(logo.trim_start_matches('/'));
            assert!(file.is_file(), "missing builtin logo: {id}");
        }
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
fn session_template_is_literal_and_requires_explicit_binding() {
    let mut task: wakeit_core::config::TaskConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"session-test","name":"session test","runner":"runner","agent":"agent","cwd":"/project","prompt":"private","schedule":null})).unwrap();
    let id = uuid::Uuid::new_v4();
    assert!(wakeit_core::config::render("{{session.id}}", &task, id, None).is_err());
    task.session = Some(wakeit_core::config::SessionBinding {
        id: "ses_example-full-id".into(),
    });
    task.validate().unwrap();
    assert_eq!(
        wakeit_core::config::render("id={{session.id}}", &task, id, None).unwrap(),
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
    let title = wakeit_core::config::generated_session_title("每日仓库检查", time, "Asia/Shanghai")
        .unwrap();
    assert_eq!(title, "每日仓库检查-wakeit-20261015-150230123");
    assert_eq!(
        wakeit_core::config::generated_session_title("Report", time, "UTC").unwrap(),
        "Report-wakeit-20261015-070230123"
    );
    let long =
        wakeit_core::config::generated_session_title(&"中".repeat(200), time, "Asia/Shanghai")
            .unwrap();
    assert!(long.len() <= 200);
    assert!(long.ends_with("-wakeit-20261015-150230123"));
    assert!(
        wakeit_core::config::generated_session_title("test", time, "invalid/timezone").is_err()
    );
    let task: wakeit_core::config::TaskConfig=serde_json::from_value(serde_json::json!({"version":1,"id":"literal-title","name":"literal","runner":"runner","agent":"agent","cwd":"/project","prompt":"SECRET QUERY","schedule":null})).unwrap();
    let literal = "检查{{task.prompt}} \"quoted\" -wakeit-20261015-150230123";
    assert_eq!(
        wakeit_core::config::render_named(
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
        wakeit_core::config::render("{{session.title}}", &task, uuid::Uuid::new_v4(), None)
            .is_err()
    );
}

#[test]
fn parameter_defaults_and_choices() {
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
}
