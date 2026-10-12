use agent_history_manager::{api, config::Config, db, ingest::Ingestor};
use clap::Parser;
use std::{sync::Arc, time::Duration};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    db::init(&config.db())?;
    let state = Arc::new(Ingestor::new(config.db(), config.roots()));
    state.load_saved_paths()?;
    if config.full_ingest || config.sync_once {
        let full = config.full_ingest;
        let summary = tokio::task::spawn_blocking(move || state.run(full)).await??;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind((config.host.as_str(), config.port)).await?;
    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
    let worker = if config.no_sync {
        None
    } else {
        let state = state.clone();
        let seconds = config.sync_interval;
        Some(tokio::spawn(async move {
            loop {
                if *stop_rx.borrow() {
                    break;
                }
                let state = state.clone();
                match tokio::task::spawn_blocking(move || state.run(false)).await {
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => eprintln!("sync failed: {e:#}"),
                    Err(e) => eprintln!("sync task failed: {e}"),
                };
                tokio::select! {_=tokio::time::sleep(Duration::from_secs(seconds))=>{},_=stop_rx.changed()=>{break;}}
            }
        }))
    };
    println!(
        "AgentHistoryManager Rust: http://{}:{}",
        config.host, config.port
    );
    let result = axum::serve(listener, api::router(state, config.web()))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    let _ = stop_tx.send(true);
    if let Some(worker) = worker {
        worker.await?;
    }
    result?;
    Ok(())
}
