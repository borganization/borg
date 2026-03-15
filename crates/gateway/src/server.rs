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
use crate::handler::InboundMessage;
use crate::manifest::ChannelMode;
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
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Gateway listening on {addr} with {channel_count} channel(s)");

        // Spawn poll loops for poll-mode channels
        let mut poll_handles = Vec::new();
        for channel in state.registry.all_channels() {
            if channel.manifest.settings.mode != ChannelMode::Poll {
                continue;
            }

            let poll_interval_ms = channel.manifest.settings.poll_interval_ms.unwrap_or(5000);
            let channel_name = channel.manifest.name.clone();
            let channel_dir = channel.dir.clone();
            let manifest = channel.manifest.clone();
            let config = state.config.clone();
            let shutdown = self.shutdown.clone();

            info!(
                "Starting poll loop for channel '{}' (interval: {}ms)",
                channel_name, poll_interval_ms
            );

            let request_timeout = state.request_timeout;
            let handle = tokio::spawn(async move {
                let start =
                    tokio::time::Instant::now() + Duration::from_millis(poll_interval_ms);
                let mut interval =
                    tokio::time::interval_at(start, Duration::from_millis(poll_interval_ms));

                loop {
                    tokio::select! {
                        _ = shutdown.cancelled() => {
                            info!("Poll loop for '{}' shutting down", channel_name);
                            break;
                        }
                        _ = interval.tick() => {
                            let executor =
                                crate::executor::ChannelExecutor::new(&manifest, &channel_dir);
                            let blocked_paths = &config.security.blocked_paths;

                            let input = serde_json::json!({
                                "channel_dir": channel_dir.to_string_lossy(),
                            });

                            let poll_result = executor
                                .poll(&input.to_string(), blocked_paths)
                                .await;

                            let output = match poll_result {
                                Ok(o) => o,
                                Err(e) => {
                                    warn!("Poll error for '{}': {e}", channel_name);
                                    continue;
                                }
                            };

                            let trimmed = output.trim();
                            if trimmed.is_empty() {
                                continue;
                            }

                            let messages: Vec<InboundMessage> =
                                match serde_json::from_str(trimmed) {
                                    Ok(m) => m,
                                    Err(e) => {
                                        warn!(
                                            "Failed to parse poll output for '{}': {e}",
                                            channel_name
                                        );
                                        continue;
                                    }
                                };

                            if messages.is_empty() {
                                continue;
                            }

                            info!(
                                "Poll for '{}' returned {} message(s)",
                                channel_name,
                                messages.len()
                            );

                            let reg_channel = crate::registry::RegisteredChannel {
                                manifest: manifest.clone(),
                                dir: channel_dir.clone(),
                            };

                            for msg in messages {
                                let result = tokio::time::timeout(
                                    request_timeout,
                                    handler::handle_polled_message(
                                        &reg_channel,
                                        msg,
                                        &config,
                                    ),
                                )
                                .await;

                                match result {
                                    Ok(Err(e)) => warn!(
                                        "Failed to handle polled message for '{}': {e}",
                                        channel_name
                                    ),
                                    Err(_) => warn!(
                                        "Polled message for '{}' timed out",
                                        channel_name
                                    ),
                                    Ok(Ok(_)) => {}
                                }
                            }
                        }
                    }
                }
            });

            poll_handles.push(handle);
        }

        let shutdown = self.shutdown.clone();
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                shutdown.cancelled().await;
                info!("Gateway shutting down");
            })
            .await?;

        // Wait for poll tasks to finish after server shutdown
        for handle in poll_handles {
            let _ = handle.await;
        }

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
