//! Device extension management operations.
use super::*;
impl Runtime {
    pub(super) async fn control_inner(&self, method: &str, params: Value) -> Result<Value> {
        let id = params["instance_id"]
            .as_str()
            .and_then(|s| s.parse::<Uuid>().ok());
        match method {
            "auth_callback" => {
                super::auth::callback(
                    &self.journal.pool,
                    &self.http,
                    params["state"].as_str().context("state required")?,
                    params["code"].as_str().context("code required")?,
                )
                .await
            }
            "install" | "update_package" | "remove_package" => {
                ensure!(
                    self.remote_management(),
                    "Device owner has not enabled remote package management"
                );
                let source = params["source"]
                    .as_str()
                    .context("package source required")?;
                match method {
                    "install" => crate::plugins::install(source, &[]).await,
                    "update_package" => crate::plugins::update(Some(source)).await,
                    _ => crate::plugins::remove(source),
                }
            }
            "authorize" => {
                let id = id.context("instance required")?;
                let instance = self.instance(id).await?;
                let (_, _, declaration) =
                    self.declaration(&instance.source, &instance.extension)?;
                let grants: Vec<String> = serde_json::from_value(params["grants"].clone())?;
                ensure!(
                    grants.iter().all(|g| declaration.permissions.contains(g)),
                    "undeclared capability"
                );
                store::snapshot(&self.journal.pool, id).await?;
                sqlx::query(
                    "UPDATE extension_instances SET grants=?,revision=revision+1 WHERE id=?",
                )
                .bind(json!(grants).to_string())
                .bind(id.to_string())
                .execute(&self.journal.pool)
                .await?;
                store::snapshot(&self.journal.pool, id).await?;
                Ok(json!({"state":"authorized"}))
            }
            "upgrade" => {
                let id = id.context("instance required")?;
                let current = self.instance(id).await?;
                let active:i64=sqlx::query_scalar("SELECT count(*) FROM extension_invocations WHERE instance=? AND state='running'").bind(id.to_string()).fetch_one(&self.journal.pool).await?;
                ensure!(
                    active == 0,
                    "wait for active instance calls before upgrading"
                );
                let (root, version, declaration) =
                    self.declaration(&current.source, &current.extension)?;
                ensure!(
                    declaration
                        .permissions
                        .iter()
                        .all(|g| current.grants.contains(g)),
                    "New plugin capabilities need authorization"
                );
                let mut candidate = current.clone();
                candidate.declaration = declaration.clone();
                candidate.version = version.clone();
                candidate.revision += 1;
                let peer = self.spawn_instance(&candidate, &root, false).await?;
                let validated:Result<Value>=async{
                    let migrated=peer.call("migrateConfig",json!({"from_version":current.version,"to_version":version,"config":current.config}),Duration::from_secs(30),self.stop.child_token()).await?;
                    let config=migrated.get("config").cloned().unwrap_or(current.config);
                    wakeit_core::extensions::validate_schema(&declaration.config,&config)?;
                    peer.call("validateConfig",json!({"config":config}),Duration::from_secs(20),self.stop.child_token()).await?;
                    peer.call("activate",json!({"config":config,"revision":candidate.revision}),Duration::from_secs(30),self.stop.child_token()).await?;Ok(config)
                }.await;
                let config = match validated {
                    Ok(c) => c,
                    Err(e) => {
                        peer.stop();
                        return Err(e);
                    }
                };
                store::snapshot(&self.journal.pool, id).await?;
                sqlx::query("UPDATE extension_instances SET root=?,declaration=?,version=?,config=?,revision=?,applied_revision=?,state='running',error=NULL WHERE id=? AND revision=?").bind(root.to_string_lossy().as_ref()).bind(json!(declaration).to_string()).bind(&version).bind(config.to_string()).bind(candidate.revision).bind(candidate.revision).bind(id.to_string()).bind(current.revision).execute(&self.journal.pool).await?;
                store::snapshot(&self.journal.pool, id).await?;
                if let Some(old) = self.peers.lock().await.insert(id, peer) {
                    old.stop();
                }
                Ok(json!({"state":"running","version":version}))
            }
            "reprocess_result" => {
                let run: Uuid = params["run_id"]
                    .as_str()
                    .context("Run ID required")?
                    .parse()?;
                let row=sqlx::query("SELECT id,data FROM extension_results WHERE run_id=? AND kind='native' ORDER BY created_at DESC,id DESC LIMIT 1").bind(run.to_string()).fetch_one(&self.journal.pool).await?;
                let local = self
                    .journal
                    .get(run)
                    .await?
                    .context("local execution missing")?;
                let native: Value = serde_json::from_str(row.get("data"))?;
                let processed = self
                    .hooks(
                        "result.publish.prepare",
                        json!({"run_id":run,"task_id":local.assignment.task.id,"result":native}),
                        self.stop.child_token(),
                    )
                    .await?;
                let effective = self
                    .journal
                    .extension_result(run, "effective", &processed["result"])
                    .await?;
                Ok(
                    json!({"run_id":run,"result":processed["result"],"result_ref":effective,"native_result_ref":row.get::<String,_>("id")}),
                )
            }
            "file_read" => super::files::read(self, &params).await,
            "catalog" => self.catalog().await,
            "create" => {
                let source = params["source"].as_str().context("source required")?;
                let extension = params["extension"].as_str().context("extension required")?;
                let (root, version, declaration) = self.declaration(source, extension)?;
                let grants = params["grants"].as_array().cloned().unwrap_or_default();
                ensure!(
                    grants.iter().all(|g| g
                        .as_str()
                        .is_some_and(|g| declaration.permissions.iter().any(|p| p == g))),
                    "undeclared permission grant"
                );
                let id = id.unwrap_or_else(Uuid::new_v4);
                sqlx::query("INSERT INTO extension_instances(id,source,extension,name,version,grants,config,root,declaration) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(id) DO NOTHING")
                    .bind(id.to_string()).bind(source).bind(extension).bind(params["name"].as_str().unwrap_or(&declaration.name)).bind(version).bind(json!(grants).to_string()).bind(params.get("config").cloned().unwrap_or_else(||json!({})).to_string()).bind(root.to_string_lossy().as_ref()).bind(serde_json::to_string(&declaration)?).execute(&self.journal.pool).await?;
                Ok(serde_json::to_value(self.instance(id).await?)?)
            }
            "configure" => {
                let id = id.context("instance required")?;
                let instance = self.instance(id).await?;
                let update: ConfigUpdate = serde_json::from_value(params["update"].clone())?;
                wakeit_core::extensions::validate_schema(
                    &instance.declaration.config,
                    &update.config,
                )?;
                let existing = self
                    .peers
                    .lock()
                    .await
                    .get(&id)
                    .cloned()
                    .filter(Peer::running);
                if let Some(peer) = existing {
                    peer.call(
                        "validateConfig",
                        json!({"config":update.config}),
                        Duration::from_secs(20),
                        self.stop.child_token(),
                    )
                    .await?;
                } else {
                    let root: String =
                        sqlx::query_scalar("SELECT root FROM extension_instances WHERE id=?")
                            .bind(id.to_string())
                            .fetch_one(&self.journal.pool)
                            .await?;
                    let candidate = self
                        .spawn_instance(&instance, std::path::Path::new(&root), false)
                        .await?;
                    let result = candidate
                        .call(
                            "validateConfig",
                            json!({"config":update.config}),
                            Duration::from_secs(20),
                            self.stop.child_token(),
                        )
                        .await;
                    candidate.stop();
                    result?;
                }
                let result =
                    store::save(&self.journal.pool, id, update, &instance.declaration).await?;
                let active:i64=sqlx::query_scalar("SELECT count(*) FROM extension_invocations WHERE instance=? AND state='running'").bind(id.to_string()).fetch_one(&self.journal.pool).await?;
                if instance.enabled && active == 0 {
                    self.reload(id).await?;
                }
                Ok(result)
            }
            "enable" | "disable" => {
                let id = id.context("instance required")?;
                if method == "enable" {
                    let instance = self.instance(id).await?;
                    wakeit_core::extensions::validate_schema(
                        &instance.declaration.config,
                        &instance.config,
                    )?;
                    ensure!(
                        instance
                            .declaration
                            .credentials
                            .iter()
                            .all(|(id, slot)| !slot.required
                                || instance.credential_status.get(id) == Some(&true)),
                        "required credentials are not configured"
                    );
                    sqlx::query("UPDATE extension_instances SET enabled=1,state='starting',revision=revision+1,restart_attempts=0,next_start=NULL WHERE id=?").bind(id.to_string()).execute(&self.journal.pool).await?;
                    self.peer(id).await?;
                } else {
                    sqlx::query("UPDATE extension_instances SET enabled=0,state='disabled',revision=revision+1 WHERE id=?").bind(id.to_string()).execute(&self.journal.pool).await?;
                    if let Some(peer) = self.peers.lock().await.remove(&id) {
                        let _ = peer
                            .call(
                                "deactivate",
                                json!({}),
                                Duration::from_secs(5),
                                CancellationToken::new(),
                            )
                            .await;
                        peer.stop();
                    }
                }
                Ok(serde_json::to_value(self.instance(id).await?)?)
            }
            "invoke" => {
                let mut invocation: Invocation =
                    serde_json::from_value(params["invocation"].clone())?;
                if invocation.kind == "action" {
                    let value=self.hooks("action.prepare",json!({"input":invocation.input,"instance_id":invocation.instance_id,"handler":invocation.handler}),self.stop.child_token()).await?;
                    invocation.input = value["input"].clone();
                }
                self.invoke(invocation).await
            }
            "cancel" => {
                let call: Uuid = params["invocation_id"]
                    .as_str()
                    .context("invocation required")?
                    .parse()?;
                if let Some(token) = self.calls.lock().await.get(&call) {
                    token.cancel();
                }
                Ok(json!({"state":"cancellation_requested"}))
            }
            "interaction" => {
                let interaction = params["id"].as_str().context("interaction ID required")?;
                let owner = id.context("instance required")?;
                let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM extension_interactions WHERE id=? AND instance=? AND state='waiting')").bind(interaction).bind(owner.to_string()).fetch_one(&self.journal.pool).await?;
                ensure!(exists, "interaction expired or already answered");
                let raw: String = sqlx::query_scalar(
                    "SELECT request FROM extension_interactions WHERE id=? AND instance=?",
                )
                .bind(interaction)
                .bind(owner.to_string())
                .fetch_one(&self.journal.pool)
                .await?;
                let request: wakeit_core::extensions::Interaction = serde_json::from_str(&raw)?;
                match &request {
                    wakeit_core::extensions::Interaction::Form { schema, .. } => {
                        wakeit_core::extensions::validate_schema(schema, &params["response"])?
                    }
                    wakeit_core::extensions::Interaction::Choice { options, .. } => ensure!(
                        params["response"]
                            .as_str()
                            .is_some_and(|v| options.iter().any(|o| o.value == v)),
                        "invalid interaction choice"
                    ),
                    wakeit_core::extensions::Interaction::Confirm { .. } => ensure!(
                        params["response"].is_boolean(),
                        "confirmation response must be a boolean"
                    ),
                    _ => {}
                }
                self.responses
                    .lock()
                    .await
                    .insert(interaction.to_owned(), params["response"].clone());
                sqlx::query(
                    "UPDATE extension_interactions SET state='answered' WHERE id=? AND instance=?",
                )
                .bind(interaction)
                .bind(owner.to_string())
                .execute(&self.journal.pool)
                .await?;
                Ok(json!({"state":"answered"}))
            }
            "activity" => {
                let rows=sqlx::query("SELECT id,kind,handler,state,error,created_at,finished_at FROM extension_invocations WHERE instance=? ORDER BY created_at DESC LIMIT 100").bind(id.context("instance required")?.to_string()).fetch_all(&self.journal.pool).await?;
                let interactions=sqlx::query("SELECT id,invocation,request,state FROM extension_interactions WHERE instance=? AND state='waiting'").bind(id.unwrap().to_string()).fetch_all(&self.journal.pool).await?;
                let notifications=sqlx::query("SELECT id,invocation,request FROM extension_interactions WHERE instance=? AND state='notification' ORDER BY rowid DESC LIMIT 30").bind(id.unwrap().to_string()).fetch_all(&self.journal.pool).await?;
                Ok(
                    json!({"items":rows.iter().map(|r|json!({"id":r.get::<String,_>("id"),"kind":r.get::<String,_>("kind"),"handler":r.get::<String,_>("handler"),"state":r.get::<String,_>("state"),"error":r.get::<Option<String>,_>("error"),"created_at":r.get::<String,_>("created_at"),"finished_at":r.get::<Option<String>,_>("finished_at")})).collect::<Vec<_>>(),"notifications":notifications.iter().map(|r|json!({"id":r.get::<String,_>("id"),"invocation_id":r.get::<String,_>("invocation"),"request":serde_json::from_str::<Value>(r.get("request")).unwrap_or(Value::Null)})).collect::<Vec<_>>(),"interactions":interactions.iter().map(|r|json!({"id":r.get::<String,_>("id"),"invocation_id":r.get::<String,_>("invocation"),"request":serde_json::from_str::<Value>(r.get("request")).unwrap_or(Value::Null)})).collect::<Vec<_>>()}),
                )
            }
            "reload" => {
                sqlx::query(
                    "UPDATE extension_instances SET restart_attempts=0,next_start=NULL WHERE id=?",
                )
                .bind(id.context("instance required")?.to_string())
                .execute(&self.journal.pool)
                .await?;
                self.reload(id.context("instance required")?).await?;
                Ok(json!({"state":"running"}))
            }
            "remove" => {
                let id = id.context("instance required")?;
                let instance = self.instance(id).await?;
                ensure!(!instance.enabled, "disable extension before removal");
                sqlx::query("UPDATE extension_instances SET removed=1,state='removed' WHERE id=?")
                    .bind(id.to_string())
                    .execute(&self.journal.pool)
                    .await?;
                if params["clear_data"] == true {
                    let mut tx = self.journal.pool.begin().await?;
                    for table in [
                        "extension_storage",
                        "extension_invocations",
                        "extension_operations",
                        "extension_interactions",
                    ] {
                        sqlx::query(&format!("DELETE FROM {table} WHERE instance=?"))
                            .bind(id.to_string())
                            .execute(&mut *tx)
                            .await?;
                    }
                    sqlx::query("DELETE FROM extension_instances WHERE id=?")
                        .bind(id.to_string())
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                }
                Ok(json!({"state":"removed","data_retained":params["clear_data"]!=true}))
            }
            _ => bail!("unsupported extension control operation"),
        }
    }
}
