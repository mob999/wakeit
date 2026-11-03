//! Host APIs exposed to an individual extension instance.
use super::peer::Host;
use super::*;
use futures::future::BoxFuture;
pub(super) struct InstanceHost {
    pub(super) runtime: Runtime,
    pub(super) instance: Uuid,
    pub(super) snapshot: Instance,
}
impl Host for InstanceHost {
    fn request(&self, method: String, params: Value) -> BoxFuture<'static, Result<Value>> {
        let runtime = self.runtime.clone();
        let id = self.instance;
        let snapshot = self.snapshot.clone();
        Box::pin(async move {
            let mut instance = snapshot;
            let current = runtime.instance(id).await?;
            ensure!(
                current.enabled
                    || ["config.get", "credentials.status", "runtime.info"]
                        .contains(&method.as_str()),
                "extension was disabled"
            );
            instance.grants = current.grants.clone();
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
            let in_hook = if let Some(invocation) = invocation {
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
            if method.starts_with("storage.") {
                ensure!(
                    !in_hook || ["storage.get", "storage.list"].contains(&method.as_str()),
                    "Hook context cannot modify storage"
                );
                return store::storage(&runtime.journal.pool, id, &method, &params).await;
            }
            match method.as_str() {
                "runtime.info" => Ok(
                    json!({"runner_id":runtime.config.id,"instance_id":id,"sdk_protocol":1,"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"invocation_id":invocation}),
                ),
                "providers.query" => {
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
                        "Hook HTTP access must be read-only"
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
                    let signal = if let Some(call) = invocation.and_then(|s| s.parse::<Uuid>().ok())
                    {
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
                    let output = crate::process::execute(
                        &spec,
                        &task,
                        Uuid::new_v4(),
                        None,
                        Duration::from_secs(
                            params["timeout_seconds"].as_u64().unwrap_or(60).min(3600),
                        ),
                        signal,
                        None,
                    )
                    .await?;
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
                        ensure!(!in_hook, "Hook cannot revoke credentials");
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
                    ensure!(!in_hook, "Hook context cannot modify configuration");
                    let update: ConfigUpdate = serde_json::from_value(params["update"].clone())?;
                    store::save(&runtime.journal.pool, id, update, &instance.declaration).await
                }
                "credentials.set" | "credentials.remove" => {
                    ensure!(!in_hook, "Hook context cannot modify credentials");
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
                    if method == "health.report" {
                        sqlx::query("UPDATE extension_instances SET error=? WHERE id=?")
                            .bind(data["message"].as_str())
                            .bind(id.to_string())
                            .execute(&runtime.journal.pool)
                            .await?;
                    }
                    let mut text = data.to_string();
                    let secrets: String =
                        sqlx::query_scalar("SELECT secrets FROM extension_instances WHERE id=?")
                            .bind(id.to_string())
                            .fetch_one(&runtime.journal.pool)
                            .await?;
                    let secrets: Value = serde_json::from_str(&secrets)?;
                    for secret in secrets
                        .as_object()
                        .into_iter()
                        .flat_map(|o| o.values())
                        .filter_map(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        text = text.replace(secret, "[redacted]");
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
                            "Hook context only permits read APIs"
                        );
                    }
                    runtime.remote(id, &method, params).await
                }
            }
        })
    }
}
