//! All official Agent integrations register through the same public SDK.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use wakeit_plugin_sdk::{
    Plugin,
    adapter::{
        AgentDescriptor, AgentOperation, AgentReply, AgentRequest, Detection, ExecutionOutcome,
        InteractivePlan,
    },
    agent::{Agent, AgentContext},
};

pub fn package_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("WAKEIT_PLUGIN_ROOT") {
        let root = PathBuf::from(root).canonicalize()?;
        ensure!(
            root.join("wakeit-plugin.json").is_file(),
            "plugin manifest unavailable"
        );
        return Ok(root);
    }
    std::env::current_exe()?
        .ancestors()
        .skip(1)
        .find(|path| path.join("wakeit-plugin.json").is_file())
        .map(Path::to_path_buf)
        .context("official plugin root is unavailable")
}

/// Implementation capabilities, independent of the manifest being checked.
pub fn implemented_operations(id: &str) -> Result<BTreeSet<AgentOperation>> {
    use AgentOperation::*;
    ensure!(
        [
            "agy",
            "claude-code",
            "cline",
            "codex",
            "copilot",
            "cursor",
            "gemini-cli",
            "goose",
            "kimi-code",
            "opencode",
            "pi",
            "qwen-code"
        ]
        .contains(&id),
        "Official Agent implementation is not available: {id}"
    );
    let mut operations: BTreeSet<_> = [Detect, Execute, Reconcile].into();
    if ["agy", "codex", "kimi-code", "opencode", "pi"].contains(&id) {
        operations.insert(Models);
    }
    if ["claude-code", "codex", "kimi-code", "opencode", "pi"].contains(&id) {
        operations.extend([
            SessionList,
            SessionInspect,
            SessionResume,
            SessionInteractive,
        ]);
    }
    if id == "codex" {
        operations.insert(SessionRename);
    }
    Ok(operations)
}

pub async fn serve() -> Result<()> {
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(package_root()?.join("wakeit-plugin.json"))?)?;
    let descriptors: BTreeMap<String, AgentDescriptor> =
        serde_json::from_value(manifest["wakeit"]["plugins"]["official"]["agents"].clone())?;
    let mut plugin = Plugin::new();
    for (id, descriptor) in descriptors {
        let operations = implemented_operations(&id)?;
        ensure!(
            descriptor.operations == operations,
            "Official Agent declaration differs from its implementation: {id}"
        );
        let mut implementation = Agent::new(descriptor);
        for operation in &operations {
            implementation = implementation.on(*operation, |request, context| async move {
                handle(request, context).await
            });
        }
        plugin = plugin.agent(id, implementation);
    }
    plugin.serve().await
}

async fn native_version(id: &str) -> Result<String> {
    let program = match id {
        "codex" => "codex",
        "claude-code" => "claude",
        "pi" => "pi",
        "opencode" => "opencode",
        _ => return crate::discovery::detect(id).await,
    };
    let mut executable = program.to_owned();
    let mut args = vec!["--version".to_owned()];
    wakeit_plugin_sdk::native::normalize_native_command(&mut executable, &mut args)?;
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut child = Command::new(executable)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .context("native version output")?
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(
            bytes.len() <= 64 * 1024,
            "native version output is too large"
        );
        ensure!(
            child.wait().await?.success(),
            "native version command failed"
        );
        Ok(String::from_utf8(bytes)?.trim().chars().take(512).collect())
    })
    .await
    .context("native detection timed out")?
}

async fn handle(request: AgentRequest, context: AgentContext) -> Result<AgentReply> {
    ensure!(
        std::env::current_dir()?.canonicalize()?
            == Path::new(&request.context.cwd).canonicalize()?,
        "SDK request working directory differs from its process"
    );
    match request.operation {
        AgentOperation::Detect => Ok(AgentReply::Detection(
            match native_version(&request.agent).await {
                Ok(version) => Detection {
                    available: true,
                    version: Some(version),
                    reason: None,
                },
                Err(error) => Detection {
                    available: false,
                    version: None,
                    reason: Some(error.to_string()),
                },
            },
        )),
        AgentOperation::Models => Ok(AgentReply::Models(
            crate::discovery::discover(&request.agent).await?,
        )),
        AgentOperation::SessionList => {
            let result = crate::sessions::list(
                &request.agent,
                Path::new(&request.context.cwd),
                request.input["cursor"].as_str(),
                request.input["limit"].as_u64().unwrap_or(10000) as usize,
                request.input["query"].as_str(),
            )
            .await?;
            Ok(AgentReply::Sessions(serde_json::from_value(result)?))
        }
        AgentOperation::SessionInspect => {
            let result = crate::sessions::inspect(
                &request.agent,
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?,
                Path::new(&request.context.cwd),
            )
            .await?;
            Ok(AgentReply::Session(serde_json::from_value(result)?))
        }
        AgentOperation::SessionRename => {
            crate::sessions::rename(
                &request.agent,
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?,
                request.input["title"]
                    .as_str()
                    .context("Session title required")?,
                Path::new(&request.context.cwd),
            )
            .await?;
            Ok(AgentReply::Renamed)
        }
        AgentOperation::SessionInteractive => {
            let plan = InteractivePlan {
                cwd: request.context.cwd.clone(),
                argv: crate::official_args::interactive_args(&request)?,
                env: BTreeMap::new(),
            };
            plan.validate(&request.context.cwd)?;
            Ok(AgentReply::Interactive(plan))
        }
        AgentOperation::Execute | AgentOperation::SessionResume => {
            let native = crate::official_args::native_args(
                &request,
                request.operation == AgentOperation::SessionResume,
            )?;
            let (result, started, recorded_result) = crate::official_io::scope(context, async {
                if request.agent == "kimi-code" {
                    crate::kimi_acp::run(
                        &native,
                        request.context.parameters.get("model").map(String::as_str),
                        request.context.session_id.as_deref(),
                    )
                    .await
                } else {
                    crate::stream::stream_checked(
                        &request.agent,
                        &native,
                        request.context.session_id.as_deref(),
                    )
                    .await
                }
            })
            .await;
            match result {
                Ok(code) => Ok(AgentReply::Execution(ExecutionOutcome::Completed {
                    exit_code: Some(code),
                    result: if code == 0 {
                        recorded_result.unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    },
                    confirmed: true,
                })),
                Err(error) if !started => Ok(AgentReply::Execution(ExecutionOutcome::NotStarted {
                    message: error.to_string(),
                })),
                Err(error) => Err(error),
            }
        }
        AgentOperation::Cancel => bail!(
            "Cancel the active SDK execution channel; this Agent has no independent native job cancellation operation"
        ),
        AgentOperation::Reconcile => Ok(AgentReply::Execution(ExecutionOutcome::Uncertain {
            reason: "Native execution ownership cannot be recovered by starting another process"
                .into(),
        })),
        AgentOperation::Submit | AgentOperation::Poll => {
            bail!("Official native Agents execute synchronously")
        }
    }
}
