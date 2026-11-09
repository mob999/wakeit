//! Host APIs exposed to an individual extension instance.
use super::peer::Host;
use super::*;
use futures::future::BoxFuture;
pub(super) struct InstanceHost {
    pub(super) runtime: Runtime,
    pub(super) instance: Uuid,
    pub(super) snapshot: Instance,
    pub(super) agent_context: Option<(
        wakeit_plugin_protocol::adapter::AgentRequest,
        CancellationToken,
        Arc<std::sync::atomic::AtomicBool>,
    )>,
}
impl Host for InstanceHost {
    fn request(&self, method: String, params: Value) -> BoxFuture<'static, Result<Value>> {
        let runtime = self.runtime.clone();
        let id = self.instance;
        let snapshot = self.snapshot.clone();
        let agent_context = self.agent_context.clone();
        Box::pin(async move {
            if let Some((request, stop, _)) = &agent_context {
                ensure!(
                    !stop.is_cancelled() && request.context.deadline > Utc::now(),
                    "Agent host request is no longer active"
                );
            }
            let mut instance = snapshot;
            let current = runtime.instance(id).await?;
            ensure!(
                current.enabled
                    || method.starts_with("storage.")
                    || [
                        "config.get",
                        "credentials.status",
                        "runtime.info",
                        "log.write"
                    ]
                    .contains(&method.as_str()),
                "extension was disabled"
            );
            instance.grants = current
                .grants
                .iter()
                .filter(|grant| {
                    instance.declaration.permissions.contains(grant)
                        && current.declaration.permissions.contains(grant)
                })
                .cloned()
                .collect();
            if ![
                "config.get",
                "credentials.status",
                "runtime.info",
                "log.write",
                "health.report",
                "storage.get",
                "storage.list",
                "storage.set",
            ]
            .contains(&method.as_str())
            {
                let authorized = runtime.authorized_grants(id).await?;
                instance.grants.retain(|grant| authorized.contains(grant));
            }
            if let Some(snapshot) = sqlx::query_scalar::<_, String>(
                "SELECT snapshot FROM extension_versions WHERE instance=? AND revision=?",
            )
            .bind(id.to_string())
            .bind(instance.revision)
            .fetch_optional(&runtime.journal.pool)
            .await?
            {
                let snapshot: Value = serde_json::from_str(&snapshot)?;
                instance.config = snapshot["config"].clone();
            }
            let invocation = params["invocation_id"].as_str();
            let in_hook = agent_context
                .as_ref()
                .is_some_and(|(request, _, _)| request.operation.is_read_only())
                || if let Some(invocation) = invocation {
                    sqlx::query_scalar::<_, String>(
                        "SELECT kind FROM extension_invocations WHERE id=? AND instance=?",
                    )
                    .bind(invocation)
                    .bind(id.to_string())
                    .fetch_optional(&runtime.journal.pool)
                    .await?
                        == Some("hook".into())
                } else {
                    false
                };
            let actor = if let Some(invocation) = invocation {
                sqlx::query_scalar::<_, String>(
                    "SELECT actor FROM extension_invocations WHERE id=? AND instance=?",
                )
                .bind(invocation)
                .bind(id.to_string())
                .fetch_optional(&runtime.journal.pool)
                .await?
                .map(|s| serde_json::from_str::<wakeit_core::extensions::Actor>(&s))
                .transpose()?
            } else {
                None
            };
            // The absence of an invocation is administrative only for lifecycle callbacks,
            // not for Agent requests or a caller supplying an unknown invocation ID.
            let user_admin = agent_context.is_none()
                && (invocation.is_none()
                    || actor.as_ref().is_some_and(|actor| {
                        actor.kind == "user" && actor.roles.iter().any(|role| role == "admin")
                    }));
            if method.starts_with("storage.") {
                ensure!(
                    !in_hook || ["storage.get", "storage.list"].contains(&method.as_str()),
                    "Read-only plugin context cannot modify storage"
                );
                return store::storage(&runtime.journal.pool, id, &method, &params).await;
            }
            match method.as_str() {
                "runtime.info" => Ok(serde_json::to_value(
                    wakeit_core::extensions::RuntimeInfo {
                        runner_id: runtime.config.id,
                        instance_id: id,
                        sdk_protocol: 1,
                        version: env!("CARGO_PKG_VERSION").into(),
                        platform: std::env::consts::OS.into(),
                        arch: std::env::consts::ARCH.into(),
                        invocation_id: agent_context
                            .as_ref()
                            .map(|(request, _, _)| request.id)
                            .or(invocation.map(str::parse).transpose()?),
                        public_url: public_url(&runtime.http, &runtime.config.gateway).await,
                    },
                )?),
                "providers.query" => {
                    ensure!(
                        current.state == "running",
                        "extension providers are unavailable during activation"
                    );
                    let provider = params["provider"]
                        .as_str()
                        .context("provider ID required")?;
                    let declaration = instance
                        .declaration
                        .providers
                        .get(provider)
                        .context("provider not declared")?;
                    wakeit_core::extensions::validate_schema(&declaration.input, &params["input"])?;
                    let result = runtime
                        .invoke(Invocation {
                            id: Uuid::new_v4(),
                            operation_id: Uuid::new_v4(),
                            instance_id: id,
                            kind: "provider".into(),
                            handler: provider.into(),
                            input: params["input"].clone(),
                            deadline: Utc::now() + chrono::Duration::seconds(25),
                            expected_revision: Some(instance.revision),
                            actor: wakeit_core::extensions::Actor {
                                kind: "plugin".into(),
                                id: Some(id.to_string()),
                                roles: vec![],
                            },
                            causation_id: None,
                        })
                        .await?;
                    wakeit_core::extensions::validate_schema(&declaration.output, &result)?;
                    if declaration.kind == "models" {
                        let catalog: crate::protocol::ModelCatalog =
                            serde_json::from_value(result.clone())?;
                        ensure!(catalog.version == 1, "invalid model catalog version");
                    }
                    if declaration.kind == "sessions" {
                        let catalog: wakeit_core::session_catalog::Catalog =
                            serde_json::from_value(result.clone())?;
                        ensure!(catalog.version == 1, "invalid session catalog version");
                        let expected = params["input"]["cwd"]
                            .as_str()
                            .context("session provider requires a working directory")?;
                        let cwd = std::path::Path::new(expected).canonicalize()?;
                        for session in catalog.sessions {
                            wakeit_core::sessions::validate_id(&session.id)?;
                            ensure!(
                                std::path::Path::new(&session.cwd).canonicalize()? == cwd,
                                "session provider returned another working directory"
                            );
                        }
                    }
                    Ok(result)
                }
                "executions.get" => {
                    ensure!(
                        instance.grants.contains(&"executions:read".into()),
                        "execution access not granted"
                    );
                    let run: Uuid = params["id"].as_str().context("run ID required")?.parse()?;
                    let local = runtime
                        .journal
                        .get(run)
                        .await?
                        .context("execution not found")?;
                    Ok(
                        json!({"id":run,"task":local.assignment.task,"phase":local.phase,"session":local.session,"started_at":local.started}),
                    )
                }
                "results.read" => {
                    ensure!(
                        instance.grants.contains(&"results:read".into()),
                        "result access not granted"
                    );
                    let reference = params["id"].as_str().context("result reference required")?;
                    let row = sqlx::query(
                        "SELECT run_id,kind,data,created_at FROM extension_results WHERE id=?",
                    )
                    .bind(reference)
                    .fetch_one(&runtime.journal.pool)
                    .await?;
                    Ok(
                        json!({"id":reference,"run_id":row.get::<String,_>("run_id"),"kind":row.get::<String,_>("kind"),"result":serde_json::from_str::<Value>(row.get("data"))?,"created_at":row.get::<String,_>("created_at")}),
                    )
                }
                "files.watch" | "files.unwatch" => {
                    ensure!(
                        !in_hook && instance.grants.contains(&"files:watch".into()),
                        "file watching not granted"
                    );
                    if method == "files.watch" {
                        super::watch::start(&runtime, id, &params).await
                    } else {
                        super::watch::stop(
                            &runtime,
                            id,
                            params["id"]
                                .as_str()
                                .context("watch ID required")?
                                .parse()?,
                        )
                        .await
                    }
                }
                "files.stat" | "files.read" => {
                    ensure!(
                        instance.grants.contains(&"files:read".into()),
                        "file access not granted"
                    );
                    let target = params["runner"]
                        .as_str()
                        .and_then(|s| s.parse::<Uuid>().ok());
                    let mut request = params.clone();
                    request["operation"] = json!(if method == "files.stat" {
                        "stat"
                    } else {
                        "read"
                    });
                    if target.is_some_and(|r| r != runtime.config.id) {
                        runtime.remote(id, "files.device_read", request).await
                    } else {
                        super::files::read(&runtime, &request).await
                    }
                }
                "artifacts.register" | "artifacts.export" => {
                    ensure!(
                        !in_hook && instance.grants.contains(&"artifacts:write".into()),
                        "artifact creation not granted"
                    );
                    super::files::register(&runtime, id, &params).await
                }
                "artifacts.get" | "artifacts.list" => {
                    ensure!(
                        instance.grants.contains(&"artifacts:read".into()),
                        "artifact reading not granted"
                    );
                    let rows=sqlx::query("SELECT id,name,reference,metadata,expires_at FROM extension_artifacts WHERE instance=? AND (? IS NULL OR id=?) AND (expires_at IS NULL OR expires_at>?) ORDER BY id LIMIT 200").bind(id.to_string()).bind(params["id"].as_str()).bind(params["id"].as_str()).bind(Utc::now().to_rfc3339()).fetch_all(&runtime.journal.pool).await?;
                    Ok(
                        json!({"items":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"name":r.get::<String,_>("name"),"reference":serde_json::from_str::<Value>(r.get("reference")).unwrap_or(Value::Null),"metadata":serde_json::from_str::<Value>(r.get("metadata")).unwrap_or(Value::Null),"expires_at":r.get::<Option<String>,_>("expires_at")})).collect::<Vec<_>>()}),
                    )
                }
                "http.request" => {
                    ensure!(
                        instance.grants.contains(&"http:request".into()),
                        "HTTP access is not granted"
                    );
                    ensure!(
                        !in_hook || params["method"].as_str().unwrap_or("GET") == "GET",
                        "Read-only plugin HTTP access must be read-only"
                    );
                    let url = reqwest::Url::parse(params["url"].as_str().context("URL required")?)?;
                    ensure!(
                        ["http", "https"].contains(&url.scheme()),
                        "unsupported URL scheme"
                    );
                    let method = params["method"].as_str().unwrap_or("GET").parse()?;
                    let mut request = runtime
                        .http
                        .request(method, url)
                        .timeout(Duration::from_secs(30));
                    if let Some(headers) = params["headers"].as_object() {
                        for (key, value) in headers {
                            request = request
                                .header(key, value.as_str().context("header value must be text")?);
                        }
                    }
                    if let Some(body) = params.get("body") {
                        request = request.json(body);
                    }
                    let mut response = request.send().await?;
                    let status = response.status().as_u16();
                    let mut bytes = Vec::new();
                    while let Some(chunk) = response.chunk().await? {
                        ensure!(
                            bytes.len() + chunk.len() <= 4 * 1024 * 1024,
                            "HTTP response exceeds inline limit"
                        );
                        bytes.extend(chunk);
                    }
                    Ok(json!({"status":status,"body":String::from_utf8_lossy(&bytes)}))
                }
                "process.execute" => {
                    ensure!(
                        !in_hook && instance.grants.contains(&"process:execute".into()),
                        "process execution not granted in this context"
                    );
                    let cwd = params["cwd"]
                        .as_str()
                        .context("working directory required")?;
                    crate::process::validate_cwd(cwd, &runtime.config.allowed_roots)?;
                    let spec: crate::config::CommandSpec =
                        serde_json::from_value(params["command"].clone())?;
                    let task: crate::config::TaskConfig = serde_json::from_value(
                        json!({"version":1,"id":"plugin-action","name":instance.name,"runner":runtime.config.name,"agent":"plugin","cwd":cwd,"prompt":""}),
                    )?;
                    if let Some((request, _, _)) = &agent_context {
                        ensure!(
                            std::path::Path::new(cwd).canonicalize()?
                                == std::path::Path::new(&request.context.cwd).canonicalize()?,
                            "Agent process working directory differs from its Run"
                        );
                    }
                    let signal = if let Some((_, signal, _)) = &agent_context {
                        signal.clone()
                    } else if let Some(call) = invocation.and_then(|s| s.parse::<Uuid>().ok()) {
                        runtime
                            .calls
                            .lock()
                            .await
                            .get(&call)
                            .cloned()
                            .unwrap_or_else(|| runtime.stop.child_token())
                    } else {
                        runtime.stop.child_token()
                    };
                    if let Some((_, _, pending)) = &agent_context {
                        pending.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    let output = crate::process::execute(
                        &spec,
                        &task,
                        agent_context
                            .as_ref()
                            .and_then(|(r, _, _)| r.context.run_id)
                            .unwrap_or_else(Uuid::new_v4),
                        None,
                        Duration::from_secs(
                            params["timeout_seconds"].as_u64().unwrap_or(60).min(3600),
                        )
                        .min(
                            agent_context
                                .as_ref()
                                .map(|(r, _, _)| {
                                    (r.context.deadline - Utc::now())
                                        .to_std()
                                        .unwrap_or_default()
                                })
                                .unwrap_or(Duration::from_secs(3600)),
                        ),
                        signal,
                        None,
                    )
                    .await;
                    if let Some((_, _, pending)) = &agent_context {
                        let unfinished = match &output {
                            Ok(o) => !o.confirmed,
                            Err(e) => e.is::<crate::process::CommandStartedError>(),
                        };
                        pending.store(unfinished, std::sync::atomic::Ordering::SeqCst);
                    }
                    let output = output?;
                    Ok(
                        json!({"exit_code":output.code,"stdout":output.stdout,"stderr":output.stderr,"confirmed":output.confirmed,"stopped":output.stopped}),
                    )
                }
                "events.emit" => {
                    ensure!(!in_hook, "Hook cannot emit business events");
                    let name = params["event"].as_str().context("event required")?;
                    let schema = instance
                        .declaration
                        .events
                        .get(name)
                        .context("custom event not declared")?;
                    wakeit_core::extensions::validate_schema(schema, &params["data"])?;
                    let event_type =
                        format!("plugin:{}#{}/{}", instance.source, instance.extension, name);
                    let event_id = Uuid::new_v4();
                    sqlx::query(
                        "INSERT INTO extension_events(id,event_type,instance,data) VALUES(?,?,?,?)",
                    )
                    .bind(event_id.to_string())
                    .bind(event_type)
                    .bind(id.to_string())
                    .bind(params["data"].to_string())
                    .execute(&runtime.journal.pool)
                    .await?;
                    Ok(json!({"id":event_id}))
                }
                "auth.token" | "auth.revoke" => {
                    ensure!(
                        instance.grants.contains(&"auth:manage".into()),
                        "authorization management not granted"
                    );
                    let slot = params["slot"]
                        .as_str()
                        .context("credential slot required")?;
                    ensure!(
                        instance.declaration.credentials.contains_key(slot),
                        "credential slot not declared"
                    );
                    if method == "auth.token" {
                        super::auth::token(
                            &runtime.journal.pool,
                            &runtime.http,
                            id,
                            slot,
                            params["token_url"].as_str(),
                            params["client_id"].as_str(),
                        )
                        .await
                    } else {
                        ensure!(
                            user_admin && !in_hook,
                            "administrator required to revoke credentials"
                        );
                        super::auth::revoke(
                            &runtime.journal.pool,
                            &runtime.http,
                            id,
                            slot,
                            params["revocation_url"].as_str(),
                        )
                        .await
                    }
                }
                "auth.begin" => {
                    ensure!(user_admin, "administrator required to authorize an account");
                    ensure!(
                        !in_hook && instance.grants.contains(&"auth:manage".into()),
                        "authorization flow not permitted"
                    );
                    let slot = params["slot"]
                        .as_str()
                        .context("credential slot required")?;
                    ensure!(
                        instance.declaration.credentials.contains_key(slot),
                        "credential slot not declared"
                    );
                    super::auth::begin(
                        &runtime.journal.pool,
                        &runtime.http,
                        id,
                        &params,
                        &runtime.config.gateway,
                        &runtime.token,
                        runtime.stop.child_token(),
                    )
                    .await
                }
                "auth.status" | "auth.cancel" => {
                    if method == "auth.cancel" {
                        ensure!(
                            user_admin && !in_hook,
                            "administrator required to cancel authorization"
                        );
                    }
                    super::auth::status(
                        &runtime.journal.pool,
                        id,
                        params["id"].as_str().context("flow ID required")?,
                        method == "auth.cancel",
                    )
                    .await
                }
                "config.get" => Ok(json!({"value":instance.config,"revision":instance.revision})),
                "credentials.status" => Ok(json!(instance.credential_status)),
                "config.save" => {
                    ensure!(
                        user_admin,
                        "administrator required to change plugin settings"
                    );
                    ensure!(
                        !in_hook,
                        "Read-only plugin context cannot modify configuration"
                    );
                    let update: ConfigUpdate = serde_json::from_value(params["update"].clone())?;
                    store::save(&runtime.journal.pool, id, update, &instance.declaration).await
                }
                "credentials.set" | "credentials.remove" => {
                    ensure!(user_admin, "administrator required to change credentials");
                    ensure!(
                        !in_hook,
                        "Read-only plugin context cannot modify credentials"
                    );
                    let slot = params["slot"]
                        .as_str()
                        .context("credential slot required")?
                        .to_owned();
                    let value = if method == "credentials.remove" {
                        wakeit_core::extensions::SecretUpdate::Clear
                    } else {
                        wakeit_core::extensions::SecretUpdate::Replace(
                            params["value"]
                                .as_str()
                                .context("credential value required")?
                                .into(),
                        )
                    };
                    store::save(
                        &runtime.journal.pool,
                        id,
                        ConfigUpdate {
                            operation_id: Uuid::new_v4(),
                            expected_revision: instance.revision,
                            config: instance.config.clone(),
                            secrets: [(slot, value)].into(),
                        },
                        &instance.declaration,
                    )
                    .await
                }
                "credentials.get" => {
                    let slot = params["slot"]
                        .as_str()
                        .context("credential slot required")?;
                    ensure!(
                        instance.declaration.credentials.contains_key(slot),
                        "credential slot not declared"
                    );
                    let secrets: String =
                        sqlx::query_scalar("SELECT secrets FROM extension_instances WHERE id=?")
                            .bind(id.to_string())
                            .fetch_one(&runtime.journal.pool)
                            .await?;
                    let secrets: Value = serde_json::from_str(&secrets)?;
                    Ok(secrets[slot].clone())
                }
                "ui.request" => {
                    ensure!(!in_hook, "Hook cannot wait for user interaction");
                    let call = invocation.context("interaction requires an action invocation")?;
                    let kind:Option<String>=sqlx::query_scalar("SELECT kind FROM extension_invocations WHERE id=? AND instance=? AND state='running'").bind(call).bind(id.to_string()).fetch_optional(&runtime.journal.pool).await?;
                    ensure!(
                        kind.as_deref() == Some("action"),
                        "interaction requires an active action"
                    );
                    let interaction: wakeit_core::extensions::Interaction =
                        serde_json::from_value(params["request"].clone())?;
                    if matches!(
                        &interaction,
                        wakeit_core::extensions::Interaction::Progress { .. }
                            | wakeit_core::extensions::Interaction::Toast { .. }
                            | wakeit_core::extensions::Interaction::Navigate { .. }
                            | wakeit_core::extensions::Interaction::Download { .. }
                    ) {
                        sqlx::query("INSERT INTO extension_interactions(id,invocation,instance,request,state) VALUES(?,?,?,?,'notification')").bind(Uuid::new_v4().to_string()).bind(call).bind(id.to_string()).bind(serde_json::to_string(&interaction)?).execute(&runtime.journal.pool).await?;
                        return Ok(Value::Null);
                    }
                    match &interaction {
                        wakeit_core::extensions::Interaction::OpenUrl { url, .. }
                        | wakeit_core::extensions::Interaction::DeviceCode { url, .. } => {
                            let url = reqwest::Url::parse(url)?;
                            ensure!(
                                ["http", "https"].contains(&url.scheme())
                                    && url.username().is_empty()
                                    && url.password().is_none(),
                                "unsupported authorization URL"
                            );
                        }
                        _ => {}
                    }
                    let identity = Uuid::new_v4().to_string();
                    sqlx::query("INSERT INTO extension_interactions(id,invocation,instance,request) VALUES(?,?,?,?)").bind(&identity).bind(call).bind(id.to_string()).bind(serde_json::to_string(&interaction)?).execute(&runtime.journal.pool).await?;
                    let signal = runtime
                        .calls
                        .lock()
                        .await
                        .get(&call.parse::<Uuid>()?)
                        .cloned()
                        .context("action is no longer active")?;
                    loop {
                        if runtime.stop.is_cancelled() || signal.is_cancelled() {
                            sqlx::query("UPDATE extension_interactions SET state='cancelled' WHERE id=? AND state='waiting'").bind(&identity).execute(&runtime.journal.pool).await?;
                            bail!("interaction cancelled");
                        }
                        if let Some(response) = runtime.responses.lock().await.remove(&identity) {
                            return Ok(response);
                        }
                        if let Some(call) = runtime.calls.lock().await.get(&call.parse::<Uuid>()?)
                            && call.is_cancelled()
                        {
                            bail!("interaction cancelled");
                        }
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
                "log.write" | "health.report" => {
                    let mut data = params.clone();
                    if let Some(object) = data.as_object_mut() {
                        object.remove("invocation_id");
                    }
                    let text = runtime.redact(id, &data.to_string()).await;
                    if method == "health.report" {
                        let message = data["message"].as_str().unwrap_or("");
                        let message = runtime.redact(id, message).await;
                        sqlx::query("UPDATE extension_instances SET error=? WHERE id=?")
                            .bind(message)
                            .bind(id.to_string())
                            .execute(&runtime.journal.pool)
                            .await?;
                    }
                    sqlx::query(
                        "INSERT INTO extension_events(id,event_type,instance,data) VALUES(?,?,?,?)",
                    )
                    .bind(Uuid::new_v4().to_string())
                    .bind(&method)
                    .bind(id.to_string())
                    .bind(text)
                    .execute(&runtime.journal.pool)
                    .await?;
                    sqlx::query("DELETE FROM extension_events WHERE instance=? AND event_type IN ('log.write','health.report') AND rowid NOT IN (SELECT rowid FROM extension_events WHERE instance=? AND event_type IN ('log.write','health.report') ORDER BY rowid DESC LIMIT 1000)").bind(id.to_string()).bind(id.to_string()).execute(&runtime.journal.pool).await?;
                    Ok(json!({"accepted":true}))
                }
                _ => {
                    let permission = method.split('.').next().context("invalid API method")?;
                    ensure!(
                        instance.grants.iter().any(|g| g == &method
                            || g == &format!("{permission}:read")
                            || g == &format!("{permission}:write")),
                        "host API permission denied: {method}"
                    );
                    if in_hook {
                        ensure!(
                            [
                                "get", "list", "validate", "preview", "inspect", "status", "read"
                            ]
                            .contains(&method.rsplit('.').next().unwrap_or("")),
                            "Read-only plugin context only permits read APIs"
                        );
                    }
                    runtime.remote(id, &method, params).await
                }
            }
        })
    }
}

