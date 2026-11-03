//! Device-scoped extension instances. Process ownership, settings and invocations live with Runner.
use self::host::InstanceHost;
use self::peer::Peer;
use self::storage as store;
use super::manifest;
mod auth;
mod host;
mod management;
mod peer;
mod storage;
use crate::runner::{config::RunnerConfig, journal::Journal};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use sqlx::Row;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use wakeit_core::extensions::{ConfigUpdate, Decision, Extension, Instance, Invocation};

#[derive(Clone)]
pub struct Runtime {
    pub config: Arc<RunnerConfig>,
    pub journal: Journal,
    peers: Arc<Mutex<HashMap<Uuid, Peer>>>,
    calls: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
    responses: Arc<Mutex<HashMap<String, Value>>>,
    http: reqwest::Client,
    token: Arc<String>,
    stop: CancellationToken,
}
impl Runtime {
    pub async fn new(
        config: Arc<RunnerConfig>,
        journal: Journal,
        stop: CancellationToken,
    ) -> Result<Self> {
        store::initialize(&journal.pool).await?;
        self::auth::initialize(&journal.pool).await?;
        Ok(Self {
            responses: Default::default(),
            token: Arc::new(std::fs::read_to_string(&config.token_file)?),
            http: reqwest::Client::new(),
            config,
            journal,
            peers: Default::default(),
            calls: Default::default(),
            stop,
        })
    }
    fn declaration(&self, source: &str, id: &str) -> Result<(PathBuf, String, Extension)> {
        let identity = if source.starts_with("local:") {
            source.to_owned()
        } else {
            super::source::Source::parse(source, &std::env::current_dir()?)?.identity()
        };
        let package = super::packages(&super::registry()?, self.config.bundled_agents)?
            .into_iter()
            .find(|p| p.source.identity() == identity)
            .context("plugin is not installed")?;
        let manifest = manifest::read(&package.root)?;
        let declaration = manifest
            .wakeit
            .extensions
            .get(id)
            .context("extension resource not found")?
            .clone();
        Ok((package.root, manifest.version, declaration))
    }
    pub async fn catalog(&self) -> Result<Value> {
        let mut packages = Vec::new();
        for p in super::packages(&super::registry()?, self.config.bundled_agents)? {
            match manifest::read(&p.root){Ok(m)=>packages.push(json!({"source":p.source.identity(),"name":m.name,"version":m.version,"extensions":m.wakeit.extensions,"agents":m.wakeit.agents.keys().collect::<Vec<_>>(),"commands":m.wakeit.commands.keys().collect::<Vec<_>>() })),Err(e)=>packages.push(json!({"source":p.source.identity(),"error":e.to_string()}))}
        }
        let rows =
            sqlx::query("SELECT id,source,extension FROM extension_instances ORDER BY name,id")
                .fetch_all(&self.journal.pool)
                .await?;
        let mut instances = Vec::new();
        for row in rows {
            let id: Uuid = row.get::<String, _>("id").parse()?;
            match self.instance(id).await {
                Ok(i) => instances.push(serde_json::to_value(i)?),
                Err(e) => {
                    instances.push(json!({"id":id,"state":"unavailable","error":e.to_string()}))
                }
            }
        }
        Ok(
            json!({"packages":packages,"instances":instances,"sdk_protocol":1,"runner_id":self.config.id,"remote_management":self.remote_management()}),
        )
    }
    async fn instance(&self, id: Uuid) -> Result<Instance> {
        let definition: String =
            sqlx::query_scalar("SELECT declaration FROM extension_instances WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&self.journal.pool)
                .await?;
        store::get(
            &self.journal.pool,
            id,
            self.config.id,
            serde_json::from_str(&definition)?,
        )
        .await
    }
    async fn spawn_instance(
        &self,
        instance: &Instance,
        root: &std::path::Path,
        activate: bool,
    ) -> Result<Peer> {
        let declaration = &instance.declaration;
        let peer = Peer::spawn(
            &declaration.command,
            &declaration.args,
            root,
            Arc::new(InstanceHost {
                runtime: self.clone(),
                instance: instance.id,
                snapshot: instance.clone(),
            }),
        )
        .await?;
        let initialized=peer.call("initialize",json!({"protocol_version":1,"instance":instance,"host":{"version":env!("CARGO_PKG_VERSION"),"runner_id":self.config.id},"declaration":declaration}),Duration::from_secs(20),self.stop.child_token()).await;
        let result: Result<()> = async {
            let setup = initialized?;
            ensure!(
                setup["protocol_version"] == 1,
                "extension protocol negotiation failed"
            );
            for (key, declared) in [
                ("handlers", declaration.handlers.keys().collect::<Vec<_>>()),
                ("hooks", declaration.hooks.keys().collect::<Vec<_>>()),
                ("actions", declaration.actions.keys().collect::<Vec<_>>()),
                (
                    "providers",
                    declaration.providers.keys().collect::<Vec<_>>(),
                ),
            ] {
                let registered = setup[key]
                    .as_array()
                    .context("extension registration missing")?;
                ensure!(
                    registered.len() == declared.len()
                        && registered.iter().all(|v| v
                            .as_str()
                            .is_some_and(|id| declared.iter().any(|s| s.as_str() == id))),
                    "extension registrations differ from manifest: {key}"
                );
            }
            if activate {
                peer.call(
                    "activate",
                    json!({"config":instance.config,"revision":instance.revision}),
                    Duration::from_secs(30),
                    self.stop.child_token(),
                )
                .await?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            peer.stop();
            return Err(error);
        }
        Ok(peer)
    }
    async fn peer(&self, id: Uuid) -> Result<Peer> {
        let mut peers = self.peers.lock().await;
        if let Some(peer) = peers.get(&id)
            && peer.running()
        {
            return Ok(peer.clone());
        }
        let instance = self.instance(id).await?;
        ensure!(instance.enabled, "extension disabled");
        store::snapshot(&self.journal.pool, id).await?;
        let root: String = sqlx::query_scalar("SELECT root FROM extension_instances WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&self.journal.pool)
            .await?;
        let peer = self
            .spawn_instance(&instance, std::path::Path::new(&root), true)
            .await?;
        sqlx::query("UPDATE extension_instances SET state='running',applied_revision=revision,error=NULL WHERE id=?").bind(id.to_string()).execute(&self.journal.pool).await?;
        peers.insert(id, peer.clone());
        Ok(peer)
    }
    fn remote_management(&self) -> bool {
        std::fs::read(self.config.data_dir.join("plugins-policy.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|p| p["remote_package_management"] == true)
    }
    async fn redact(&self, id: Uuid, message: &str) -> String {
        let raw =
            sqlx::query_scalar::<_, String>("SELECT secrets FROM extension_instances WHERE id=?")
                .bind(id.to_string())
                .fetch_optional(&self.journal.pool)
                .await
                .ok()
                .flatten();
        let mut text = message.to_owned();
        if let Some(raw) = raw
            && let Ok(secrets) = serde_json::from_str::<Value>(&raw)
        {
            fn values(value: &Value, out: &mut Vec<String>) {
                match value {
                    Value::String(s) => {
                        if !s.is_empty() {
                            out.push(s.clone());
                            if let Ok(v) = serde_json::from_str::<Value>(s) {
                                values(&v, out);
                            }
                        }
                    }
                    Value::Object(o) => {
                        for value in o.values() {
                            values(value, out)
                        }
                    }
                    Value::Array(a) => {
                        for value in a {
                            values(value, out)
                        }
                    }
                    _ => {}
                }
            }
            let mut secret_values = Vec::new();
            values(&secrets, &mut secret_values);
            secret_values.sort_by_key(|s| std::cmp::Reverse(s.len()));
            for value in secret_values {
                text = text.replace(&value, "[redacted]");
            }
        }
        text
    }
    pub async fn control(&self, method: &str, params: Value) -> Result<Value> {
        let id = params["instance_id"]
            .as_str()
            .or(params["invocation"]["instance_id"].as_str())
            .and_then(|s| s.parse::<Uuid>().ok());
        match self.control_inner(method, params).await {
            Ok(v) => Ok(v),
            Err(e) => {
                if let Some(id) = id {
                    Err(anyhow::anyhow!(self.redact(id, &e.to_string()).await))
                } else {
                    Err(e)
                }
            }
        }
    }
    async fn reload(&self, id: Uuid) -> Result<()> {
        if let Some(peer) = self.peers.lock().await.remove(&id) {
            let _ = peer
                .call(
                    "dispose",
                    json!({}),
                    Duration::from_secs(3),
                    CancellationToken::new(),
                )
                .await;
            peer.stop();
        }
        self.peer(id).await?;
        Ok(())
    }
    pub async fn invoke(&self, invocation: Invocation) -> Result<Value> {
        self.invoke_cancelled(invocation, self.stop.child_token())
            .await
    }
    async fn invoke_cancelled(
        &self,
        invocation: Invocation,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let mut instance = self.instance(invocation.instance_id).await?;
        let mut retired_root = None;
        if let Some(revision) = invocation.expected_revision
            && revision != instance.revision
        {
            let snapshot: Option<String> = sqlx::query_scalar(
                "SELECT snapshot FROM extension_versions WHERE instance=? AND revision=?",
            )
            .bind(instance.id.to_string())
            .bind(revision)
            .fetch_optional(&self.journal.pool)
            .await?;
            let snapshot: Value = serde_json::from_str(
                &snapshot.context("invocation configuration version is unavailable")?,
            )?;
            instance.config = snapshot["config"].clone();
            instance.declaration = serde_json::from_value(snapshot["declaration"].clone())?;
            instance.version = snapshot["version"]
                .as_str()
                .context("version missing")?
                .into();
            instance.revision = revision;
            retired_root = Some(PathBuf::from(
                snapshot["root"].as_str().context("version root missing")?,
            ));
        }
        if let Some(row) =
            sqlx::query("SELECT state,input,result,error FROM extension_invocations WHERE id=?")
                .bind(invocation.id.to_string())
                .fetch_optional(&self.journal.pool)
                .await?
        {
            ensure!(
                serde_json::from_str::<Value>(row.get("input"))? == invocation.input,
                "invocation ID reused with different input"
            );
            if row.get::<String, _>("state") == "succeeded" {
                return Ok(serde_json::from_str(row.get("result"))?);
            }
            if row.get::<String, _>("state") == "failed" && invocation.kind == "event" {
                sqlx::query("DELETE FROM extension_invocations WHERE id=? AND state='failed'")
                    .bind(invocation.id.to_string())
                    .execute(&self.journal.pool)
                    .await?;
            } else {
                bail!(
                    "invocation {}: {}",
                    row.get::<String, _>("state"),
                    row.get::<Option<String>, _>("error").unwrap_or_default()
                );
            }
        }
        if let Some(revision) = invocation.expected_revision {
            ensure!(
                instance.revision == revision,
                "instance configuration changed"
            );
        }
        let timeout = (invocation.deadline - Utc::now())
            .to_std()
            .context("invocation expired")?;
        match invocation.kind.as_str() {
            "event" => {
                ensure!(
                    instance
                        .declaration
                        .handlers
                        .contains_key(&invocation.handler),
                    "handler is not declared"
                );
            }
            "hook" => {
                ensure!(
                    instance.declaration.hooks.contains_key(&invocation.handler),
                    "hook is not declared"
                );
            }
            "action" => {
                let action = instance
                    .declaration
                    .actions
                    .get(&invocation.handler)
                    .context("action is not declared")?;
                wakeit_core::extensions::validate_schema(&action.input, &invocation.input)?;
            }
            "provider" => {
                ensure!(
                    instance
                        .declaration
                        .providers
                        .contains_key(&invocation.handler),
                    "provider is not declared"
                );
            }
            _ => bail!("unsupported invocation kind"),
        }
        if retired_root.is_none() && instance.state == "pending" {
            let root: String =
                sqlx::query_scalar("SELECT root FROM extension_instances WHERE id=?")
                    .bind(instance.id.to_string())
                    .fetch_one(&self.journal.pool)
                    .await?;
            retired_root = Some(PathBuf::from(root));
        }
        let peer = if let Some(root) = &retired_root {
            self.spawn_instance(&instance, root, true).await?
        } else {
            self.peer(instance.id).await?
        };
        sqlx::query("INSERT INTO extension_invocations(id,instance,kind,handler,input,state) VALUES(?,?,?,?,?,'running')").bind(invocation.id.to_string()).bind(instance.id.to_string()).bind(&invocation.kind).bind(&invocation.handler).bind(invocation.input.to_string()).execute(&self.journal.pool).await?;
        self.calls
            .lock()
            .await
            .insert(invocation.id, cancel.clone());
        let result = peer
            .call(
                "invoke",
                serde_json::to_value(&invocation)?,
                timeout,
                cancel.clone(),
            )
            .await;
        cancel.cancel();
        if retired_root.is_some() {
            peer.stop();
        }
        self.calls.lock().await.remove(&invocation.id);
        let (state, value, error) = match &result {
            Ok(value) => ("succeeded", Some(value.to_string()), None),
            Err(error) => (
                "failed",
                None,
                Some(self.redact(instance.id, &error.to_string()).await),
            ),
        };
        sqlx::query("UPDATE extension_invocations SET state=?,result=?,error=?,finished_at=CURRENT_TIMESTAMP WHERE id=?").bind(state).bind(value).bind(error).bind(invocation.id.to_string()).execute(&self.journal.pool).await?;
        result
    }
    pub async fn hooks(
        &self,
        stage: &str,
        mut proposal: Value,
        cancel: CancellationToken,
    ) -> Result<Value> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT binding FROM extension_bindings ORDER BY position,id")
                .fetch_all(&self.journal.pool)
                .await?;
        for row in rows {
            let binding: wakeit_core::extensions::Binding = serde_json::from_str(&row)?;
            if !binding.enabled || binding.kind != "hook" || binding.event != stage {
                continue;
            }
            let instance = self.instance(binding.instance_id).await?;
            if !instance.enabled {
                continue;
            }
            ensure!(
                instance.grants.contains(&format!("hooks:{stage}")),
                "hook stage is not authorized"
            );
            let invocation = Invocation {
                id: Uuid::new_v4(),
                operation_id: Uuid::new_v4(),
                instance_id: instance.id,
                kind: "hook".into(),
                handler: binding.handler.clone(),
                input: proposal.clone(),
                deadline: Utc::now() + chrono::Duration::seconds(binding.timeout_seconds as i64),
                expected_revision: Some(instance.revision),
                actor: wakeit_core::extensions::Actor {
                    kind: "runner".into(),
                    id: Some(self.config.id.to_string()),
                    roles: vec![],
                },
                causation_id: None,
            };
            let result = self
                .invoke_cancelled(invocation, cancel.child_token())
                .await;
            match result {
                Ok(value) => wakeit_core::extensions::apply_decision(
                    stage,
                    &mut proposal,
                    serde_json::from_value::<Decision>(value)?,
                )?,
                Err(_)
                    if matches!(
                        binding.failure_policy,
                        wakeit_core::extensions::FailurePolicy::Continue
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(proposal)
    }
    async fn remote(&self, instance: Uuid, method: &str, params: Value) -> Result<Value> {
        let response = self
            .http
            .post(format!(
                "{}/api/v1/device/plugins/api",
                self.config.gateway.trim_end_matches('/')
            ))
            .bearer_auth(self.token.trim())
            .json(&json!({"instance_id":instance,"method":method,"params":params}))
            .send()
            .await?;
        let status = response.status();
        let result: Value = response.json().await?;
        ensure!(status.is_success(), "host API failed: {}", result["error"]);
        Ok(result)
    }
    pub async fn serve(self, client: async_nats::Client, runtime: Uuid) -> Result<()> {
        use futures::StreamExt;
        let subject = format!("wakeit.v1.runner.{}.plugins", self.config.id);
        let mut sub = client.subscribe(subject).await?;
        let tick = self.clone();
        tokio::spawn(async move {
            let _ = tick.worker().await;
        });
        while let Some(msg) = tokio::select! {_=self.stop.cancelled()=>None,msg=sub.next()=>msg} {
            let Some(reply) = msg.reply else { continue };
            if !reply
                .as_str()
                .starts_with(&(crate::protocol::reply_prefix(self.config.id) + "."))
            {
                continue;
            }
            let request: crate::protocol::QueryRequest = match serde_json::from_slice(&msg.payload)
            {
                Ok(v) => v,
                Err(_) => continue,
            };
            if request.runtime_id != runtime {
                continue;
            }
            let this = self.clone();
            let client = client.clone();
            tokio::spawn(async move {
                let result = this.control(&request.action, request.payload).await;
                let (data, error) = match result {
                    Ok(v) => (Some(v), None),
                    Err(e) => (
                        None,
                        Some(crate::protocol::QueryError {
                            code: "plugin_error".into(),
                            message: e.to_string(),
                        }),
                    ),
                };
                let reply_value = crate::protocol::QueryReply {
                    version: 1,
                    id: request.id,
                    runtime_id: runtime,
                    data,
                    error,
                };
                if let Ok(bytes) = serde_json::to_vec(&reply_value) {
                    let _ = client.publish(reply, bytes.into()).await;
                }
            });
        }
        for peer in self.peers.lock().await.values() {
            peer.stop();
        }
        Ok(())
    }
    async fn worker(&self) -> Result<()> {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {_=self.stop.cancelled()=>break,_=tick.tick()=>{}}
            let events=sqlx::query("SELECT id,event_type,instance,data,created_at FROM extension_events WHERE published=0 AND event_type NOT IN ('log.write','health.report') ORDER BY created_at,id LIMIT 50").fetch_all(&self.journal.pool).await?;
            if !events.is_empty() {
                let payload = json!({"items":events.iter().map(|r|json!({"id":r.get::<String,_>("id"),"event_type":r.get::<String,_>("event_type"),"instance_id":r.get::<Option<String>,_>("instance"),"data":serde_json::from_str::<Value>(r.get("data")).unwrap_or(Value::Null)})).collect::<Vec<_>>()});
                if self
                    .http
                    .post(format!(
                        "{}/api/v1/device/plugins/events",
                        self.config.gateway.trim_end_matches('/')
                    ))
                    .bearer_auth(self.token.trim())
                    .json(&payload)
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    for event in events {
                        sqlx::query("UPDATE extension_events SET published=1 WHERE id=?")
                            .bind(event.get::<String, _>("id"))
                            .execute(&self.journal.pool)
                            .await?;
                    }
                }
            }
            let enabled: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM extension_instances WHERE enabled=1 AND state!='failed'",
            )
            .fetch_all(&self.journal.pool)
            .await?;
            for id in enabled {
                let id: Uuid = id.parse()?;
                let pending:bool=sqlx::query_scalar("SELECT state='pending' AND NOT EXISTS(SELECT 1 FROM extension_invocations c WHERE c.instance=extension_instances.id AND c.state='running') FROM extension_instances WHERE id=?").bind(id.to_string()).fetch_one(&self.journal.pool).await?;
                let result = if pending {
                    self.reload(id).await.map(|_| ())
                } else {
                    self.peer(id).await.map(|_| ())
                };
                if let Err(error) = result {
                    sqlx::query("UPDATE extension_instances SET state='failed',error=? WHERE id=?")
                        .bind(error.to_string())
                        .bind(id.to_string())
                        .execute(&self.journal.pool)
                        .await?;
                }
            }
            let catalog = self.catalog().await?;
            let response = self
                .http
                .post(format!(
                    "{}/api/v1/device/plugins/poll",
                    self.config.gateway.trim_end_matches('/')
                ))
                .bearer_auth(self.token.trim())
                .json(&catalog)
                .send()
                .await;
            let Ok(response) = response else { continue };
            if !response.status().is_success() {
                continue;
            }
            let Ok(commands) = response.json::<Value>().await else {
                continue;
            };
            if let Some(bindings) = commands["bindings"].as_array() {
                let mut tx = self.journal.pool.begin().await?;
                sqlx::query("DELETE FROM extension_bindings")
                    .execute(&mut *tx)
                    .await?;
                for binding in bindings {
                    sqlx::query(
                        "INSERT INTO extension_bindings(id,position,binding) VALUES(?,?,?)",
                    )
                    .bind(binding["id"].as_str().context("binding id")?)
                    .bind(binding["position"].as_i64().unwrap_or(0))
                    .bind(binding.to_string())
                    .execute(&mut *tx)
                    .await?;
                }
                tx.commit().await?;
            }
            for command in commands["items"].as_array().into_iter().flatten() {
                let command = command.clone();
                let this = self.clone();
                tokio::spawn(async move {
                    let result = this
                        .control(
                            command["action"].as_str().unwrap_or("invoke"),
                            command["params"].clone(),
                        )
                        .await;
                    let body = match result {
                        Ok(v) => json!({"id":command["id"],"state":"succeeded","result":v}),
                        Err(e) => {
                            json!({"id":command["id"],"state":"failed","error":e.to_string()})
                        }
                    };
                    let _ = this
                        .http
                        .post(format!(
                            "{}/api/v1/device/plugins/ack",
                            this.config.gateway.trim_end_matches('/')
                        ))
                        .bearer_auth(this.token.trim())
                        .json(&body)
                        .send()
                        .await;
                });
            }
        }
        Ok(())
    }
}
use chrono::Utc;
