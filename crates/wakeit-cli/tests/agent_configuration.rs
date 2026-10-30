use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{fs, process::Command};

#[cfg(unix)]
#[test]
fn initialized_agent_survives_removing_the_installation_executable() {
    let dir = tempfile::tempdir().unwrap();
    let cli = env!("CARGO_BIN_EXE_wakeit");
    let installer = dir.path().join("temporary-wakeit");
    fs::copy(cli, &installer).unwrap();
    let bundle = dir.path().join("bundle");
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: Value =
        serde_json::from_slice(&fs::read(source.join("wakeit-plugin.json")).unwrap()).unwrap();
    for path in manifest["wakeit"]["agents"].as_array().unwrap() {
        let path = std::path::Path::new(path.as_str().unwrap());
        fs::create_dir_all(bundle.join(path).parent().unwrap()).unwrap();
        fs::copy(source.join(path), bundle.join(path)).unwrap();
    }
    fs::copy(
        source.join("wakeit-plugin.json"),
        bundle.join("wakeit-plugin.json"),
    )
    .unwrap();
    let helper = bundle.join("plugins/official/bin/wakeit-agent-runtime");
    fs::create_dir_all(helper.parent().unwrap()).unwrap();
    fs::copy(
        std::path::Path::new(cli).with_file_name("wakeit-agent-runtime"),
        &helper,
    )
    .unwrap();
    let agent = dir.path().join("kimi.yaml");
    let initialized = Command::new(&installer)
        .env("WAKEIT_BUNDLED_PLUGIN_DIR", &bundle)
        .env("WAKEIT_PLUGIN_HOME", dir.path().join("plugins"))
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
    let agents: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(agents[0]["available"], true);
    assert_eq!(agents[0]["version"], "2.3.2");
}

#[test]
fn agent_test_uses_configured_result_format_and_pointer() {
    let dir = tempfile::tempdir().unwrap();
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
