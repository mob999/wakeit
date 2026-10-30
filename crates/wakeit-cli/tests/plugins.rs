use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Device {
    _temp: tempfile::TempDir,
    root: PathBuf,
    bundle: PathBuf,
}
impl Device {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_owned();
        let bundle = root.join("bundled");
        package(&bundle, "official-fixture", "bundled", "bundled");
        Self {
            _temp: temp,
            root,
            bundle,
        }
    }
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_wakeit"))
            .args(args)
            .current_dir(&self.root)
            .env("HOME", self.root.join("home"))
            .env("APPDATA", self.root.join("appdata"))
            .env("WAKEIT_PLUGIN_HOME", self.root.join("plugins"))
            .env("WAKEIT_BUNDLED_PLUGIN_DIR", &self.bundle)
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Value {
        let output = self.cli(args);
        assert!(
            output.status.success(),
            "{args:?}: {} {}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn execute(&self, config: &Path) -> Value {
        self.ok(&[
            "--json",
            "agent",
            "test",
            "--file",
            config.to_str().unwrap(),
            "--cwd",
            self.root.to_str().unwrap(),
            "--prompt",
            "hello plugin",
        ])
    }
}
fn package(root: &Path, name: &str, id: &str, answer: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("wakeit-plugin.json"),json!({"manifest_version":1,"name":name,"version":"1.0.0","wakeit":{"protocol":1,"agents":["./agent.yaml"],"runtimes":["node"]}}).to_string()).unwrap();
    fs::write(root.join("agent.yaml"),json!({"version":1,"id":id,"name":id,"mode":"sync","submit":{"command":"node","args":["{{plugin.root}}/adapter with spaces.cjs"],"stdin":"{{task.prompt}}"},"output":{"format":"events"}}).to_string()).unwrap();
    fs::write(root.join("adapter with spaces.cjs"),format!("const fs=require('fs');console.log(JSON.stringify({{version:1,type:'result',data:{{answer:{},cwd:process.cwd(),prompt:fs.readFileSync(0,'utf8')}}}}))",serde_json::to_string(answer).unwrap())).unwrap();
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn commit(root: &Path) {
    git(root, &["add", "."]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "-m",
            "fixture",
        ],
    );
    git(root, &["update-server-info"]);
}
#[test]
fn local_packages_execute_with_task_cwd_and_explicit_agent_selection() {
    let device = Device::new();
    let first = device.root.join("first package");
    package(&first, "same-display-name", "fixture", "first");
    device.ok(&["--json", "plugin", "install", first.to_str().unwrap()]);
    let config = device.root.join("export.yaml");
    let export = device.cli(&[
        "agent",
        "init",
        "fixture",
        "--file",
        config.to_str().unwrap(),
    ]);
    assert!(export.status.success());
    let result = device.execute(&config);
    assert_eq!(result["result"]["answer"], "first");
    assert_eq!(
        Path::new(result["result"]["cwd"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        device.root.canonicalize().unwrap()
    );
    assert_eq!(result["result"]["prompt"], "hello plugin");
    let second = device.root.join("second");
    package(&second, "same-display-name", "fixture", "second");
    let failure = device.cli(&["plugin", "install", second.to_str().unwrap()]);
    assert!(!failure.status.success());
    let message = String::from_utf8_lossy(&failure.stderr);
    assert!(message.contains("both") && message.contains("--replace-agent"));
    device.ok(&[
        "--json",
        "plugin",
        "install",
        second.to_str().unwrap(),
        "--replace-agent",
        "fixture",
    ]);
    let config2 = device.root.join("second.yaml");
    assert!(
        device
            .cli(&[
                "agent",
                "init",
                "fixture",
                "--file",
                config2.to_str().unwrap()
            ])
            .status
            .success()
    );
    assert_eq!(device.execute(&config2)["result"]["answer"], "second");
    let list = device.ok(&["--json", "plugin", "list"]);
    assert_eq!(list["packages"].as_array().unwrap().len(), 3);
    fs::remove_dir_all(&second).unwrap();
    device.ok(&["--json", "plugin", "remove", second.to_str().unwrap()]);
    assert_eq!(device.execute(&config)["result"]["answer"], "first");
    let old = device.cli(&["adapter", "detect", "kimi-code"]);
    assert!(
        !old.status.success(),
        "the removed command must not remain an alias"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn git_tags_are_pinned_and_updates_keep_exported_versions_executable() {
    use axum::{
        Router,
        extract::{Path as HttpPath, State},
        http::StatusCode,
        routing::get,
    };
    let device = Device::new();
    let repo = device.root.join("repo");
    package(&repo, "fixture", "remote", "one");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    let script = fs::read(repo.join("adapter with spaces.cjs")).unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_size(script.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append_data(&mut header, "adapter with spaces.cjs", script.as_slice())
        .unwrap();
    let artifact = archive.into_inner().unwrap().finish().unwrap();
    use sha2::{Digest, Sha256};
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(repo.join("wakeit-plugin.json")).unwrap()).unwrap();
    manifest["wakeit"]["artifacts"] = json!({env!("WAKEIT_BUILD_TARGET"):{"url":format!("http://{address}/artifact.tar.gz"),"sha256":hex::encode(Sha256::digest(&artifact))}});
    fs::write(repo.join("wakeit-plugin.json"), manifest.to_string()).unwrap();
    fs::remove_file(repo.join("adapter with spaces.cjs")).unwrap();
    git(&repo, &["init", "--initial-branch=main"]);
    commit(&repo);
    git(&repo, &["tag", "v1.0.0"]);
    git(&repo, &["update-server-info"]);
    let router = Router::new()
        .route(
            "/artifact.tar.gz",
            get(move || {
                let bytes = artifact.clone();
                async move { bytes }
            }),
        )
        .route(
            "/example/agent/{*path}",
            get(
                |State(root): State<PathBuf>, HttpPath(path): HttpPath<String>| async move {
                    match fs::read(root.join(path)) {
                        Ok(bytes) => Ok(bytes),
                        Err(_) => Err(StatusCode::NOT_FOUND),
                    }
                },
            ),
        )
        .with_state(repo.join(".git"));
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let source = format!("http://{address}/example/agent");
    let pinned = format!("{source}@v1.0.0");
    let installed = device.ok(&["--json", "plugin", "install", &pinned]);
    let old_revision = installed["revision"].clone();
    let config = device.root.join("old.yaml");
    assert!(
        device
            .cli(&[
                "agent",
                "init",
                "remote",
                "--file",
                config.to_str().unwrap()
            ])
            .status
            .success()
    );
    package(&repo, "fixture", "remote", "two");
    commit(&repo);
    let update = device.ok(&["--json", "plugin", "update", &source]);
    assert_eq!(update["packages"][0]["status"], "pinned");
    let changed = device.ok(&["--json", "plugin", "install", &source]);
    assert_ne!(old_revision, changed["revision"]);
    assert_eq!(device.execute(&config)["result"]["answer"], "one");
    let config2 = device.root.join("new.yaml");
    assert!(
        device
            .cli(&[
                "agent",
                "init",
                "remote",
                "--file",
                config2.to_str().unwrap()
            ])
            .status
            .success()
    );
    assert_eq!(device.execute(&config2)["result"]["answer"], "two");
    package(&repo, "fixture", "remote", "three");
    commit(&repo);
    let update = device.ok(&["--json", "plugin", "update", &source]);
    assert_ne!(update["packages"][0]["revision"], changed["revision"]);
    let mut invalid: Value =
        serde_json::from_slice(&fs::read(repo.join("wakeit-plugin.json")).unwrap()).unwrap();
    invalid["wakeit"]["artifacts"] = json!({env!("WAKEIT_BUILD_TARGET"):{"url":format!("http://{address}/artifact.tar.gz"),"sha256":"0".repeat(64)}});
    fs::write(repo.join("wakeit-plugin.json"), invalid.to_string()).unwrap();
    commit(&repo);
    let rejected = device.cli(&["--json", "plugin", "update", &source]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("checksum mismatch"));
    let list = device.ok(&["--json", "plugin", "list"]);
    assert_eq!(
        list["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "fixture")
            .unwrap()["revision"],
        update["packages"][0]["revision"]
    );
    device.ok(&["--json", "plugin", "remove", &source]);
    assert_eq!(device.execute(&config2)["result"]["answer"], "two");
    server.abort();
}
