#![cfg(unix)]

use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn initialized_agent_survives_removing_the_installation_executable() {
    let dir = tempfile::tempdir().unwrap();
    let cli = env!("CARGO_BIN_EXE_wakeit");
    let installer = dir.path().join("temporary-wakeit");
    fs::copy(cli, &installer).unwrap();
    let agent = dir.path().join("kimi.yaml");
    let initialized = Command::new(&installer)
        .args(["agent", "init", "kimi-code", "--file"])
        .arg(&agent)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    fs::remove_file(installer).unwrap();

    let kimi = dir.path().join("kimi");
    fs::write(
        &kimi,
        "#!/bin/sh\ncase \"$1\" in --help) printf 'web --session\\n';; --version) printf '2.3.2\\n';; *) exit 1;; esac\n",
    )
    .unwrap();
    fs::set_permissions(&kimi, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = dir.path().join("runner.yaml");
    fs::write(
        &runner,
        json!({
            "version":1,"gateway":"http://127.0.0.1:1","id":uuid::Uuid::new_v4(),
            "name":"fixture","token_file":"unused","nats_url":"nats://127.0.0.1:1",
            "credentials_file":"unused","data_dir":dir.path(),"agents":[agent]
        })
        .to_string(),
    )
    .unwrap();
    let doctor = Command::new(cli)
        .args(["--json", "agent", "doctor", "--config"])
        .arg(runner)
        .env("PATH", dir.path())
        .env("HOME", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(doctor.status.success(), "{doctor:?}");
    let agents: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(agents[0]["available"], true);
    assert_eq!(agents[0]["version"], "2.3.2");
}
