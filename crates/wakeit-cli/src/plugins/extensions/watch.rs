//! Explicit, bounded file subscriptions. No implicit monitoring outside allowed roots.
use super::*;
use notify::{RecursiveMode, Watcher};
pub(super) struct Watch {
    pub instance: Uuid,
    _watcher: notify::RecommendedWatcher,
}
pub(super) async fn start(runtime: &Runtime, instance: Uuid, params: &Value) -> Result<Value> {
    let path =
        PathBuf::from(params["path"].as_str().context("watch path required")?).canonicalize()?;
    crate::process::validate_cwd(
        path.to_str().context("invalid watch path")?,
        &runtime.config.allowed_roots,
    )?;
    let recursive = params["recursive"] == true;
    let id = Uuid::new_v4();
    let (tx, mut events) = tokio::sync::mpsc::channel(256);
    let overflow = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = overflow.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if tx.try_send(event).is_err() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    })?;
    watcher.watch(
        &path,
        if recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        },
    )?;
    runtime.watches.lock().await.insert(
        id,
        Watch {
            instance,
            _watcher: watcher,
        },
    );
    let runtime = runtime.clone();
    let stop = runtime.stop.clone();
    let observed = path.clone();
    tokio::spawn(async move {
        while let Some(event) = tokio::select! {_=stop.cancelled()=>None,event=events.recv()=>event}
        {
            if !runtime.watches.lock().await.contains_key(&id) {
                break;
            }
            if !runtime.instance(instance).await.is_ok_and(|i| i.enabled) {
                runtime.watches.lock().await.remove(&id);
                break;
            }
            if overflow.swap(false, std::sync::atomic::Ordering::Relaxed) {
                let _=runtime.journal.extension_event("file.changed",json!({"watch_id":id,"instance_id":instance,"gap":true,"reason":"watch_buffer_overflow"})).await;
            }
            let data = match event {
                Ok(event) => {
                    json!({"watch_id":id,"instance_id":instance,"root":observed,"kind":format!("{:?}",event.kind),"paths":event.paths})
                }
                Err(_) => json!({"watch_id":id,"instance_id":instance,"root":observed,"gap":true}),
            };
            let _ = runtime.journal.extension_event("file.changed", data).await;
        }
    });
    Ok(json!({"id":id,"path":path,"recursive":recursive}))
}
pub(super) async fn stop(runtime: &Runtime, instance: Uuid, id: Uuid) -> Result<Value> {
    let mut watches = runtime.watches.lock().await;
    ensure!(
        watches.get(&id).is_some_and(|w| w.instance == instance),
        "watch does not belong to instance"
    );
    watches.remove(&id);
    Ok(json!({"state":"stopped"}))
}
