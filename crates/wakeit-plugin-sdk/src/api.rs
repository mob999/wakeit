//! Typed entry points for the same public business APIs used by CLI and WebUI.
use crate::{Context, Receipt};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
pub use wakeit_plugin_protocol::protocol::{AgentInfo, ModelCatalog};
pub use wakeit_plugin_protocol::sessions::SessionInfo;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub revision: i64,
    /// Public version 1 task configuration JSON, validated by the host.
    pub config: Value,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: Uuid,
    pub state: String,
    /// Public version 1 task configuration JSON, validated by the host.
    pub config: Value,
    pub result: Option<Value>,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}
impl Context {
    pub async fn runtime_info(&self) -> Result<crate::RuntimeInfo> {
        self.call("runtime.info", json!({})).await
    }
    pub async fn task(&self, id: &str) -> Result<TaskRecord> {
        self.call("tasks.get", json!({"id":id})).await
    }
    /// Save public task JSON with an explicit task ID and optimistic revision.
    pub async fn save_task(
        &self,
        id: &str,
        config: Value,
        revision: Option<i64>,
        request: Uuid,
    ) -> Result<TaskRecord> {
        self.call("tasks.update",json!({"id":id,"body":{"config":config,"expected_revision":revision},"request_key":request})).await
    }
    pub async fn trigger(&self, id: &str, request: Uuid) -> Result<Value> {
        self.call("tasks.trigger", json!({"id":id,"request_key":request}))
            .await
    }
    pub async fn run(&self, id: Uuid) -> Result<RunRecord> {
        self.call("runs.get", json!({"id":id})).await
    }
    pub async fn cancel_run(&self, id: Uuid, request: Uuid) -> Result<Value> {
        self.call("runs.cancel", json!({"id":id,"request_key":request}))
            .await
    }
    /// Request observation of an unknown saved execution; never resubmit it.
    pub async fn reconcile_run(&self, id: Uuid, request: Uuid) -> Result<Value> {
        self.call("runs.reconcile", json!({"id":id,"request_key":request}))
            .await
    }
    pub async fn retry_run(&self, id: Uuid, request: Uuid) -> Result<Value> {
        self.call("runs.retry", json!({"id":id,"request_key":request}))
            .await
    }
    pub async fn models(&self, runner: Uuid, agent: &str, cwd: &str) -> Result<ModelCatalog> {
        self.call(
            "models.list",
            json!({"runner":runner,"agent":agent,"body":{"cwd":cwd}}),
        )
        .await
    }
    pub async fn operation(&self, id: Uuid) -> Result<Receipt> {
        self.call("runtime.operation", json!({"id":id})).await
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunEventRow {
    pub id: i64,
    pub seq: i64,
    pub kind: String,
    pub data: Value,
    pub created_at: Option<String>,
}
pub struct EventCursor {
    pub run_id: Uuid,
    pub after: i64,
    pub kind: Option<String>,
}
impl EventCursor {
    pub fn new(run_id: Uuid) -> Self {
        Self {
            run_id,
            after: 0,
            kind: None,
        }
    }
    pub async fn next_page(&mut self, ctx: &Context) -> Result<Vec<RunEventRow>> {
        let events: Vec<RunEventRow> = ctx
            .call(
                "runs.events",
                json!({"id":self.run_id,"query":{"after":self.after,"kind":self.kind}}),
            )
            .await?;
        if let Some(last) = events.iter().map(|e| e.id).max() {
            self.after = last;
        }
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Client;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tokio::sync::{Mutex, mpsc, watch};

    #[tokio::test]
    async fn save_task_preserves_public_json_revision_and_request_key() {
        let (writes, mut requests) = mpsc::channel(1);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let (_, cancelled) = watch::channel(false);
        let ctx = Context {
            invocation: None,
            cancelled,
            host: Client {
                writes,
                pending: pending.clone(),
            },
        };
        let config = json!({
            "version": 1, "id": "synthetic", "name": "Synthetic task",
            "runner": "fixture", "agent": "synthetic", "cwd": "/synthetic",
            "prompt": "Synthetic instructions", "parameters": {"model": "synthetic/model"},
            "session": null, "dag": null
        });
        let expected = config.clone();
        let request_key = Uuid::new_v4();
        let save = tokio::spawn(async move {
            ctx.save_task("synthetic", config, Some(42), request_key)
                .await
        });
        let request = tokio::time::timeout(Duration::from_secs(5), requests.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request["method"], "tasks.update");
        assert_eq!(
            request["params"],
            json!({
                "id": "synthetic", "body": {"config": expected, "expected_revision": 42},
                "request_key": request_key
            })
        );
        let rpc_id: Uuid = serde_json::from_value(request["id"].clone()).unwrap();
        pending
            .lock()
            .await
            .remove(&rpc_id)
            .unwrap()
            .send(Ok(json!({
                "id": "synthetic", "revision": 43, "config": expected, "enabled": true
            })))
            .unwrap();
        let saved = save.await.unwrap().unwrap();
        assert_eq!(saved.config, expected);
        assert_eq!(saved.revision, 43);
    }
}
