//! Compose the real TypeScript SDK with the Runner bridge, not two matching mocks.
use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use uuid::Uuid;
use wakeit::{
    config::{Execution, TaskConfig},
    process::LogSink,
    protocol::Assignment,
    runner::{
        agent_backend::{Agent, Backend, SdkBackend},
        journal::Journal,
    },
};
use wakeit_plugin_protocol::{
    adapter::{AgentDescriptor, AgentOperation},
    agent::AgentMode,
};

// Isolate stdio composition from package discovery and Gateway registration. The
// Runner still resolves the invocation against a real enabled instance in SQLite.
async fn bind_fixture_instance(
    journal: &Journal,
    data: &std::path::Path,
    agent: &mut Agent,
    declaration: &Value,
) {
    let Backend::SdkBackend(backend) = &agent.backend else {
        unreachable!()
    };
    sqlx::query("CREATE TABLE extension_instances(id TEXT PRIMARY KEY, source TEXT, extension TEXT, root TEXT, version TEXT, revision INTEGER, enabled INTEGER, removed INTEGER, grants TEXT, declaration TEXT, config TEXT, secrets TEXT)")
        .execute(&journal.pool).await.unwrap();
    let instance = Uuid::new_v4();
    sqlx::query("INSERT INTO extension_instances VALUES(?,?,?,?,?,1,1,0,'[]',?,'{}','{}')")
        .bind(instance.to_string())
        .bind(&backend.source)
        .bind(&backend.plugin)
        .bind(backend.root.to_str().unwrap())
        .bind(&backend.version)
        .bind(declaration.to_string())
        .execute(&journal.pool)
        .await
        .unwrap();
    agent.bind_runner(data, Some(instance)).unwrap();
}

