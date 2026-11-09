use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Command};

fn fixture_dir() -> tempfile::TempDir {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local");
    fs::create_dir_all(&path).unwrap();
    tempfile::tempdir_in(path).unwrap()
}

#[cfg(unix)]
#[test]
fn bundled_sdk_agent_is_detected_without_generating_agent_yaml() {
    let dir = fixture_dir();
    let cli = env!("CARGO_BIN_EXE_wakeit");
    let bundle = dir.path().join("bundle");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&fs::read(source.join("wakeit-plugin.json")).unwrap()).unwrap();
    assert!(
        manifest["wakeit"]["agents"]
            .as_object()
            .is_none_or(|agents| agents.is_empty())
    );
    assert!(manifest["wakeit"]["plugins"]["official"]["agents"]["kimi-code"].is_object());
    fs::create_dir_all(&bundle).unwrap();
    fs::copy(
        source.join("wakeit-plugin.json"),
        bundle.join("wakeit-plugin.json"),
    )
    .unwrap();
    let entry = manifest["wakeit"]["plugins"]["official"]["command"]
        .as_str()
        .unwrap()
        .strip_prefix("./")
        .unwrap();
    let helper = bundle.join(entry);
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::copy(
        std::path::Path::new(cli).with_file_name("wakeit-agent-runtime"),
        &helper,
    )
    .unwrap();
    let agent = dir.path().join("kimi.yaml");
    let initialized = Command::new(cli)
        .env("WAKEIT_BUNDLED_PLUGIN_DIR", &bundle)
        .env("WAKEIT_PLUGIN_HOME", dir.path().join("plugins"))
        .args(["agent", "init", "kimi-code", "--file"])
        .arg(&agent)
        .output()
        .unwrap();
    assert!(
        !initialized.status.success(),
        "SDK Agent unexpectedly generated YAML"
    );
    assert!(!agent.exists());

    let kimi = dir.path().join("kimi");
    fs::write(&kimi,
        "#!/bin/sh\ncase \"$1\" in --help) printf 'acp --session\\n';; --version) printf '2.3.2\\n';; *) exit 1;; esac\n",
    ).unwrap();
    fs::set_permissions(&kimi, fs::Permissions::from_mode(0o755)).unwrap();
    let runner = dir.path().join("runner.yaml");
    fs::write(
        &runner,
        json!({
            "version":1,"gateway":"http://127.0.0.1:1","id":uuid::Uuid::new_v4(),
            "name":"fixture","token_file":"unused","nats_url":"nats://127.0.0.1:1",
            "credentials_file":"unused","data_dir":dir.path(),"bundled_agents":true,"agents":[]
        })
        .to_string(),
    )
    .unwrap();
    let doctor = Command::new(cli)
        .env("WAKEIT_BUNDLED_PLUGIN_DIR", &bundle)
        .env("WAKEIT_PLUGIN_HOME", dir.path().join("plugins"))
        .args(["--json", "agent", "doctor", "--config"])
        .arg(runner)
        .env("PATH", dir.path())
        .env("HOME", dir.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(doctor.status.success(), "{doctor:?}");
    let agents: Vec<Value> = serde_json::from_slice(&doctor.stdout).unwrap();
    let detected: Vec<_> = agents
        .iter()
        .filter(|agent| agent["id"] == "kimi-code")
        .collect();
    assert_eq!(
        detected.len(),
        1,
        "Official Agent is missing or declared twice"
    );
    assert_eq!(detected[0]["available"], true, "{detected:?}");
    assert_eq!(detected[0]["version"], "2.3.2");
    assert!(!agent.exists(), "Detection must not create Agent YAML");
}

#[test]
fn agent_test_uses_configured_result_format_and_pointer() {
    let dir = fixture_dir();
    let path = dir.path().join("agent.yaml");
    for (format, pointer, output, expected) in [
        (
            "json",
            Some("/answer"),
            r#"{"answer":"ok"}"#,
            Some(json!("ok")),
        ),
        ("jsonl", None, "1\n2\n", Some(json!([1, 2]))),
        ("json", None, "not JSON", None),
        ("json", Some("/missing"), "{}", None),
    ] {
        fs::write(&path, json!({
            "version":1,"id":"fixture","name":"fixture","mode":"sync",
            "submit":{"command":"node","args":["-e","process.stdout.write(process.argv[1])",output]},
            "output":{"format":format,"result_pointer":pointer}
        }).to_string()).unwrap();
        let actual = Command::new(env!("CARGO_BIN_EXE_wakeit"))
            .args(["--json", "agent", "test", "--file"])
            .arg(&path)
            .arg("--cwd")
            .arg(dir.path())
            .output()
            .unwrap();
        if let Some(expected) = expected {
            assert!(actual.status.success(), "{actual:?}");
            let response: Value = serde_json::from_slice(&actual.stdout).unwrap();
            assert_eq!(response["state"], "succeeded");
            assert_eq!(response["result"], expected);
        } else {
            assert!(
                !actual.status.success(),
                "accepted {format} output {output:?}"
            );
            assert!(!String::from_utf8_lossy(&actual.stdout).contains("succeeded"));
        }
    }
}
