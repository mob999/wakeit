#[path = "../../../fixtures/official_agent.rs"]
mod support;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

fn native_cwd(path: &Path) -> std::path::PathBuf {
    let path = fs::canonicalize(path).unwrap();
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(unc) = value.strip_prefix("\\\\?\\UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{unc}"));
        }
        if let Some(value) = value.strip_prefix("\\\\?\\") {
            return std::path::PathBuf::from(value);
        }
    }
    path
}

fn inspect(protocol: &str, id: &str, cwd: &Path, home: &Path, storage: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
            "session",
            "inspect",
            protocol,
            id,
        ])
        .current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("PI_CODING_AGENT_SESSION_DIR", storage)
        .env("CLAUDE_CONFIG_DIR", storage)
        .env_remove("CLAUDE_CODE_PROJECT_DIR_NAME")
        .output()
        .unwrap()
}

fn write_pi(storage: &Path, id: &str, cwd: &Path) {
    fs::create_dir_all(storage).unwrap();
    fs::write(storage.join(format!("2026-10-15T00-00-00Z_{id}.jsonl")), format!("{}\n", json!({"type":"session","version":3,"id":id,"timestamp":"2026-10-15T00:00:00Z","cwd":cwd}))).unwrap();
}

#[test]
fn pi_exact_session_inspection_never_creates_or_accepts_a_prefix() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("sessions");
    write_pi(&storage, ID, &cwd);
    let before = fs::read_dir(&storage).unwrap().count();
    let result = inspect("pi", ID, &cwd, &cwd, &storage);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let metadata: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        metadata,
        json!({"version":1,"id":ID,"cwd":cwd,"persisted":true,"title":null})
    );
    for id in [
        "550e8400",
        "not-found",
        "../sessions/anything",
        "memory-session",
    ] {
        assert!(!inspect("pi", id, &cwd, &cwd, &storage).status.success());
    }
    assert_eq!(fs::read_dir(&storage).unwrap().count(), before);
    let mismatched = cwd.join("other");
    fs::create_dir(&mismatched).unwrap();
    assert!(
        !inspect("pi", ID, &mismatched, &cwd, &storage)
            .status
            .success()
    );
}

#[test]
fn pi_rejects_duplicate_invalid_and_empty_session_metadata() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("sessions");
    write_pi(&storage, ID, &cwd);
    let path = storage.join(format!("2026-10-15T00-00-00Z_{ID}.jsonl"));
    fs::write(
        storage.join(format!("another_{ID}.jsonl")),
        fs::read(&path).unwrap(),
    )
    .unwrap();
    assert!(!inspect("pi", ID, &cwd, &cwd, &storage).status.success());
    fs::remove_file(storage.join(format!("another_{ID}.jsonl"))).unwrap();
    for body in [
        String::new(),
        "{}\n".into(),
        format!(
            "{}\n",
            json!({"type":"session","version":3,"id":"wrong","cwd":cwd})
        ),
    ] {
        fs::write(&path, &body).unwrap();
        assert!(!inspect("pi", ID, &cwd, &cwd, &storage).status.success());
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
    }
}

#[test]
fn claude_reads_only_exact_project_metadata_and_rejects_directory_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("claude");
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let project = storage.join("projects").join(encoded);
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{ID}.jsonl"));
    fs::write(&path, format!("{{\"type\":\"queue-operation\"}}\n{}\nnot-json-after-header",json!({"type":"user","sessionId":ID,"cwd":cwd,"message":{"role":"user","content":"not returned"}}))).unwrap();
    let result = inspect("claude-code", ID, &cwd, &cwd, &storage);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["id"], ID);
    assert_eq!(value.as_object().unwrap().len(), 5);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("not returned"));
    assert!(
        !inspect(
            "claude-code",
            "550e8400-e29b-41d4-a716-446655440001",
            &cwd,
            &cwd,
            &storage
        )
        .status
        .success()
    );
    fs::write(
        &path,
        format!("{}\n", json!({"sessionId":ID,"cwd":storage})),
    )
    .unwrap();
    assert!(
        !inspect("claude-code", ID, &cwd, &cwd, &storage)
            .status
            .success()
    );
}