#[tokio::test]
async fn real_sdk_output_reaches_runner_result_and_failed_output_is_not_published() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let local = root.join(".local/sdk-agent-integration");
    std::fs::create_dir_all(&local).unwrap();
    let sdk = reqwest::Url::from_file_path(
        root.join("sdk/typescript/src/index.ts")
            .canonicalize()
            .unwrap(),
    )
    .unwrap();
    let node = which::which("node").expect("Node.js 22.18+ is required for SDK plugin integration");
    for exit_code in [0, 1] {
        let package = tempfile::tempdir_in(&local).unwrap();
        let path = package.path().canonicalize().unwrap();
        let descriptor = AgentDescriptor {
            name: "Synthetic SDK Agent".into(),
            logo: None,
            parameters: BTreeMap::new(),
            mode: AgentMode::Sync,
            operations: [AgentOperation::Detect, AgentOperation::Execute].into(),
        };
        let declaration = json!({
            "name":"Synthetic plugin", "command":"node", "args":["{{plugin.root}}/adapter.ts"],
            "agents":{"synthetic":descriptor}
        });
        std::fs::write(path.join("wakeit-plugin.json"), json!({"manifest_version":1,"name":"sdk-result-composition","version":"0.0.0","wakeit":{"protocol":1,"plugins":{"synthetic":declaration}}}).to_string()).unwrap();
        let script = format!(
            r#"
import {{ Agent, Plugin }} from {sdk};
const descriptor = {descriptor};
const agent = new Agent(descriptor)
  .on('detect', () => ({{kind:'detection',value:{{available:true,version:null,reason:null}}}}))
  .on('execute', async (request, ctx) => {{
    if (request.context.prompt !== 'synthetic prompt') throw new Error('Execution input changed');
    await ctx.result('actual SDK result');
    return {{kind:'execution',value:{{outcome:'completed',exit_code:{exit_code},result:null,confirmed:true}}}};
  }});
await new Plugin().agent('synthetic', agent).serve();
"#,
            sdk = serde_json::to_string(sdk.as_str()).unwrap(),
            descriptor = serde_json::to_string(&descriptor).unwrap()
        );
        std::fs::write(path.join("adapter.ts"), script).unwrap();
        let mut agent = Agent::sdk(SdkBackend {
            id: "synthetic".into(),
            plugin: "synthetic".into(),
            descriptor,
            source: format!("local:{}", path.display()),
            version: "0.0.0".into(),
            revision: None,
            root: path.clone(),
            command: node.clone(),
            args: vec![wakeit_core::native_command::external_path(
                &path.join("adapter.ts"),
            )],
            instance_id: None,
            legacy_stateless: false,
            runtime_directory: None,
            configuration: None,
        })
        .unwrap();
        let task = TaskConfig {
            version: 1,
            id: "synthetic-task".into(),
            name: "Synthetic task".into(),
            runner: "synthetic-device".into(),
            agent: "synthetic".into(),
            agent_instance: None,
            cwd: path.to_string_lossy().into(),
            prompt: "synthetic prompt".into(),
            parameters: BTreeMap::new(),
            session: None,
            schedule: None,
            enabled: false,
            execution: Execution::default(),
            dag: None,
        };
        let runner = Uuid::new_v4();
        let run = Uuid::new_v4();
        let data = path.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let journal = Journal::open(&data, runner).await.unwrap();
        bind_fixture_instance(&journal, &data, &mut agent, &declaration).await;
        journal
            .insert(
                &Assignment {
                    version: 1,
                    run_id: run,
                    runner_id: runner,
                    task: task.clone(),
                    deadline: Utc::now() + chrono::Duration::seconds(20),
                },
                &agent,
            )
            .await
            .unwrap();
        let output = agent
            .execute(
                &task,
                run,
                Duration::from_secs(15),
                tokio_util::sync::CancellationToken::new(),
                Some(LogSink {
                    journal: journal.clone(),
                    id: run,
                    stdout: true,
                }),
                None,
            )
            .await
            .unwrap();
        assert_eq!(output.code, Some(exit_code));
        assert!(output.confirmed);
        assert_eq!(
            output.result_value(&agent.output).unwrap(),
            if exit_code == 0 {
                json!("actual SDK result")
            } else {
                Value::Null
            }
        );
        journal.pool.close().await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn installed_official_agent_uses_the_sdk_runner_path_with_synthetic_native_io() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let local = source.join(".local/sdk-agent-integration");
    fs::create_dir_all(&local).unwrap();
    let directory = tempfile::tempdir_in(&local).unwrap();
    let root = directory.path().canonicalize().unwrap();
    let package = root.join("package");
    let bin = root.join("native-bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&package).unwrap();
    fs::copy(
        source.join("wakeit-plugin.json"),
        package.join("wakeit-plugin.json"),
    )
    .unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(package.join("wakeit-plugin.json")).unwrap()).unwrap();
    let entry = manifest["wakeit"]["plugins"]["official"]["command"]
        .as_str()
        .unwrap()
        .trim_start_matches("./");
    let program = package.join(entry);
    fs::create_dir_all(program.parent().unwrap()).unwrap();
    fs::copy(
        PathBuf::from(env!("CARGO_BIN_EXE_wakeit")).with_file_name("wakeit-agent-runtime"),
        &program,
    )
    .unwrap();
    let native = bin.join("copilot");
    fs::write(&native, r#"#!/usr/bin/env node
let input='';process.stdin.setEncoding('utf8');
process.stdin.on('data',value=>input+=value);
process.stdin.on('end',()=>{
 if(input!=='Synthetic official input')process.exit(9);
 console.log(JSON.stringify({type:'assistant.message',data:{messageId:'synthetic',content:'Official SDK report'}}));
 console.log(JSON.stringify({type:'result',exitCode:0}));
});
"#).unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
        std::iter::once(bin.clone())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let plugin_home = root.join("registry");
    let cli = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_wakeit"))
            .args(args)
            .current_dir(&root)
            .env("HOME", root.join("home"))
            .env("APPDATA", root.join("appdata"))
            .env("PATH", &path)
            .env("WAKEIT_PLUGIN_HOME", &plugin_home)
            .env("WAKEIT_BUNDLED_PLUGIN_DIR", &package)
            .output()
            .unwrap()
    };
    let mut install_args = vec!["--json", "plugin", "install", package.to_str().unwrap()];
    for id in manifest["wakeit"]["plugins"]["official"]["agents"]
        .as_object()
        .unwrap()
        .keys()
    {
        install_args.extend(["--replace-agent", id.as_str()]);
    }
    let installed = cli(&install_args);
    assert!(
        installed.status.success(),
        "{} {}",
        String::from_utf8_lossy(&installed.stderr),
        String::from_utf8_lossy(&installed.stdout)
    );
    let registered = cli(&["--json", "plugin", "list"]);
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    let listing: Value = serde_json::from_slice(&registered.stdout).unwrap();
    assert!(listing["packages"].as_array().unwrap().iter().any(|p| {
        p["agents"]
            .as_array()
            .is_some_and(|agents| agents.iter().any(|id| id == "copilot"))
    }));
    let descriptor = serde_json::from_value(
        manifest["wakeit"]["plugins"]["official"]["agents"]["copilot"].clone(),
    )
    .unwrap();
    let mut agent = Agent::sdk(SdkBackend {
        id: "copilot".into(),
        plugin: "official".into(),
        descriptor,
        source: format!("local:{}", package.display()),
        version: manifest["version"].as_str().unwrap().into(),
        revision: None,
        root: package.clone(),
        command: program,
        args: vec!["sdk".into()],
        instance_id: None,
        legacy_stateless: false,
        runtime_directory: None,
        configuration: None,
    })
    .unwrap();
    let data = root.join("data");
    let journal = Journal::open(&data, Uuid::new_v4()).await.unwrap();
    bind_fixture_instance(
        &journal,
        &data,
        &mut agent,
        &manifest["wakeit"]["plugins"]["official"],
    )
    .await;
    journal.pool.close().await;
    let snapshot = root.join("backend.json");
    fs::write(&snapshot, serde_json::to_vec(&agent).unwrap()).unwrap();
    let output = cli(&[
        "--json",
        "agent",
        "test",
        "--file",
        snapshot.to_str().unwrap(),
        "--cwd",
        root.to_str().unwrap(),
        "--prompt",
        "Synthetic official input",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["state"], "succeeded");
    assert_eq!(result["result"], "Official SDK report");
    assert!(!package.join("plugins/official/agents").exists());
}
