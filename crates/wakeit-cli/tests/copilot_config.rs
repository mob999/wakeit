//! Official SDK Copilot native argv, piped input and permissions.
use chrono::Utc;
use uuid::Uuid;
use wakeit_agent_runtime::official_args::native_args;
use wakeit_plugin_protocol::adapter::{
    AgentContext, AgentDescriptor, AgentOperation, AgentRequest,
};

fn descriptor() -> AgentDescriptor {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../wakeit-plugin.json")).unwrap();
    serde_json::from_value(manifest["wakeit"]["plugins"]["official"]["agents"]["copilot"].clone())
        .unwrap()
}

fn request(cwd: &str, prompt: &str) -> AgentRequest {
    AgentRequest {
        id: Uuid::new_v4(),
        agent: "copilot".into(),
        operation: AgentOperation::Execute,
        context: AgentContext {
            cwd: cwd.into(),
            deadline: Utc::now() + chrono::Duration::minutes(1),
            run_id: Some(Uuid::new_v4()),
            task_id: Some("copilot-literal-test".into()),
            parameters: Default::default(),
            prompt: Some(prompt.into()),
            session_id: None,
            session_title: None,
            job_id: None,
        },
        input: serde_json::Value::Null,
    }
}

#[test]
fn native_pipe_contract_retains_native_permissions_without_prompt_argv() {
    let descriptor = descriptor();
    descriptor.validate().unwrap();
    assert_eq!(
        descriptor.operations,
        [
            AgentOperation::Detect,
            AgentOperation::Execute,
            AgentOperation::Reconcile,
        ]
        .into()
    );
    let request = request(".", "piped prompt");
    request.validate(&descriptor).unwrap();
    // Exact native argv rejects extra permission flags. No -p: it discards piped input.
    assert_eq!(
        native_args(&request, false).unwrap(),
        ["copilot", "--no-ask-user", "--output-format", "json"]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn model_and_multiline_prompt_are_passed_as_literal_argv_and_stdin() {
    use std::{process::Stdio, time::Duration};
    use tokio::{io::AsyncWriteExt, process::Command};

    let directory = tempfile::tempdir().unwrap();
    let model = "model with spaces; $(printf expanded) {{task.id}}";
    let prompt = "Keep this literal:\n`printf expanded`\n$(printf expanded)\n'\" {{run.id}} 中文";
    let mut request = request(directory.path().to_str().unwrap(), prompt);
    request
        .context
        .parameters
        .insert("model".into(), model.into());
    request.validate(&descriptor()).unwrap();
    let args = native_args(&request, false).unwrap();
    // Record the production SDK's native argv and input without launching Copilot.
    // Legacy CommandSpec interpolation must not be applied to already literal SDK input.
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "printf '%s\\0' \"$@\"; printf '\\n'; cat",
            "copilot-test",
        ])
        .args(args)
        .current_dir(directory.path())
        .env("HOME", directory.path())
        .env("USERPROFILE", directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.context.prompt.as_ref().unwrap().as_bytes())
        .await
        .unwrap();
    let output = tokio::time::timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_args = [
        "copilot",
        "--no-ask-user",
        "--output-format",
        "json",
        "--model",
        model,
    ];
    let expected = format!("{}\0\n{prompt}", expected_args.join("\0"));
    assert_eq!(output.stdout, expected.as_bytes());
}