#[test]
fn resume_missing_session_does_not_execute_the_supplied_command() {
    let root = tempfile::tempdir().unwrap();
    let storage = root.path().join("sessions");
    fs::create_dir(&storage).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
            "session",
            "resume",
            "pi",
            ID,
            "--",
            env!("CARGO_BIN_EXE_wakeit"),
            "--version",
        ])
        .current_dir(root.path())
        .env("PI_CODING_AGENT_SESSION_DIR", storage)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        !String::from_utf8_lossy(&result.stdout)
            .contains(&format!("wakeit {}", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn resume_identity_mismatch_is_failure_even_with_native_exit_zero() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("sessions");
    write_pi(&storage, ID, &cwd);
    let code = r#"console.log(JSON.stringify({type:'session',id:'different-session',cwd:process.cwd()}));console.log(JSON.stringify({type:'message_end',message:{role:'assistant',content:[{type:'text',text:'must not become successful report'}],stopReason:'stop'}}))"#;
    let result = Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
            "session",
            "resume",
            "pi",
            ID,
            "--",
            "node",
            "-e",
            code,
        ])
        .current_dir(&cwd)
        .env("PI_CODING_AGENT_SESSION_DIR", storage)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("different session ID"));
    assert!(!String::from_utf8_lossy(&result.stdout).contains("must not become successful report"));
}

fn list(protocol: &str, cwd: &Path, storage: &Path, more: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wakeit"))
        .args([
            "plugin",
            "exec",
            "git:github.com/mob999/wakeit",
            "adapter",
            "--",
            "session",
            "list",
            protocol,
        ])
        .args(more)
        .current_dir(cwd)
        .env("HOME", cwd)
        .env("USERPROFILE", cwd)
        .env("PI_CODING_AGENT_SESSION_DIR", storage)
        .env("CLAUDE_CONFIG_DIR", storage)
        .env_remove("CLAUDE_CODE_PROJECT_DIR_NAME")
        .output()
        .unwrap()
}
#[test]
fn pi_catalog_is_project_scoped_searchable_and_keyset_paginated() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("sessions");
    write_pi(&storage, ID, &cwd);
    write_pi(&storage, "another-complete-id", &cwd);
    let other = cwd.join("other");
    fs::create_dir(&other).unwrap();
    write_pi(&storage, "outside-project", &other);
    let first = list("pi", &cwd, &storage, &["--limit", "1"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["sessions"].as_array().unwrap().len(), 1);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = list("pi", &cwd, &storage, &["--limit", "1", "--cursor", cursor]);
    assert!(second.status.success());
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_ne!(second["sessions"][0]["id"], first["sessions"][0]["id"]);
    assert!(second["next_cursor"].is_null());
    assert_ne!(second["sessions"][0]["id"], "outside-project");
    let search = list("pi", &cwd, &storage, &["--query", "another"]);
    assert!(search.status.success());
    let search: Value = serde_json::from_slice(&search.stdout).unwrap();
    assert_eq!(search["sessions"][0]["id"], "another-complete-id");
    assert!(
        !list(
            "pi",
            &cwd,
            &storage,
            &["--query", "another", "--cursor", cursor]
        )
        .status
        .success()
    );
}
#[test]
fn claude_catalog_uses_project_metadata_without_reading_message_bodies() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("claude");
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let project = storage.join("projects").join(encoded);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join(format!("{ID}.jsonl")),
        "not JSON; transcript contents are not metadata",
    )
    .unwrap();
    fs::write(project.join("invalid.jsonl"), "data").unwrap();
    fs::write(project.join("sessions-index.json"),json!({"entries":[{"sessionId":ID,"customTitle":"项目检查","summary":"ignored summary","firstPrompt":"must not be returned"}]}).to_string()).unwrap();
    let output = list("claude-code", &cwd, &storage, &["--query", "项目"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(value["sessions"][0]["title"], "项目检查");
    assert_eq!(value["sessions"][0]["id"], ID);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("must not be returned"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("transcript contents"));
}

#[test]
fn pi_catalog_uses_the_latest_real_session_name_and_never_a_query_preview() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("sessions");
    write_pi(&storage, ID, &cwd);
    let file = storage.join(format!("2026-10-15T00-00-00Z_{ID}.jsonl"));
    use std::io::Write;
    let mut output = fs::OpenOptions::new().append(true).open(&file).unwrap();
    writeln!(
        output,
        "{}",
        json!({"type":"message","message":{"role":"user","content":"private first query"}})
    )
    .unwrap();
    writeln!(output, "{}", json!({"type":"session_info","name":"旧名称"})).unwrap();
    writeln!(
        output,
        "{}",
        json!({"type":"message","message":{"content":"x".repeat(2*1024*1024)}})
    )
    .unwrap();
    writeln!(
        output,
        "{}",
        json!({"type":"session_info","name":"每日检查-wakeit-1792022400000"})
    )
    .unwrap();
    drop(output);
    let result = list("pi", &cwd, &storage, &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value["sessions"][0]["title"],
        "每日检查-wakeit-1792022400000"
    );
    let inspected = inspect("pi", ID, &cwd, &cwd, &storage);
    assert!(inspected.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&inspected.stdout).unwrap()["title"],
        "每日检查-wakeit-1792022400000"
    );
    assert!(!String::from_utf8_lossy(&result.stdout).contains("private first query"));
    let mut output = fs::OpenOptions::new().append(true).open(&file).unwrap();
    writeln!(output, "{}", json!({"type":"session_info"})).unwrap();
    drop(output);
    let result = list("pi", &cwd, &storage, &[]);
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(value["sessions"][0]["title"].is_null());
}

