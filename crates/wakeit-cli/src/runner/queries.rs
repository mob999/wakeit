//! Read-only device capabilities. Queries never enter the execution journal or durable queues.
use crate::runner::config::RunnerConfig;
use crate::{
    config::{AgentConfig, TaskConfig},
    process,
    protocol::{self, ModelCatalog, QueryError, QueryReply, QueryRequest},
};
use anyhow::{Context, Result, ensure};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub fn directory_roots(roots: &[PathBuf]) -> Vec<String> {
    roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .filter(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
pub fn list_directories(
    roots: &[PathBuf],
    path: Option<&str>,
    cursor: Option<&str>,
) -> Result<Value> {
    ensure!(!roots.is_empty(), "allowed_roots is not configured");
    if path.is_none() {
        return Ok(
            json!({"path":null,"parent":null,"breadcrumbs":[],"directories":directory_roots(roots).into_iter().map(|path| json!({"name":path,"path":path})).collect::<Vec<_>>(),"next_cursor":null}),
        );
    }
    let path = path.context("path")?;
    process::validate_cwd(path, roots)?;
    let current = Path::new(path).canonicalize()?;
    let root = roots
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .filter(|p| current.starts_with(p))
        .max_by_key(|p| p.components().count())
        .context("outside allowed_roots")?;
    let mut directories = Vec::new();
    for entry in std::fs::read_dir(&current)? {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = match entry.file_name().into_string() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Ok(real) = entry.path().canonicalize() else {
            continue;
        };
        if real.is_dir()
            && process::validate_cwd(&real.to_string_lossy(), roots).is_ok()
            && cursor.is_none_or(|c| name.as_str() > c)
        {
            directories.push((name, entry.path().to_string_lossy().into_owned()));
        }
    }
    directories.sort_by(|a, b| a.0.cmp(&b.0));
    let next_cursor = if directories.len() > 200 {
        Some(directories[199].0.clone())
    } else {
        None
    };
    directories.truncate(200);
    let mut breadcrumbs = Vec::new();
    let mut crumb = root.clone();
    breadcrumbs.push(json!({"name":root.file_name().map(|n|n.to_string_lossy()).unwrap_or_else(||root.to_string_lossy()),"path":root.to_string_lossy()}));
    for part in current.strip_prefix(&root)?.components() {
        crumb.push(part);
        breadcrumbs.push(
            json!({"name":part.as_os_str().to_string_lossy(),"path":crumb.to_string_lossy()}),
        );
    }
    let parent = if current == root {
        None
    } else {
        current.parent().map(|p| p.to_string_lossy().into_owned())
    };
    Ok(
        json!({"path":current.to_string_lossy(),"parent":parent,"breadcrumbs":breadcrumbs,"directories":directories.into_iter().map(|(name,path)|json!({"name":name,"path":path})).collect::<Vec<_>>(),"next_cursor":next_cursor}),
    )
}
pub async fn models(agent: &AgentConfig, cwd: &str) -> Result<ModelCatalog> {
    let command = agent
        .models
        .as_ref()
        .context("agent has no models interface")?;
    let task = TaskConfig {
        dag: None,
        version: 1,
        id: "models-query".into(),
        name: "Model query".into(),
        runner: "local".into(),
        agent: agent.id.clone(),
        cwd: cwd.into(),
        prompt: String::new(),
        parameters: crate::config::resolve_parameters(&agent.parameters, &Default::default())?,
        session: None,
        schedule: None,
        enabled: false,
        execution: Default::default(),
    };
    let output = process::execute(
        command,
        &task,
        Uuid::new_v4(),
        None,
        Duration::from_secs(25),
        CancellationToken::new(),
        None,
    )
    .await?;
    ensure!(
        output.code == Some(0) && output.stopped.is_none(),
        "models command failed or timed out"
    );
    let catalog: ModelCatalog =
        serde_json::from_str(&output.stdout).context("models command returned invalid JSON")?;
    ensure!(
        catalog.version == 1 && catalog.models.len() <= 10000,
        "invalid models response version or size"
    );
    let mut ids = std::collections::HashSet::new();
    for model in &catalog.models {
        ensure!(
            model.parameters.get("model").is_some_and(|v| !v.is_empty()),
            "model entry must bind the model parameter"
        );
        ensure!(
            !model.id.is_empty() && !model.name.is_empty() && ids.insert(&model.id),
            "invalid or duplicate model ID"
        );
        crate::config::resolve_parameters(&agent.parameters, &model.parameters)?;
        ensure!(
            model
                .parameter_options
                .keys()
                .all(|k| agent.parameters.contains_key(k)),
            "undeclared model parameter options"
        );
    }
    Ok(catalog)
}
pub async fn serve(
    client: async_nats::Client,
    config: Arc<RunnerConfig>,
    agents: Arc<HashMap<String, AgentConfig>>,
    runtime: Uuid,
    stop: CancellationToken,
) -> Result<()> {
    let mut sub = client.subscribe(protocol::query_subject(config.id)).await?;
    let permits = Arc::new(tokio::sync::Semaphore::new(8));
    let image_permits = Arc::new(tokio::sync::Semaphore::new(2));
    loop {
        let msg = tokio::select! {_=stop.cancelled()=>break,msg=sub.next()=>match msg {Some(m)=>m,None=>break}};
        let Some(reply) = msg.reply else { continue };
        if !reply
            .as_str()
            .starts_with(&(protocol::reply_prefix(config.id) + "."))
        {
            continue;
        }
        let Ok(request) = serde_json::from_slice::<QueryRequest>(&msg.payload) else {
            continue;
        };
        let client = client.clone();
        let config = config.clone();
        let agents = agents.clone();
        let stop = stop.clone();
        let permit = permits.clone().try_acquire_owned();
        let image_permits = image_permits.clone();
        tokio::spawn(async move {
            let result: Result<Value> = async {
                let _permit = permit.context("query capacity reached")?;
                ensure!(
                    request.version == 1 && request.runtime_id == runtime,
                    "runner runtime changed"
                );
                match request.action.as_str() {
                    "read_result_file" => {
                        let image: crate::runner::image_preview::Request = serde_json::from_value(request.payload.clone())?;
                        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
                        let image_permit = if matches!(image.image, Some(crate::runner::image_preview::Mode::Thumbnail)) {
                            Some(tokio::select! {
                                _ = stop.cancelled() => anyhow::bail!("Runner is stopping"),
                                permit = tokio::time::timeout_at(deadline, image_permits.acquire_owned()) => permit.context("file preview timed out")??,
                            })
                        } else { None };
                        let roots=config.allowed_roots.clone();
                        let href=request.payload["href"].as_str().context("file link is required")?.to_owned();
                        let cwd=request.payload["cwd"].as_str().map(str::to_owned);
                        let limit=(client.server_info().max_payload / 2).min(crate::runner::result_files::MAX_BYTES);
                        tokio::select! {
                            _ = stop.cancelled() => anyhow::bail!("Runner is stopping"),
                            value = tokio::time::timeout_at(deadline,tokio::task::spawn_blocking(move || { let _held=(_permit, image_permit); crate::runner::image_preview::read(&roots,&href,cwd.as_deref(),limit,image) })) => value.context("file preview timed out")??,
                        }
                    }
                    "list_directories" => {
                        let roots = config.allowed_roots.clone();
                        let path = request.payload["path"].as_str().map(str::to_owned);
                        let cursor = request.payload["cursor"].as_str().map(str::to_owned);
                        tokio::task::spawn_blocking(move || {
                            list_directories(&roots, path.as_deref(), cursor.as_deref())
                        })
                        .await?
                    }
                    "list_sessions" => {
                        let cwd = request.payload["cwd"].as_str().context("cwd is required")?;
                        process::validate_cwd(cwd,&config.allowed_roots)?;
                        let agent=agents.get(request.payload["agent"].as_str().context("agent is required")?).context("agent is unavailable")?;
                        crate::adapters::catalog::query(agent,cwd,request.payload["q"].as_str(),request.payload["cursor"].as_str()).await
                    }
                    "inspect_session" => {
                        let cwd = request.payload["cwd"].as_str().context("cwd is required")?;
                        process::validate_cwd(cwd, &config.allowed_roots)?;
                        let agent = agents.get(request.payload["agent"].as_str().context("agent is required")?).context("agent is unavailable")?;
                        let id=request.payload["id"].as_str().context("session ID is required")?;
                        let task:TaskConfig=serde_json::from_value(json!({"version":1,"id":"session-query","name":"session query","runner":config.name,"agent":agent.id,"cwd":cwd,"prompt":"","schedule":null}))?;
                        Ok(serde_json::to_value(crate::runner::sessions::inspect(agent,&task,Uuid::new_v4(),id).await?)?)
                    }
                    "list_models" => {
                        let cwd = request.payload["cwd"].as_str().context("cwd is required")?;
                        process::validate_cwd(cwd, &config.allowed_roots)?;
                        let id = request.payload["agent"]
                            .as_str()
                            .context("agent is required")?;
                        let agent = agents.get(id).context("agent is unavailable")?;
                        Ok(serde_json::to_value(models(agent, cwd).await?)?)
                    }
                    _ => anyhow::bail!("unsupported query action"),
                }
            }
            .await;
            let (data, error) = match result {
                Ok(v) => (Some(v), None),
                Err(e) => (
                    None,
                    Some(QueryError {
                        code: "query_failed".into(),
                        message: format!("{e:#}"),
                    }),
                ),
            };
            let response = QueryReply {
                version: 1,
                id: request.id,
                runtime_id: runtime,
                data,
                error,
            };
            if !stop.is_cancelled() {
                let _ = client
                    .publish(
                        reply,
                        bounded_response(response, client.server_info().max_payload).into(),
                    )
                    .await;
            }
        });
    }
    Ok(())
}

fn bounded_response(mut response: QueryReply, max_payload: usize) -> Vec<u8> {
    let encoded = serde_json::to_vec(&response).unwrap_or_default();
    if encoded.len() <= max_payload {
        return encoded;
    }
    response.data = None;
    response.error = Some(QueryError {
        code: "query_result_too_large".into(),
        message: "query response exceeds the transport limit".into(),
    });
    serde_json::to_vec(&response).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_text_cannot_exceed_the_query_transport_limit() {
        let reply = QueryReply {
            version: 1,
            id: Uuid::new_v4(),
            runtime_id: Uuid::new_v4(),
            data: Some(json!({"content": "\u{1}".repeat(500)})),
            error: None,
        };
        let encoded = bounded_response(reply, 2048);
        assert!(encoded.len() < 2048);
        let reply: QueryReply = serde_json::from_slice(&encoded).unwrap();
        assert!(reply.data.is_none());
        assert_eq!(reply.error.unwrap().code, "query_result_too_large");
    }
}
