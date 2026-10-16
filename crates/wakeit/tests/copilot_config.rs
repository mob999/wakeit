//! Native Copilot CLI JSONL input framing and permissions.
use wakeit::config::{AgentConfig, CommandArg, OutputFormat};

#[test]
fn native_pipe_contract_is_jsonl_and_retains_native_permissions() {
    let config: AgentConfig =
        yaml_serde::from_str(include_str!("../../../agents/copilot.yaml")).unwrap();
    config.validate().unwrap();
    assert!(matches!(config.output.format, OutputFormat::Events));
    assert_eq!(config.submit.stdin.as_deref(), Some("{{task.prompt}}"));
    assert!(config.models.is_none() && config.session.is_none());
    let args: Vec<&str> = config
        .submit
        .args
        .iter()
        .filter_map(|v| match v {
            CommandArg::Literal(v) => Some(v.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        args,
        [
            "adapter",
            "stream",
            "copilot",
            "--",
            "copilot",
            "--no-ask-user",
            "--output-format",
            "json"
        ]
    );
    // No -p: upstream explicitly says -p would silently discard piped input.
    assert!(!args.contains(&"-p") && !args.contains(&"--prompt"));
    assert!(!args.contains(&"--allow-all") && !args.contains(&"--allow-all-tools"));
}

#[cfg(unix)]
#[tokio::test]
async fn model_and_multiline_prompt_are_passed_as_literal_argv_and_stdin() {
    use std::{collections::BTreeMap, time::Duration};
    use tokio_util::sync::CancellationToken;
    use uuid::Uuid;
    use wakeit::{
        config::{TaskConfig, resolve_parameters},
        process,
    };

    let config: AgentConfig =
        yaml_serde::from_str(include_str!("../../../agents/copilot.yaml")).unwrap();
    let model = "model with spaces; $(printf expanded) {{task.id}}";
    let prompt = "Keep this literal:\n`printf expanded`\n$(printf expanded)\n'\" {{run.id}} 中文";
    let parameters = resolve_parameters(
        &config.parameters,
        &BTreeMap::from([("model".into(), model.into())]),
    )
    .unwrap();
    let task = TaskConfig {
        version: 1,
        id: "copilot-literal-test".into(),
        name: "test".into(),
        runner: "test".into(),
        agent: "copilot".into(),
        cwd: std::env::current_dir()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        prompt: prompt.into(),
        parameters,
        session: None,
        schedule: None,
        enabled: false,
        execution: Default::default(),
    };
    let mut spec = config.submit;
    // This fixed local helper records received argv and stdin. It never launches Copilot.
    spec.command = "/bin/sh".into();
    let mut prefix: Vec<CommandArg> = [
        "-c",
        "printf '%s\\0' \"$@\"; printf '\\n'; cat",
        "copilot-test",
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    prefix.append(&mut spec.args);
    spec.args = prefix;
    let output = process::execute(
        &spec,
        &task,
        Uuid::new_v4(),
        None,
        Duration::from_secs(5),
        CancellationToken::new(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(output.code, Some(0));
    let expected_args = [
        "adapter",
        "stream",
        "copilot",
        "--",
        "copilot",
        "--no-ask-user",
        "--output-format",
        "json",
        "--model",
        model,
    ];
    let expected = format!("{}\0\n{prompt}", expected_args.join("\0"));
    assert_eq!(output.stdout, expected);
}
