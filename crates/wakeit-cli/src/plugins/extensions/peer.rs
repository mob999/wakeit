//! One extension process, multiplexed stdio RPC, and owned process-tree cancellation.
use anyhow::{Context, Result, bail, ensure};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::{collections::HashMap, path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug)]
pub(super) struct RemoteError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub paths: Vec<String>,
}
impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RemoteError {}
pub trait Host: Send + Sync + 'static {
    fn request(&self, method: String, params: Value) -> BoxFuture<'static, Result<Value>>;
}
struct Call {
    id: Uuid,
    method: String,
    params: Value,
    reply: oneshot::Sender<Result<Value>>,
}
#[derive(Clone)]
pub struct Peer {
    calls: mpsc::Sender<Call>,
    stop: CancellationToken,
}
impl Peer {
    pub async fn spawn(
        command: &str,
        args: &[String],
        root: &Path,
        host: Arc<dyn Host>,
    ) -> Result<Self> {
        let executable = if command.starts_with("./") {
            root.join(command)
        } else {
            command.into()
        };
        let executable = wakeit_core::native_command::external_path(&executable);
        let external_root = wakeit_core::native_command::external_path(root);
        let mut command = Command::new(executable);
        command
            .args(
                args.iter()
                    .map(|v| v.replace("{{plugin.root}}", &external_root)),
            )
            .current_dir(&external_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Native user configuration remains accessible, but Wakeit credentials are never injected.
        for key in [
            "WAKEIT_TOKEN",
            "WAKEIT_TOKEN_FILE",
            "WAKEIT_PASSWORD",
            "WAKEIT_ENROLL_TOKEN",
        ] {
            command.env_remove(key);
        }
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().context("start extension")?;
        let group = crate::process::ProcessGroup::new(&child)?;
        let mut input = child.stdin.take().context("extension stdin")?;
        let mut output = BufReader::new(child.stdout.take().context("extension stdout")?);
        let mut stderr = BufReader::new(child.stderr.take().context("extension stderr")?);
        let stop = CancellationToken::new();
        let (calls, mut rx) = mpsc::channel::<Call>(64);
        let (writes, mut responses) = mpsc::channel::<Value>(64);
        let token = stop.clone();
        let diagnostics = host.clone();
        tokio::spawn(async move {
            let mut bytes = [0u8; 8192];
            loop {
                let Ok(count) = stderr.read(&mut bytes).await else {
                    break;
                };
                if count == 0 {
                    break;
                }
                let _=diagnostics.request("log.write".into(),json!({"level":"error","stream":"stderr","message":String::from_utf8_lossy(&bytes[..count])})).await;
            }
        });
        tokio::spawn(async move {
            let mut pending = HashMap::<Uuid, oneshot::Sender<Result<Value>>>::new();
            let result:Result<()> = async {
                let mut frame=Vec::new();
                loop {
                    let mut bounded=(&mut output).take(8*1024*1024+1-frame.len() as u64);
                    tokio::select! {
                        _=token.cancelled()=>break,
                        Some(call)=rx.recv()=>{
                            let value=json!({"jsonrpc":"2.0","id":call.id,"method":call.method,"params":call.params});
                            input.write_all(&serde_json::to_vec(&value)?).await?;input.write_all(b"\n").await?;input.flush().await?;
                            pending.insert(call.id,call.reply);
                        }
                        Some(value)=responses.recv()=>{
                            input.write_all(&serde_json::to_vec(&value)?).await?;input.write_all(b"\n").await?;input.flush().await?;
                        }
                        read=bounded.read_until(b'\n',&mut frame)=>{
                            ensure!(read?>0,"extension disconnected");
                            ensure!(frame.len()<=8*1024*1024,"extension frame exceeds 8 MiB");
                            let value:Value=serde_json::from_slice(&frame).context("invalid extension RPC")?;
                            frame.clear();
                            ensure!(value["jsonrpc"]=="2.0","invalid extension RPC version");
                            if let Some(method)=value["method"].as_str() {
                                let host=host.clone();let tx=writes.clone();let method=method.to_owned();
                                tokio::spawn(async move {
                                    let result=host.request(method,value["params"].clone()).await;
                                    if !value["id"].is_null(){let response=match result {Ok(result)=>json!({"jsonrpc":"2.0","id":value["id"],"result":result}),Err(e)=>{let error=wakeit_core::extensions::ExtensionError::from_error(&e);json!({"jsonrpc":"2.0","id":value["id"],"error":{"code":-32000,"message":error.message,"data":{"code":error.code,"retryable":error.retryable,"paths":error.paths}}})}};let _=tx.send(response).await;}
                                });
                            } else if let Some(id)=value["id"].as_str().and_then(|s|s.parse().ok()) && let Some(reply)=pending.remove(&id) {
                                let result=if value.get("error").is_some(){Err(RemoteError{code:value["error"]["data"]["code"].as_str().or(value["error"]["code"].as_str()).map(str::to_owned).unwrap_or_else(||value["error"]["code"].to_string()),message:value["error"]["message"].as_str().unwrap_or("extension request failed").into(),retryable:value["error"]["data"]["retryable"]==true,paths:serde_json::from_value(value["error"]["data"]["paths"].clone()).unwrap_or_default()}.into())}else{Ok(value["result"].clone())};
                                let _=reply.send(result);
                            }
                        }
                    }
                }
                Ok(())
            }.await;
            token.cancel();
            let message = result
                .err()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "extension stopped".into());
            for reply in pending.into_values() {
                let _ = reply.send(Err(anyhow::anyhow!(message.clone())));
            }
            group.terminate();
            drop(input);
            if tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .is_err()
            {
                group.kill();
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            let _ = group.finish().await;
        });
        Ok(Self { calls, stop })
    }
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: CancellationToken,
    ) -> Result<Value> {
        ensure!(!self.stop.is_cancelled(), "extension is not running");
        let id = Uuid::new_v4();
        let cancel_id = params["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(id);
        let (reply, result) = oneshot::channel();
        self.calls
            .send(Call {
                id,
                method: method.into(),
                params,
                reply,
            })
            .await
            .context("extension stopped")?;
        let mut result = result;
        tokio::select! {
            result=tokio::time::timeout(timeout,&mut result)=>match result {Ok(result)=>result.context("extension disconnected")?,Err(_)=>{self.cancel(cancel_id).await;self.stop();bail!("extension call timed out")}},
            _=cancel.cancelled()=>{self.cancel(cancel_id).await;if tokio::time::timeout(Duration::from_secs(2),&mut result).await.is_err(){self.stop();}bail!("extension call cancelled")}
        }
    }
    async fn cancel(&self, id: Uuid) {
        let (reply, _) = oneshot::channel();
        let _ = self
            .calls
            .send(Call {
                id: Uuid::new_v4(),
                method: "cancel".into(),
                params: json!({"id":id}),
                reply,
            })
            .await;
    }
    pub fn stop(&self) {
        self.stop.cancel();
    }
    pub fn running(&self) -> bool {
        !self.stop.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct SyntheticHost;
    impl Host for SyntheticHost {
        fn request(&self, method: String, _: Value) -> BoxFuture<'static, Result<Value>> {
            Box::pin(async move {
                match method.as_str() {
                    "config.get" => Ok(json!({"value":{"prefix":"test"},"revision":1})),
                    "credentials.get" => Ok(json!("synthetic-test")),
                    "log.write" => Ok(json!({"accepted":true})),
                    _ => anyhow::bail!("unexpected host method"),
                }
            })
        }
    }
    #[tokio::test]
    async fn typescript_peer_roundtrips_hooks_host_calls_and_cancellation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/plugins/generic")
            .canonicalize()
            .unwrap();
        let peer = Peer::spawn(
            "node",
            &["extension.ts".into()],
            &root,
            Arc::new(SyntheticHost),
        )
        .await
        .unwrap();
        let initialized = peer
            .call(
                "initialize",
                json!({"protocol_version":1}),
                Duration::from_secs(20),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(initialized["protocol_version"], 1);
        let invocation = |kind: &str, handler: &str, input: Value| json!({"id":Uuid::new_v4(),"operation_id":Uuid::new_v4(),"instance_id":Uuid::new_v4(),"kind":kind,"handler":handler,"input":input,"deadline":chrono::Utc::now()+chrono::Duration::seconds(10),"actor":{"kind":"test","id":null,"roles":[]},"expected_revision":null,"causation_id":null});
        let hook = peer
            .call(
                "invoke",
                invocation("hook", "prepare", json!({"prompt":"text"})),
                Duration::from_secs(20),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            hook,
            json!({"decision":"patch","value":{"prompt":"prepared:text"}})
        );
        let echo = peer
            .call(
                "invoke",
                invocation("action", "echo", json!({"text":"hello"})),
                Duration::from_secs(20),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(echo["settings"]["prefix"], "test");
        let cancel = CancellationToken::new();
        let task = {
            let peer = peer.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                peer.call(
                    "invoke",
                    invocation("action", "wait", json!({})),
                    Duration::from_secs(10),
                    cancel,
                )
                .await
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
        assert!(task.await.unwrap().is_err());
        peer.stop();
    }
}
