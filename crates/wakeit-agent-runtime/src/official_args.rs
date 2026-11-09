//! Native argv used by the official SDK Agent implementations.
use anyhow::{Context, Result, bail};
use wakeit_plugin_sdk::adapter::AgentRequest;
pub fn native_args(request: &AgentRequest, resume: bool) -> Result<Vec<String>> {
    let mut args = Vec::new();
    match (request.agent.as_str(), resume) {
        ("agy", false) => {
            args.push("agy".to_owned());
            args.push("--input-format".to_owned());
            args.push("stream-json".to_owned());
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            args.push("--disable-slash-commands".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--effort".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("claude-code", false) => {
            args.push("claude".to_owned());
            args.push("-p".to_owned());
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            args.push("--verbose".to_owned());
            args.push("--name".to_owned());
            args.push(request.context.session_title.clone().unwrap_or_default());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--effort".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("claude-code", true) => {
            args.push("claude".to_owned());
            args.push("-p".to_owned());
            args.push("--resume".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            args.push("--verbose".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--effort".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("cline", false) => {
            args.push("cline".to_owned());
            args.push("--json".to_owned());
            if request
                .context
                .parameters
                .get("provider")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--provider".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("provider")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--thinking".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            args.push("--".to_owned());
            args.push(request.context.prompt.clone().context("Prompt required")?);
        }
        ("codex", false) => {
            args.push("codex".to_owned());
            args.push("exec".to_owned());
            args.push("--json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("-c".to_owned());
                args.push(
                    "model_reasoning_effort=\"{{parameters.reasoning}}\"".replace(
                        "{{parameters.reasoning}}",
                        request
                            .context
                            .parameters
                            .get("reasoning")
                            .context("Parameter required")?,
                    ),
                );
            }
            args.push("-".to_owned());
        }
        ("codex", true) => {
            args.push("codex".to_owned());
            args.push("exec".to_owned());
            args.push("resume".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
            args.push("--json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("-c".to_owned());
                args.push(
                    "model_reasoning_effort=\"{{parameters.reasoning}}\"".replace(
                        "{{parameters.reasoning}}",
                        request
                            .context
                            .parameters
                            .get("reasoning")
                            .context("Parameter required")?,
                    ),
                );
            }
            args.push("-".to_owned());
        }
        ("copilot", false) => {
            args.push("copilot".to_owned());
            args.push("--no-ask-user".to_owned());
            args.push("--output-format".to_owned());
            args.push("json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("cursor", false) => {
            args.push("cursor-agent".to_owned());
            args.push("--print".to_owned());
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            args.push("--".to_owned());
            args.push(request.context.prompt.clone().context("Prompt required")?);
        }
        ("gemini-cli", false) => {
            args.push("gemini".to_owned());
            args.push("--prompt".to_owned());
            args.push("".to_owned());
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("goose", false) => {
            args.push("goose".to_owned());
            args.push("run".to_owned());
            args.push("--instructions".to_owned());
            args.push("-".to_owned());
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            args.push("--quiet".to_owned());
            if request
                .context
                .parameters
                .get("provider")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--provider".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("provider")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("kimi-code", false) => {
            args.push("kimi".to_owned());
            args.push("acp".to_owned());
        }
        ("kimi-code", true) => {
            args.push("kimi".to_owned());
            args.push("acp".to_owned());
        }
        ("opencode", false) => {
            args.push("opencode".to_owned());
            args.push("run".to_owned());
            args.push("--format".to_owned());
            args.push("json".to_owned());
            args.push("--title".to_owned());
            args.push(request.context.session_title.clone().unwrap_or_default());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("opencode", true) => {
            args.push("opencode".to_owned());
            args.push("run".to_owned());
            args.push("--session".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
            args.push("--format".to_owned());
            args.push("json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("pi", false) => {
            args.push("pi".to_owned());
            args.push("--mode".to_owned());
            args.push("json".to_owned());
            args.push("--print".to_owned());
            args.push("--name".to_owned());
            args.push(request.context.session_title.clone().unwrap_or_default());
            if request
                .context
                .parameters
                .get("provider")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--provider".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("provider")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--thinking".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("pi", true) => {
            args.push("pi".to_owned());
            args.push("--session".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
            args.push("--mode".to_owned());
            args.push("json".to_owned());
            args.push("--print".to_owned());
            if request
                .context
                .parameters
                .get("provider")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--provider".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("provider")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
            if request
                .context
                .parameters
                .get("reasoning")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--thinking".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("reasoning")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        ("qwen-code", false) => {
            args.push("qwen".to_owned());
            args.push("--prompt".to_owned());
            args.push(request.context.prompt.clone().context("Prompt required")?);
            args.push("--output-format".to_owned());
            args.push("stream-json".to_owned());
            if request
                .context
                .parameters
                .get("model")
                .is_some_and(|value| !value.is_empty())
            {
                args.push("--model".to_owned());
                args.push(
                    request
                        .context
                        .parameters
                        .get("model")
                        .context("Parameter required")?
                        .to_owned(),
                );
            }
        }
        _ => bail!("Agent execution is not supported"),
    }
    Ok(args)
}

pub fn interactive_args(request: &AgentRequest) -> Result<Vec<String>> {
    let mut args = Vec::new();
    match request.agent.as_str() {
        "claude-code" => {
            args.push("claude".to_owned());
            args.push("--resume".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
        }
        "codex" => {
            args.push("codex".to_owned());
            args.push("resume".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
        }
        "kimi-code" => {
            args.push("kimi".to_owned());
            args.push("--session".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
        }
        "opencode" => {
            args.push("opencode".to_owned());
            args.push("--session".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
        }
        "pi" => {
            args.push("pi".to_owned());
            args.push("--session".to_owned());
            args.push(
                request
                    .context
                    .session_id
                    .as_deref()
                    .context("Session ID required")?
                    .to_owned(),
            );
        }
        _ => bail!("Interactive Session continuation is not supported"),
    }
    Ok(args)
}
