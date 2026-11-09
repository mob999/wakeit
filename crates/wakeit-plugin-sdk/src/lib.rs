//! Language-neutral extension protocol client and host API for native plugins.
pub mod agent;
pub mod api;
pub mod capture;
pub mod native;
use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, mpsc, oneshot, watch},
};
use uuid::Uuid;
pub use wakeit_plugin_protocol::adapter;
pub use wakeit_plugin_protocol::agent::{AgentMode, ParameterSpec};
pub use wakeit_plugin_protocol::extensions::{
    Action, Actor, Binding, ConfigUpdate, Contract, CredentialSlot, Decision, DeliveryOutcome,
    EVENT_TYPES, Event, Extension, ExtensionError, FailurePolicy, HOOK_STAGES, Hook, Instance,
    Interaction, Invocation, Provider, Receipt, RuntimeInfo, SDK_PROTOCOL, SecretUpdate, UiChoice,
    apply_decision, event_matches, schema, valid_event_pattern, validate_schema,
};
pub use wakeit_plugin_protocol::{extensions, protocol, session_catalog, sessions, trace};
pub type Handler = Arc<
    dyn Fn(Value, Context) -> Pin<Box<dyn Future<Output = Result<Value>> + Send>> + Send + Sync,
>;
#[derive(Clone)]
pub struct Context {
    pub invocation: Option<Invocation>,
    pub cancelled: watch::Receiver<bool>,
    host: Client,
}
#[derive(Clone)]
pub struct Client {
    writes: mpsc::Sender<Value>,
    pending: Arc<Mutex<HashMap<Uuid, oneshot::Sender<Result<Value>>>>>,
}
impl Client {
    pub async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T> {
        let id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.writes
            .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        let result = rx.await.context("host disconnected")??;
        Ok(serde_json::from_value(result)?)
    }
}
impl Context {
    pub async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        mut params: Value,
    ) -> Result<T> {
        if let Some(i) = &self.invocation {
            params
                .as_object_mut()
                .context("host API params must be an object")?
                .insert("invocation_id".into(), json!(i.id));
        }
        ensure!(!*self.cancelled.borrow(), "invocation cancelled");
        self.host.call(method, params).await
    }
    pub async fn config<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        self.call("config.get", json!({})).await
    }
    pub async fn save_config(&self, update: ConfigUpdate) -> Result<Receipt> {
        self.call("config.save", json!({"update":update})).await
    }
    pub async fn credential(&self, slot: &str) -> Result<Value> {
        self.call("credentials.get", json!({"slot":slot})).await
    }
    pub async fn storage_get(&self, key: &str) -> Result<Value> {
        self.call("storage.get", json!({"key":key})).await
    }
    pub async fn storage_set(
        &self,
        key: &str,
        value: Value,
        revision: Option<i64>,
    ) -> Result<Value> {
        self.call(
            "storage.set",
            json!({"key":key,"value":value,"expected_revision":revision}),
        )
        .await
    }
    pub async fn interact(&self, request: Interaction) -> Result<Value> {
        self.call("ui.request", json!({"request":request})).await
    }
    pub async fn log(&self, level: &str, message: &str, data: Value) -> Result<Value> {
        self.call(
            "log.write",
            json!({"level":level,"message":message,"data":data}),
        )
        .await
    }
}
#[derive(Default)]
pub struct Plugin {
    agents: BTreeMap<String, agent::Agent>,
    duplicate_agent: bool,
    events: BTreeMap<String, Handler>,
    hooks: BTreeMap<String, Handler>,
    actions: BTreeMap<String, Handler>,
    providers: BTreeMap<String, Handler>,
    lifecycle: BTreeMap<String, Handler>,
}
impl Plugin {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn agent(mut self, id: impl Into<String>, implementation: agent::Agent) -> Self {
        let id = id.into();
        if let std::collections::btree_map::Entry::Vacant(entry) = self.agents.entry(id) {
            entry.insert(implementation);
        } else {
            self.duplicate_agent = true;
        }
        self
    }
    pub fn on(mut self, id: impl Into<String>, handler: Handler) -> Self {
        self.events.insert(id.into(), handler);
        self
    }
    pub fn hook(mut self, id: impl Into<String>, handler: Handler) -> Self {
        self.hooks.insert(id.into(), handler);
        self
    }
    pub fn action(mut self, id: impl Into<String>, handler: Handler) -> Self {
        self.actions.insert(id.into(), handler);
        self
    }
    pub fn provider(mut self, id: impl Into<String>, handler: Handler) -> Self {
        self.providers.insert(id.into(), handler);
        self
    }
    pub fn lifecycle(mut self, id: impl Into<String>, handler: Handler) -> Self {
        self.lifecycle.insert(id.into(), handler);
        self
    }
    pub async fn serve(self) -> Result<()> {
        let (writes, mut output) = mpsc::channel::<Value>(64);
        let writer = tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            while let Some(value) = output.recv().await {
                stdout.write_all(&serde_json::to_vec(&value)?).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
            Ok::<_, anyhow::Error>(())
        });
        let client = Client {
            writes: writes.clone(),
            pending: Default::default(),
        };
        let plugin = Arc::new(self);
        let initialized = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let controllers = Arc::new(Mutex::new(HashMap::<Uuid, watch::Sender<bool>>::new()));
        let mut input = BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = input.next_line().await? {
            let message: Value = serde_json::from_str(&line)?;
            let id = message["id"].as_str().and_then(|s| s.parse::<Uuid>().ok());
            if message["method"].is_null() {
                if let Some(id) = id
                    && let Some(reply) = client.pending.lock().await.remove(&id)
                {
                    let result = if message.get("error").is_some() {
                        Err(ExtensionError {
                            code: message["error"]["data"]["code"]
                                .as_str()
                                .unwrap_or("host_error")
                                .into(),
                            message: message["error"]["message"]
                                .as_str()
                                .unwrap_or("host call failed")
                                .into(),
                            retryable: message["error"]["data"]["retryable"] == true,
                            paths: serde_json::from_value(
                                message["error"]["data"]["paths"].clone(),
                            )
                            .unwrap_or_default(),
                        }
                        .into())
                    } else {
                        Ok(message["result"].clone())
                    };
                    let _ = reply.send(result);
                }
                continue;
            }
            let method = message["method"]
                .as_str()
                .context("missing method")?
                .to_owned();
            let params = message["params"].clone();
            if method == "cancel" {
                if let Some(id) = params["id"].as_str().and_then(|s| s.parse::<Uuid>().ok())
                    && let Some(tx) = controllers.lock().await.get(&id)
                {
                    let _ = tx.send(true);
                }
                continue;
            }
            // Register cancellation before dispatching the handler task. A cancel
            // frame in the same stdin batch must not race controller creation.
            let (tx, rx) = watch::channel(false);
            let controller_id = if ["agent.invoke", "invoke"].contains(&method.as_str()) {
                params["id"]
                    .as_str()
                    .and_then(|value| value.parse::<Uuid>().ok())
            } else {
                None
            };
            if let Some(id) = controller_id {
                let mut active = controllers.lock().await;
                if active.contains_key(&id) {
                    let _ = client.writes.send(json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32000,"message":"request is already active","data":{"code":"duplicate_request","retryable":false,"paths":[]}}})).await;
                    continue;
                }
                active.insert(id, tx.clone());
            }
            let client = client.clone();
            let plugin = plugin.clone();
            let initialized = initialized.clone();
            let controllers = controllers.clone();
            tokio::spawn(async move {
                let result:Result<Value>=async{
                    if method=="initialize"{ensure!(!plugin.duplicate_agent,"Agent registered more than once");ensure!(params["protocol_version"]==1,"unsupported protocol");for (id,implementation) in &plugin.agents{wakeit_plugin_protocol::validate_id(id)?;implementation.validate()?;}ensure!(!initialized.swap(true,std::sync::atomic::Ordering::SeqCst),"duplicate initialization");return Ok(json!({"protocol_version":1,"agents":plugin.agents.iter().map(|(id, implementation)|(id,&implementation.descriptor)).collect::<BTreeMap<_,_>>(),"handlers":plugin.events.keys().collect::<Vec<_>>(),"hooks":plugin.hooks.keys().collect::<Vec<_>>(),"actions":plugin.actions.keys().collect::<Vec<_>>(),"providers":plugin.providers.keys().collect::<Vec<_>>()}));}
                    if method=="agent.invoke" {
                        ensure!(initialized.load(std::sync::atomic::Ordering::SeqCst), "initialize before invoking");
                        let request: adapter::AgentRequest = serde_json::from_value(params)?;
                        let implementation = plugin.agents.get(&request.agent).context("Agent is not registered")?;
                        Ok(serde_json::to_value(implementation.invoke(request, Context { invocation: None, cancelled: rx, host: client.clone() }, tx).await?)?)
                    } else if method=="invoke"{
                        ensure!(initialized.load(std::sync::atomic::Ordering::SeqCst),"initialize before invoking");
                        let invocation:Invocation=serde_json::from_value(params)?;
                        let table=match invocation.kind.as_str(){"event"=>&plugin.events,"hook"=>&plugin.hooks,"action"=>&plugin.actions,"provider"=>&plugin.providers,_=>anyhow::bail!("invalid invocation kind")};
                        let handler=table.get(&invocation.handler).context("handler is not registered")?;
                        let timeout=(invocation.deadline-chrono::Utc::now()).to_std().context("invocation expired")?;
                        let value=tokio::time::timeout(timeout,handler(invocation.input.clone(),Context{invocation:Some(invocation.clone()),cancelled:rx,host:client.clone()})).await.context("invocation timed out")?;
                        controllers.lock().await.remove(&invocation.id);value
                    }else if let Some(handler)=plugin.lifecycle.get(&method){handler(params,Context{invocation:None,cancelled:rx,host:client.clone()}).await}
                    else if ["activate","deactivate","dispose","health","validateConfig","applyConfig","migrateConfig"].contains(&method.as_str()){Ok(Value::Null)}else{anyhow::bail!("unknown method")}
                }.await;
                if let Some(id) = controller_id {
                    controllers.lock().await.remove(&id);
                }
                let response = match result {
                    Ok(result) => json!({"jsonrpc":"2.0","id":message["id"],"result":result}),
                    Err(e) => {
                        let error = ExtensionError::from_error(&e);
                        json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32000,"message":error.message,"data":{"code":error.code,"retryable":error.retryable,"paths":error.paths}}})
                    }
                };
                let _ = client.writes.send(response).await;
            });
        }
        for controller in controllers.lock().await.values() {
            let _ = controller.send(true);
        }
        writer.abort();
        Ok(())
    }
}
