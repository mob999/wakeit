use super::{args::*, client::Client, display};
use crate::config::{self, TaskConfig};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

pub(super) async fn task(cli: &Cli, command: &Task) -> Result<()> {
    let c = Client::new(cli)?;
    match command {
        Task::Create {
            template,
            values,
            run_now,
        } => {
            let request = template_request(&c, template, values, *run_now).await?;
            display(
                c.post_idempotent(
                    &format!("/task-templates/{template}/instantiate"),
                    serde_json::to_value(request)?,
                )
                .await?,
                cli.json,
            );
        }
        Task::Apply { file, expect } => {
            let task: TaskConfig = if file.as_os_str() == "-" {
                use std::io::Read;
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                yaml_serde::from_str(&s)?
            } else {
                config::read(file)?
            };
            task.validate()?;
            let expected = if let Some(e) = expect {
                Some(*e)
            } else {
                let response = c
                    .http
                    .get(c.api_url(&format!("/tasks/{}", task.id)))
                    .bearer_auth(c.token.trim())
                    .send()
                    .await?;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    None
                } else {
                    let status = response.status();
                    let v: Value = response.json().await?;
                    ensure!(status.is_success(), "{status}: {}", v["error"]);
                    Some(v["revision"].as_i64().context("revision")?)
                }
            };
            display(
                c.request(
                    reqwest::Method::PUT,
                    &format!("/tasks/{}", task.id),
                    Some(json!({"config":task,"expected_revision":expected})),
                )
                .await?,
                cli.json,
            );
        }
        Task::List { archived } => display(
            c.get(if *archived {
                "/tasks?archived=true"
            } else {
                "/tasks"
            })
            .await?,
            cli.json,
        ),
        Task::Archive { id } | Task::Restore { id } => {
            let old = c.get(&format!("/tasks/{id}")).await?;
            let operation = if matches!(command, Task::Archive { .. }) {
                "archive"
            } else {
                "restore"
            };
            display(
                c.post(
                    &format!("/tasks/{id}/{operation}"),
                    json!({"expected_revision":old["revision"]}),
                )
                .await?,
                cli.json,
            );
        }
        Task::Get { id } => display(c.get(&format!("/tasks/{id}")).await?, true),
        Task::Trigger { id } => display(
            c.post(&format!("/tasks/{id}/runs"), json!({})).await?,
            cli.json,
        ),
        Task::Pause { id } | Task::Resume { id } => {
            let old = c.get(&format!("/tasks/{id}")).await?;
            let mut config = old["config"].clone();
            config["enabled"] = json!(matches!(command, Task::Resume { .. }));
            display(
                c.request(
                    reqwest::Method::PUT,
                    &format!("/tasks/{id}"),
                    Some(json!({"config":config,"expected_revision":old["revision"]})),
                )
                .await?,
                cli.json,
            );
        }
    }
    Ok(())
}
pub(super) async fn template(cli: &Cli, command: &Template) -> Result<()> {
    if let Template::Validate { file } = command {
        let definition: crate::task_templates::Definition =
            serde_json::from_value(read_template_document(file)?)?;
        let issues = crate::task_templates::validate_definition(&definition);
        display(json!({"valid":issues.is_empty(),"issues":issues}), true);
        ensure!(issues.is_empty(), "template validation failed");
        return Ok(());
    }
    let c = Client::new(cli)?;
    match command {
        Template::List { archived } => display(
            c.get(&format!("/task-templates?archived={archived}"))
                .await?,
            cli.json,
        ),
        Template::Get { id } => {
            let record = c.get(&format!("/task-templates/{id}")).await?;
            if cli.json {
                display(record, true);
            } else {
                print!("{}", yaml_serde::to_string(&record["definition"])?);
            }
        }
        Template::Apply { file, expect } => {
            let definition: crate::task_templates::Definition =
                serde_json::from_value(read_template_document(file)?)?;
            let path = format!("/task-templates/{}", definition.id);
            let expected = if let Some(value) = expect {
                Some(*value)
            } else {
                c.optional(&path)
                    .await?
                    .and_then(|v| v["revision"].as_i64())
            };
            display(
                c.request(
                    reqwest::Method::PUT,
                    &path,
                    Some(json!({"definition":definition,"expected_revision":expected})),
                )
                .await?,
                cli.json,
            );
        }
        Template::Validate { .. } => unreachable!(),
        Template::Preview { id, values } => {
            let request = template_request(&c, id, values, false).await?;
            let preview = c
                .post(
                    &format!("/task-templates/{id}/preview"),
                    serde_json::to_value(request)?,
                )
                .await?;
            let valid = preview["issues"].as_array().is_some_and(Vec::is_empty);
            display(preview, true);
            ensure!(valid, "template preview has validation errors");
        }
        Template::Archive { id } | Template::Restore { id } => {
            let revision = c.get(&format!("/task-templates/{id}")).await?["revision"].clone();
            let operation = if matches!(command, Template::Archive { .. }) {
                "archive"
            } else {
                "restore"
            };
            display(
                c.post(
                    &format!("/task-templates/{id}/{operation}"),
                    json!({"expected_revision":revision}),
                )
                .await?,
                cli.json,
            );
        }
        Template::FromTask { id, file } => {
            let definition = c.get(&format!("/tasks/{id}/template")).await?;
            if let Some(file) = file {
                ensure!(!file.exists(), "file already exists");
                std::fs::write(file, yaml_serde::to_string(&definition)?)?;
            } else if cli.json {
                display(definition, true);
            } else {
                print!("{}", yaml_serde::to_string(&definition)?);
            }
        }
    }
    Ok(())
}
fn read_template_document(file: &Path) -> Result<Value> {
    crate::task_templates::document::parse(&std::fs::read_to_string(file)?)
        .with_context(|| format!("invalid template document: {}", file.display()))
}
async fn template_request(
    client: &Client,
    id: &str,
    options: &TemplateValues,
    run_now: bool,
) -> Result<crate::task_templates::InstanceRequest> {
    let record = client.get(&format!("/task-templates/{id}")).await?;
    let mut request: crate::task_templates::InstanceRequest = if let Some(file) =
        &options.inputs_file
    {
        let input = read_template_document(file)?;
        ensure!(
            input
                .as_object()
                .is_some_and(|o| o.keys().all(|k| ["values", "unset"].contains(&k.as_str()))),
            "input file must contain values and optional unset"
        );
        serde_json::from_value(
            json!({"expected_revision":options.expect.or(record["revision"].as_i64()),"values":input.get("values").cloned().unwrap_or(json!({})),"unset":input.get("unset").cloned().unwrap_or(json!([]))}),
        )?
    } else {
        crate::task_templates::InstanceRequest {
            expected_revision: options
                .expect
                .or(record["revision"].as_i64())
                .context("missing template revision")?,
            ..Default::default()
        }
    };
    if let Some(file) = &options.bindings_file {
        request.bindings = serde_json::from_value(read_template_document(file)?)?;
    }
    if let Some(file) = &options.schedule_file {
        request.schedule = Some(read_template_document(file)?);
    }
    request.task_id = options.id.clone();
    request.name = options.name.clone();
    request.run_now = run_now;
    Ok(request)
}
