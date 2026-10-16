//! Builtin discovery adapters. The Runner invokes these through the same command contract as third parties.
use crate::protocol::{ModelCatalog, ModelEntry};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

/// Reject unrelated desktop launchers or incompatible native generations.
/// Help/version terminate before an Agent engine or model session is started.
pub async fn detect(id: &str) -> Result<String> {
    let (command, help_args, required): (&str, &[&str], &[&str]) = match id {
        "kimi-code" => (
            "kimi",
            &["--help"],
            &["--prompt", "--output-format", "--session"],
        ),
        "agy" => (
            "agy",
            &["--help"],
            &[
                "--input-format",
                "--output-format",
                "--disable-slash-commands",
            ],
        ),
        "gemini-cli" => ("gemini", &["--help"], &["--prompt", "--output-format"]),
        "qwen-code" => ("qwen", &["--help"], &["--prompt", "--output-format"]),
        "cursor" => ("cursor-agent", &["--help"], &["--print", "--output-format"]),
        "copilot" => (
            "copilot",
            &["--help"],
            &["--silent", "--no-ask-user", "--output-format", "--model"],
        ),
        "cline" => (
            "cline",
            &["--help"],
            &["--json", "--auto-approve", "--model"],
        ),
        "goose" => (
            "goose",
            &["run", "--help"],
            &["--instructions", "--output-format"],
        ),
        _ => bail!("unsupported native capability probe"),
    };
    tokio::time::timeout(Duration::from_secs(8), async {
        let help = probe(command, help_args).await?;
        ensure!(
            required.iter().all(|flag| help.contains(flag)),
            "native CLI lacks required headless options"
        );
        let version = probe(command, &["--version"]).await?;
        if id == "kimi-code" {
            let major = version.split_whitespace().find_map(|part| {
                part.trim_start_matches('v')
                    .split('.')
                    .next()?
                    .parse::<u32>()
                    .ok()
            });
            ensure!(
                major == Some(2),
                "Kimi Code adapter requires the native kimi-code 2.x CLI"
            );
        }
        Ok(version.trim().chars().take(512).collect())
    })
    .await
    .context("native CLI detection timed out")?
}