#[test]
fn claude_inspection_and_catalog_prefer_actual_custom_title_over_stale_index() {
    let root = tempfile::tempdir().unwrap();
    let cwd = native_cwd(root.path());
    let storage = cwd.join("claude");
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let project = storage.join("projects").join(encoded);
    fs::create_dir_all(&project).unwrap();
    fs::write(
        project.join("sessions-index.json"),
        json!({"entries":[{"sessionId":ID,"customTitle":"stale"}]}).to_string(),
    )
    .unwrap();
    let file = project.join(format!("{ID}.jsonl"));
    fs::write(
        &file,
        format!(
            "{}\n{}\n{}\n",
            json!({"type":"custom-title","sessionId":ID,"customTitle":"original"}),
            json!({"type":"user","sessionId":ID,"cwd":cwd,"message":{"content":"private prompt"}}),
            json!({"type":"custom-title","sessionId":ID,"customTitle":"检查-wakeit-1792022400000"})
        ),
    )
    .unwrap();
    let checked = inspect("claude-code", ID, &cwd, &cwd, &storage);
    assert!(checked.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&checked.stdout).unwrap()["title"],
        "检查-wakeit-1792022400000"
    );
    let listed = list("claude-code", &cwd, &storage, &[]);
    assert!(listed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap()["sessions"][0]["title"],
        "检查-wakeit-1792022400000"
    );
}

#[test]
fn builtin_names_only_new_sessions_and_codex_has_a_metadata_rename_command() {
    for protocol in ["codex", "claude-code", "pi", "opencode"] {
        let agent: wakeit::config::AgentConfig = support::official_agent(protocol);
        let session = agent.session.unwrap();
        fn literals(spec: &wakeit::config::CommandSpec) -> Vec<&str> {
            spec.args
                .iter()
                .filter_map(|arg| {
                    if let wakeit::config::CommandArg::Literal(arg) = arg {
                        Some(arg.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        }
        assert!(!literals(&session.resume).contains(&"{{session.title}}"));
        assert!(!literals(&session.interactive).contains(&"{{session.title}}"));
        if protocol == "codex" {
            assert!(literals(session.rename.as_ref().unwrap()).contains(&"{{session.title}}"));
        } else {
            let expected = if protocol == "opencode" {
                "--title"
            } else {
                "--name"
            };
            let args = literals(&agent.submit);
            let position = args.iter().position(|arg| *arg == expected).unwrap();
            assert_eq!(args[position + 1], "{{session.title}}");
        }
    }
}
