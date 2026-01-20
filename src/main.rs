//! Serabut - PXE Boot Server
//!
//! HTTP server for PXE booting multiple OSes with multiple configurations.

use tikv_jemallocator::Jemalloc;

#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

mod config;
mod error;
mod routes;
mod services;
mod utils;

use config::AppState;
use services::iso::IsoService;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Notify;
use tracing_appender::rolling;
use tracing_subscriber::EnvFilter;

const DEFAULT_CONFIG_PATH: &str = "/etc/serabutd.conf";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line args for config path
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_PATH));

    // Load initial configuration
    let state = AppState::new(config_path.clone())?;
    let config = state.config().await;

    // Initialize logging to /var/log/serabut/ (use logrotate for rotation)
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.tracing_filter()));

    let file_appender = rolling::never("/var/log/serabut", "serabutd.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(non_blocking)
        .init();

    tracing::info!("Starting serabutd PXE boot server");
    tracing::info!("Config path: {:?}", config_path);
    tracing::info!("Data path: {:?}", config.config_path);

    // Validate ISO directory structure
    let iso_service = IsoService::new(config.config_path.clone());
    iso_service.validate_startup();

    // Create router
    let app = routes::create_router(state.clone());

    // Bind address
    let addr = SocketAddr::from((config.interface, config.port));
    tracing::info!("HTTP server listening on IP {} port {}", config.interface, config.port);

    let listener = TcpListener::bind(addr).await?;

    // Set up signal handlers
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    // Handle SIGHUP for config reload
    tokio::spawn(handle_sighup(state.clone()));

    // Handle SIGTERM and SIGINT for graceful shutdown
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to install SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to install SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, shutting down");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT, shutting down");
            }
        }

        shutdown_clone.notify_one();
    });

    // Run server with graceful shutdown
    // Use into_make_service_with_connect_info to enable ConnectInfo<SocketAddr> for request logging
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await?;

    tracing::info!("Server stopped");
    Ok(())
}

/// Handle SIGHUP signals for configuration reload.
async fn handle_sighup(state: AppState) {
    let mut sighup = signal(SignalKind::hangup()).expect("Failed to install SIGHUP handler");

    loop {
        sighup.recv().await;
        tracing::info!("Received SIGHUP, reloading configuration");

        if let Err(e) = state.reload().await {
            tracing::error!("Failed to reload configuration: {}", e);
        }
    }
}