async fn probe(executable: &str, args: &[&str]) -> Result<String> {
    let mut spec = crate::config::CommandSpec {
        command: executable.into(),
        args: args.iter().map(|s| (*s).into()).collect(),
        ..Default::default()
    };
    crate::config::normalize_native_command(&mut spec)?;
    let mut command = Command::new(spec.command);
    for arg in spec.args {
        if let crate::config::CommandArg::Literal(value) = arg {
            command.arg(value);
        }
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("start native CLI detection")?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .context("native help stdout")?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(
        bytes.len() <= 128 * 1024,
        "native CLI detection output exceeded limit"
    );
    ensure!(child.wait().await?.success(), "native CLI detection failed");
    String::from_utf8(bytes).context("invalid native CLI detection output")
}

fn entry(id: &str, name: &str, provider: Option<&str>, model: &str) -> ModelEntry {
    let mut parameters = BTreeMap::from([("model".into(), model.into())]);
    if let Some(provider) = provider {
        parameters.insert("provider".into(), provider.into());
    }
    ModelEntry {
        id: id.into(),
        name: name.into(),
        parameters,
        parameter_options: Default::default(),
    }
}
fn spawn(executable: &str, args: &[&str]) -> Result<Child> {
    let mut spec = crate::config::AgentConfig {
        version: 1,
        id: "discovery".into(),
        name: "discovery".into(),
        logo: None,
        mode: crate::config::AgentMode::Sync,
        submit: crate::config::CommandSpec {
            command: executable.into(),
            args: args.iter().map(|s| (*s).into()).collect(),
            ..Default::default()
        },
        status: None,
        cancel: None,
        detect: None,
        session: None,
        models: None,
        parameters: Default::default(),
        output: Default::default(),
        poll_interval: "5s".into(),
    };
    crate::config::normalize_commands(&mut spec)?;
    let mut command = Command::new(&spec.submit.command);
    for arg in &spec.submit.args {
        if let crate::config::CommandArg::Literal(v) = arg {
            command.arg(v);
        }
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start {executable} model discovery"))
}
async fn send(input: &mut tokio::process::ChildStdin, v: Value) -> Result<()> {
    input.write_all(format!("{v}\n").as_bytes()).await?;
    input.flush().await?;
    Ok(())
}
async fn response(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: &Value,
) -> Result<Value> {
    loop {
        let line = lines
            .next_line()
            .await?
            .context("model discovery exited without a response")?;
        ensure!(
            line.len() <= 2 * 1024 * 1024,
            "model response exceeded limit"
        );
        let v: Value = serde_json::from_str(&line).context("invalid model protocol response")?;
        if v["id"] == *id {
            if v.get("error").is_some() {
                bail!("model interface returned an error");
            }
            ensure!(v["success"] != false, "model interface returned failure");
            return Ok(v);
        }
    }
}
pub async fn discover(id: &str) -> Result<ModelCatalog> {
    tokio::time::timeout(Duration::from_secs(25), discover_inner(id))
        .await
        .context("model discovery timed out")?
}
async fn discover_inner(id: &str) -> Result<ModelCatalog> {
    match id {
        "agy" => crate::agy_adapter::discover_models().await,
        "codex" => {
            let mut child = spawn("codex", &["app-server"])?;
            let mut input = child.stdin.take().context("stdin")?;
            let mut lines = BufReader::new(child.stdout.take().context("stdout")?).lines();
            send(&mut input,json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"wakeit","version":"0.1.0"},"capabilities":{"experimentalApi":false}}})).await?;
            response(&mut lines, &json!(1)).await?;
            send(&mut input, json!({"method":"initialized"})).await?;
            let mut models = Vec::new();
            let mut cursor = Value::Null;
            let mut count = 2;
            loop {
                send(&mut input,json!({"id":count,"method":"model/list","params":{"limit":100,"cursor":cursor,"includeHidden":false}})).await?;
                let reply = response(&mut lines, &json!(count)).await?;
                let result = &reply["result"];
                for item in result["data"]
                    .as_array()
                    .context("model/list data missing")?
                {
                    let model = item["model"].as_str().context("model id missing")?;
                    let mut e = entry(
                        item["id"].as_str().unwrap_or(model),
                        item["displayName"].as_str().unwrap_or(model),
                        None,
                        model,
                    );
                    if let Some(options) = item["supportedReasoningEfforts"].as_array() {
                        e.parameter_options.insert(
                            "reasoning".into(),
                            options
                                .iter()
                                .filter_map(|v| v["reasoningEffort"].as_str().map(str::to_owned))
                                .collect(),
                        );
                    }
                    models.push(e);
                }
                ensure!(
                    models.len() <= 10000 && count <= 100,
                    "model catalog exceeded limit"
                );
                let next = result["nextCursor"].clone();
                if next.is_null() {
                    break;
                }
                ensure!(next != cursor, "model/list repeated its cursor");
                cursor = next;
                count += 1;
            }
            child.kill().await.ok();
            let _ = child.wait().await;
            Ok(ModelCatalog { version: 1, models })
        }
        "pi" => {
            let mut child = spawn("pi", &["--mode", "rpc", "--no-session"])?;
            let mut input = child.stdin.take().context("stdin")?;
            let mut lines = BufReader::new(child.stdout.take().context("stdout")?).lines();
            send(
                &mut input,
                json!({"id":"models","type":"get_available_models"}),
            )
            .await?;
            let reply = response(&mut lines, &json!("models")).await?;
            let models = reply["data"]["models"]
                .as_array()
                .context("Pi model response missing")?
                .iter()
                .map(|item| {
                    let model = item["id"].as_str().context("Pi model ID missing")?;
                    let provider = item["provider"].as_str().context("Pi provider missing")?;
                    Ok(entry(
                        &format!("{provider}/{model}"),
                        item["name"].as_str().unwrap_or(model),
                        Some(provider),
                        model,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            child.kill().await.ok();
            let _ = child.wait().await;
            Ok(ModelCatalog { version: 1, models })
        }
        "opencode" => {
            let child = spawn("opencode", &["models"])?;
            let output = child.wait_with_output().await?;
            ensure!(output.status.success(), "OpenCode models failed");
            let text = String::from_utf8(output.stdout)?;
            let mut models = Vec::new();
            for line in text.lines().map(str::trim).filter(|v| !v.is_empty()) {
                // CLI model IDs are provider/model tokens, not human table rows.
                if !line.contains(char::is_whitespace)
                    && line
                        .split_once('/')
                        .is_some_and(|(p, m)| !p.is_empty() && !m.is_empty())
                {
                    models.push(entry(line, line, None, line));
                }
            }
            ensure!(
                text.trim().is_empty() || !models.is_empty(),
                "OpenCode model output format is unsupported"
            );
            Ok(ModelCatalog { version: 1, models })
        }
        _ => bail!("no builtin models interface for {id}"),
    }
}
