//! Typed entry points for the same public business APIs used by CLI and WebUI.
use crate::{Context, Receipt};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;
pub use wakeit_core::config::{Schedule, TaskConfig};
pub use wakeit_core::protocol::{AgentInfo, ModelCatalog};
pub use wakeit_core::sessions::SessionInfo;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: String,
    pub revision: i64,
    pub config: TaskConfig,
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: Uuid,
    pub state: String,
    pub config: TaskConfig,
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
    pub async fn task(&self, id: &str) -> Result<TaskRecord> {
        self.call("tasks.get", json!({"id":id})).await
    }
    pub async fn save_task(
        &self,
        task: TaskConfig,
        revision: Option<i64>,
        request: Uuid,
    ) -> Result<TaskRecord> {
        self.call("tasks.update",json!({"id":task.id,"body":{"config":task,"expected_revision":revision},"request_key":request})).await
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
