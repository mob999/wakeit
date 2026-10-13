use crate::{
    api::{App, router},
    bus,
    config::GatewayConfig,
    nats_auth::Authority,
    protocol::RunEvent,
    store,
};
use anyhow::Result;
use futures::StreamExt;
use std::{sync::Arc, time::Duration};

pub async fn start(config: GatewayConfig) -> Result<()> {
    anyhow::ensure!(
        config.version == 1,
        "unsupported Gateway configuration version"
    );
    let pool = store::pool(&config.database_url_env).await?;
    // Migrations are explicit: deployments migrate once before starting replicas.
    sqlx::query("SELECT 1 FROM settings LIMIT 1")
        .execute(&pool)
        .await?;
    let client = bus::connect(
        &config.nats_url,
        &config.nats_auth_dir.join("gateway.creds"),
        None,
    )
    .await?;
    let js = async_nats::jetstream::new(client.clone());
    bus::setup(&js, config.nats_replicas).await?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let app = App {
        client,
        pool,
        js,
        authority: Authority {
            dir: config.nats_auth_dir.clone(),
        },
        config: Arc::new(config),
        shutdown: cancel.clone(),
    };
    let scheduler = tokio::spawn({
        let s = app.clone();
        let cancel = cancel.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {_=cancel.cancelled()=>break,_=tick.tick()=>{if let Err(e)=store::tick(&s.pool).await{tracing::error!(error=%e,"scheduler tick failed")}}}
            }
        }
    });
    let outbox = tokio::spawn({
        let s = app.clone();
        let cancel = cancel.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {_=cancel.cancelled()=>break,_=tick.tick()=>{if let Err(e)=store::dispatch(&s.pool,&s.js).await{tracing::warn!(error=%e,"outbox publication deferred")}}}
            }
        }
    });
    let ingest = tokio::spawn({
        let s = app.clone();
        let cancel = cancel.clone();
        async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                if let Err(e) = consume_events(&s, &cancel).await {
                    tracing::error!(error=%e,"event consumer reconnecting");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    });
    let maintenance = tokio::spawn({
        let s = app.clone();
        let cancel = cancel.clone();
        async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {_=cancel.cancelled()=>break,_=tick.tick()=>{
                    if let Err(e)=s.authority.sync_revocations(&s.config.nats_url,&s.pool).await{tracing::warn!(error=%e,"NATS revocation sync deferred")}
                    if let Err(e)=cleanup(&s).await{tracing::warn!(error=%e,"retention cleanup failed")}
                }}
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(&app.config.listen).await?;
    tracing::info!(address=%listener.local_addr()?,"wakeit gateway ready");
    let stop = cancel.clone();
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            stop.cancel();
        })
        .await?;
    cancel.cancel();
    let _ = tokio::join!(scheduler, outbox, ingest, maintenance);
    Ok(())
}
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install signal handler");
        tokio::select! {_=term.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
async fn consume_events(s: &App, cancel: &tokio_util::sync::CancellationToken) -> Result<()> {
    let stream = s.js.get_stream("WAKEIT_EVENTS").await?;
    let consumer = stream
        .get_or_create_consumer(
            "gateway",
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some("gateway".into()),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                max_ack_pending: 1000,
                ..Default::default()
            },
        )
        .await?;
    let mut messages = consumer.messages().await?;
    loop {
        tokio::select! {_=cancel.cancelled()=>break,msg=messages.next()=>{let Some(msg)=msg else{break};let msg=msg?;
            let e=match serde_json::from_slice::<RunEvent>(&msg.payload){Ok(e) if e.version==1 && e.seq>0 && msg.subject.as_str()==crate::protocol::event_subject(e.runner_id)=>e,_=>{tracing::warn!("discarding malformed execution event");msg.ack_with(async_nats::jetstream::AckKind::Term).await.map_err(|e|anyhow::anyhow!(e.to_string()))?;continue}};
            match store::ingest(&s.pool,&e,s.config.log_limit_bytes).await {Ok(())=>msg.double_ack().await.map_err(|e|anyhow::anyhow!(e.to_string()))?,Err(e)=>{tracing::error!(error=%e,"event persistence deferred");msg.ack_with(async_nats::jetstream::AckKind::Nak(Some(Duration::from_secs(2)))).await.map_err(|e|anyhow::anyhow!(e.to_string()))?;}}
        }}
    }
    Ok(())
}
async fn cleanup(s: &App) -> Result<()> {
    store::clear_completed_messages(&s.pool, &s.js).await?;
    let settings: serde_json::Value = sqlx::query_scalar("SELECT data FROM settings")
        .fetch_one(&s.pool)
        .await?;
    let logs = settings["log_retention_days"].as_i64().unwrap_or(7) as i32;
    let runs = settings["run_retention_days"].as_i64().unwrap_or(90) as i32;
    // Preserve content availability independently of the payload retention period.
    sqlx::query("WITH expired AS (DELETE FROM events WHERE kind IN ('log','truncated','trace') AND created_at<now()-make_interval(days=>$1) RETURNING run_id) INSERT INTO events(run_id,seq,kind,data) SELECT DISTINCT run_id, -1, 'trace_status', '{\"status\":\"expired\"}'::jsonb FROM expired ON CONFLICT(run_id,seq) DO NOTHING")
        .bind(logs).execute(&s.pool).await?;
    sqlx::query("DELETE FROM runs WHERE finished_at<now()-make_interval(days=>$1)")
        .bind(runs)
        .execute(&s.pool)
        .await?;
    sqlx::query("DELETE FROM outbox WHERE cleared_at<now()-interval '7 days'")
        .execute(&s.pool)
        .await?;
    sqlx::query("DELETE FROM login_attempts WHERE created_at<now()-interval '1 day'")
        .execute(&s.pool)
        .await?;
    sqlx::query("DELETE FROM credentials WHERE expires_at<now()")
        .execute(&s.pool)
        .await?;
    Ok(())
}
