//! Synthetic native extension for language-independent protocol acceptance.
use serde_json::{Value, json};
use std::sync::Arc;
use wakeit_plugin_sdk::{Interaction, Plugin};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    Plugin::new()
        .hook("prepare",Arc::new(|input,_|Box::pin(async move{Ok(json!({"decision":"patch","value":{"prompt":format!("prepared:{}",input["prompt"].as_str().unwrap_or(""))}}))})))
        .hook("save",Arc::new(|input,_|Box::pin(async move{Ok(if input["config"]["name"]=="reject"{json!({"decision":"deny","code":"synthetic_rejection","message":"Synthetic definition rejected"})}else{json!({"decision":"allow"})})})))
        .action("echo",Arc::new(|input,ctx|Box::pin(async move{let config:Value=ctx.config().await?;Ok(json!({"input":input,"settings":config["value"]}))})))
        .action("wait",Arc::new(|_,mut ctx|Box::pin(async move{let _=ctx.cancelled.changed().await;anyhow::bail!("cancelled")})))
        .action("interact",Arc::new(|_,ctx|Box::pin(async move{let answer=ctx.interact(Interaction::Form{title:"Synthetic form".into(),schema:json!({"type":"object","properties":{"answer":{"type":"string"}}}),value:Value::Null}).await?;Ok(json!({"answer":answer}))})))
        .provider("fixture-models",Arc::new(|_,_|Box::pin(async move{Ok(json!({"version":1,"models":[{"id":"synthetic","name":"Synthetic model","parameters":{"model":"synthetic"},"parameter_options":{}}]}))})))
        .on("completed",Arc::new(|input,ctx|Box::pin(async move{let previous=ctx.storage_get("completed").await?;let count=previous["value"].as_i64().unwrap_or(0)+1;ctx.storage_set("completed",json!(count),Some(previous["revision"].as_i64().unwrap_or(0))).await?;Ok(json!({"event":input["id"],"handled":true}))})))
        .serve().await
}
