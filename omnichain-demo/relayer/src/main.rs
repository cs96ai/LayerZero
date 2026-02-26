mod config;
mod db;
mod eth;
mod event;
mod server;
mod solana_sim;
mod state_machine;
mod traffic_gen;
mod types;
mod verification;

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, error};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "relayer=info".into()),
        )
        .init();

    info!("Starting omnichain escrow relayer...");

    let cfg = config::Config::from_env();
    info!(?cfg, "Loaded configuration");

    // Initialize SQLite database
    let pool = db::init_db(&cfg.database_url).await?;
    info!("Database initialized");

    // Event broadcast channel for WebSocket streaming
    let (event_tx, _) = broadcast::channel::<event::LifecycleEvent>(1024);

    // Auto-start simulation if AUTO_START env is set (default: true in containers)
    let auto_start = std::env::var("AUTO_START_SIMULATION")
        .unwrap_or_else(|_| "false".into())
        .parse::<bool>()
        .unwrap_or(false);

    let auto_deadline = if auto_start {
        // 1-hour deadline
        chrono::Utc::now().timestamp() + 3600
    } else {
        0
    };

    // Shared application state
    let app_state = Arc::new(types::AppState {
        pool: pool.clone(),
        event_tx: event_tx.clone(),
        paused: std::sync::atomic::AtomicBool::new(false),
        simulation_running: std::sync::atomic::AtomicBool::new(auto_start),
        simulation_deadline: std::sync::atomic::AtomicI64::new(auto_deadline),
        config: cfg.clone(),
    });

    if auto_start {
        info!("Auto-starting simulation (1 hour)");
    }

    // Spawn the HTTP + WebSocket server
    let server_state = app_state.clone();
    let server_port = cfg.http_port;
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server::run_server(server_state, server_port).await {
            error!(?e, "Server error");
        }
    });

    // Spawn the Ethereum event listener + state machine loop
    let processor_state = app_state.clone();
    let processor_cfg = cfg.clone();
    let processor_handle = tokio::spawn(async move {
        if let Err(e) = state_machine::run_processor(processor_state, processor_cfg).await {
            error!(?e, "Processor error");
        }
    });

    // Spawn the embedded traffic generator
    let traffic_state = app_state.clone();
    let traffic_rpc = cfg.eth_rpc_url.clone();
    let traffic_escrow = cfg.escrow_address.clone();
    let traffic_handle = tokio::spawn(async move {
        traffic_gen::run_traffic_generator(traffic_state, traffic_rpc, traffic_escrow).await;
    });

    // Wait for any to finish (they shouldn't under normal operation)
    tokio::select! {
        r = server_handle => {
            error!(?r, "Server task ended");
        }
        r = processor_handle => {
            error!(?r, "Processor task ended");
        }
        r = traffic_handle => {
            error!(?r, "Traffic generator task ended");
        }
    }

    Ok(())
}