async fn public_url(http: &reqwest::Client, gateway: &str) -> Option<String> {
    // Public metadata requires no device bearer token. Do not expose that token,
    // credentials or the potentially private Runner connection URL to plugins.
    let metadata: Value = http
        .get(format!("{}/api/v1/meta", gateway.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let url = reqwest::Url::parse(metadata.get("public_url")?.as_str()?).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.as_str().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::public_url;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use serde_json::json;

    #[tokio::test]
    async fn runtime_public_url_uses_public_metadata_without_device_credentials() {
        let app = Router::new().route(
            "/api/v1/meta",
            get(|headers: HeaderMap| async move {
                assert!(!headers.contains_key("authorization"));
                Json(json!({"public_url":"https://public.example.test/wakeit/"}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert_eq!(
            public_url(&reqwest::Client::new(), &gateway)
                .await
                .as_deref(),
            Some("https://public.example.test/wakeit")
        );
        server.abort();
    }

    #[tokio::test]
    async fn runtime_public_url_never_falls_back_to_connection_url_or_exposes_credentials() {
        for value in [
            json!({}),
            json!({"public_url":"https://user:password@example.test"}),
            json!({"public_url":"https://example.test/?token=private"}),
            json!({"public_url":"javascript:alert(1)"}),
        ] {
            let app = Router::new().route("/api/v1/meta", get(move || async move { Json(value) }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let gateway = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            assert_eq!(public_url(&reqwest::Client::new(), &gateway).await, None);
            server.abort();
        }
        let app = Router::new().route(
            "/api/v1/meta",
            get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let gateway = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert_eq!(public_url(&reqwest::Client::new(), &gateway).await, None);
        server.abort();
    }
}
