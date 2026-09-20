pub mod api;
mod calendar_api;
pub mod config;
mod scheduling_api;
mod web;
mod worker;

use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use mnema_infra::db::Vault;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{api::AppState, config::ServerConfig};

pub fn router(state: AppState) -> Router {
    Router::new()
        .nest("/api", api::router())
        .merge(web::router())
        .with_state(state)
}

pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("mnema_server=info")),
        )
        .init();

    let config = ServerConfig::from_env()?;
    let vault = Vault::connect_or_init(&config.vault_path)
        .await
        .context("failed to connect to Mnema storage")?;
    vault
        .initialize_defaults()
        .await
        .context("failed to initialize Mnema defaults")?;
    scheduling_api::initialize_default_preferences(&vault, &config)
        .await
        .context("failed to initialize scheduling preferences")?;

    let state = AppState::from_env(vault, Arc::new(config.clone()))
        .context("failed to initialize credential storage")?;
    worker::spawn(state.clone());
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;

    info!(
        address = %config.bind_addr,
        vault = %config.vault_path.display(),
        "Mnema server listening"
    );

    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server failed")?;
    Ok(())
}

async fn shutdown_signal() {
    if tokio::signal::ctrl_c().await.is_ok() {
        info!("shutdown signal received");
    }
}
