use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use tamagotchi_core::config::Config;

use crate::handler;
use crate::registry::ChannelRegistry;

struct AppState {
    config: Config,
    registry: ChannelRegistry,
    semaphore: Semaphore,
    request_timeout: Duration,
}

pub struct GatewayServer {
    config: Config,
    shutdown: CancellationToken,
}

impl GatewayServer {
    pub fn new(config: Config, shutdown: CancellationToken) -> Result<Self> {
        Ok(Self { config, shutdown })
    }

    pub async fn run(self) -> Result<()> {
        let gateway_config = &self.config.gateway;
        let addr = format!("{}:{}", gateway_config.host, gateway_config.port);

        let registry = ChannelRegistry::new()?;
        let channel_count = registry.list_channels().len();

        let state = Arc::new(AppState {
            config: self.config.clone(),
            registry,
            semaphore: Semaphore::new(gateway_config.max_concurrent),
            request_timeout: Duration::from_millis(gateway_config.request_timeout_ms),
        });

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/channels", get(list_channels_handler))
            .route("/webhook/{name}", post(webhook_handler))
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024)) // 2 MB
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Gateway listening on {addr} with {channel_count} channel(s)");

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                self.shutdown.cancelled().await;
                info!("Gateway shutting down");
            })
            .await?;

        Ok(())
    }
}

async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

async fn list_channels_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let channels = state.registry.list_channels();
    axum::Json(serde_json::json!({ "channels": channels }))
}

async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let channel = match state.registry.get(&name) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": format!("Unknown channel: {name}") })),
            );
        }
    };

    // Acquire concurrency permit
    let _permit = match state.semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({ "error": "Too many concurrent requests" })),
            );
        }
    };

    // Convert headers to JSON
    let mut headers_map = serde_json::Map::new();
    for (key, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            headers_map.insert(
                key.as_str().to_string(),
                serde_json::Value::String(v.to_string()),
            );
        }
    }
    let headers_json = serde_json::Value::Object(headers_map);

    // Process with timeout
    let result = tokio::time::timeout(
        state.request_timeout,
        handler::handle_webhook(channel, headers_json, body, &state.config),
    )
    .await;

    match result {
        Ok(Ok(response)) => (
            StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "response": response })),
        ),
        Ok(Err(e)) => {
            warn!("Webhook handler error for '{name}': {e:#}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({ "error": "Internal server error" })),
            )
        }
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({ "error": "Request timed out" })),
        ),
    }
}
