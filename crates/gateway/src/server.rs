use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use tamagotchi_core::config::Config;
use tamagotchi_core::db::Database;

use crate::handler;
use crate::handler::InboundMessage;
use crate::health::ChannelHealthRegistry;
use crate::manifest::ChannelMode;
use crate::rate_limit::SlidingWindowLimiter;
use crate::registry::ChannelRegistry;
use crate::retry::RetryPolicy;
use crate::telegram::api::TelegramClient;
use crate::telegram::dedup::UpdateDeduplicator;

struct AppState {
    config: Config,
    registry: ChannelRegistry,
    semaphore: Semaphore,
    request_timeout: Duration,
    health: Arc<RwLock<ChannelHealthRegistry>>,
    rate_limiter: Option<Arc<Mutex<SlidingWindowLimiter>>>,
    telegram_client: Option<Arc<TelegramClient>>,
    telegram_dedup: Arc<Mutex<UpdateDeduplicator>>,
    telegram_secret: Option<String>,
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

        // Initialize health registry
        let mut health_reg = ChannelHealthRegistry::new();
        for channel in registry.all_channels() {
            health_reg.register(&channel.manifest.name);
        }
        let health = Arc::new(RwLock::new(health_reg));

        // Initialize rate limiter (0 = disabled)
        let rate_limiter = if gateway_config.rate_limit_per_minute > 0 {
            Some(Arc::new(Mutex::new(SlidingWindowLimiter::new(
                gateway_config.rate_limit_per_minute,
                Duration::from_secs(60),
            ))))
        } else {
            None
        };

        // Initialize native Telegram client if token is available
        let telegram_client = match std::env::var("TELEGRAM_BOT_TOKEN") {
            Ok(token) if !token.is_empty() => {
                let client = TelegramClient::new(&token);
                match client.get_me().await {
                    Ok(me) => {
                        info!(
                            "Telegram native integration active (bot: @{})",
                            me.username.as_deref().unwrap_or(&me.first_name)
                        );

                        // Set webhook if public_url is configured
                        if let Some(ref url) = self.config.gateway.public_url {
                            let webhook_url = format!("{url}/webhook/telegram");
                            let secret = std::env::var("TELEGRAM_WEBHOOK_SECRET").ok();
                            if let Err(e) =
                                client.set_webhook(&webhook_url, secret.as_deref()).await
                            {
                                warn!("Failed to set Telegram webhook: {e}");
                            } else {
                                info!("Telegram webhook set to {webhook_url}");
                            }
                        }

                        Some(Arc::new(client))
                    }
                    Err(e) => {
                        warn!("TELEGRAM_BOT_TOKEN set but getMe failed: {e}");
                        None
                    }
                }
            }
            _ => None,
        };

        let telegram_secret = std::env::var("TELEGRAM_WEBHOOK_SECRET").ok();

        let state = Arc::new(AppState {
            config: self.config.clone(),
            registry,
            semaphore: Semaphore::new(gateway_config.max_concurrent),
            request_timeout: Duration::from_millis(gateway_config.request_timeout_ms),
            health: health.clone(),
            rate_limiter,
            telegram_client: telegram_client.clone(),
            telegram_dedup: Arc::new(Mutex::new(UpdateDeduplicator::new())),
            telegram_secret,
        });

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/health/channels", get(channel_health_handler))
            .route("/channels", get(list_channels_handler))
            .route("/webhook/{name}", post(webhook_handler))
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024)) // 2 MB
            .with_state(state.clone());

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!("Gateway listening on {addr} with {channel_count} channel(s)");

        // Replay unfinished deliveries from previous run
        if let Ok(db) = Database::open() {
            match db.replay_unfinished() {
                Ok(0) => {}
                Ok(n) => info!("Reset {n} in-flight delivery(ies) to pending"),
                Err(e) => warn!("Failed to replay unfinished deliveries: {e}"),
            }
        }

        // Spawn delivery drain loop
        let drain_shutdown = self.shutdown.clone();
        let drain_state = state.clone();
        let drain_health = health.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    _ = drain_shutdown.cancelled() => break,
                    _ = interval.tick() => {
                        drain_pending_deliveries(&drain_state, &drain_health).await;
                    }
                }
            }
        });

        // Spawn poll loops for poll-mode channels
        let mut poll_handles = Vec::new();
        for channel in state.registry.all_channels() {
            if channel.manifest.settings.mode != ChannelMode::Poll {
                continue;
            }

            // iMessage is handled natively by the daemon — skip its poll loop here
            if channel.manifest.name == "imessage" {
                info!(
                    "Skipping poll loop for '{}' (handled natively)",
                    channel.manifest.name
                );
                continue;
            }

            let poll_interval_ms = channel.manifest.settings.poll_interval_ms.unwrap_or(5000);
            let channel_name = channel.manifest.name.clone();
            let channel_dir = channel.dir.clone();
            let manifest = channel.manifest.clone();
            let config = state.config.clone();
            let shutdown = self.shutdown.clone();
            let poll_health = health.clone();

            info!(
                "Starting poll loop for channel '{}' (interval: {}ms)",
                channel_name, poll_interval_ms
            );

            let request_timeout = state.request_timeout;
            let handle = tokio::spawn(async move {
                let start = tokio::time::Instant::now() + Duration::from_millis(poll_interval_ms);
                let mut interval =
                    tokio::time::interval_at(start, Duration::from_millis(poll_interval_ms));

                let mut consecutive_errors: u32 = 0;
                let initial_backoff = Duration::from_secs(5);
                let max_backoff = Duration::from_secs(300);
                let max_consecutive_errors: u32 = 10;

                loop {
                    tokio::select! {
                        _ = shutdown.cancelled() => {
                            info!("Poll loop for '{}' shutting down", channel_name);
                            break;
                        }
                        _ = interval.tick() => {
                            // If in error state, apply backoff
                            if consecutive_errors > 0 {
                                let backoff_secs = initial_backoff.as_secs_f64()
                                    * 2.0_f64.powi((consecutive_errors - 1) as i32);
                                let backoff = Duration::from_secs_f64(
                                    backoff_secs.min(max_backoff.as_secs_f64())
                                );

                                if consecutive_errors >= max_consecutive_errors {
                                    warn!(
                                        "Poll loop for '{}' hit {} consecutive errors, pausing for {:?}",
                                        channel_name, consecutive_errors, max_backoff
                                    );
                                    tokio::time::sleep(max_backoff).await;
                                    consecutive_errors = 0;
                                    poll_health.write().await.record_reconnect(&channel_name);
                                    continue;
                                }

                                tokio::time::sleep(backoff).await;
                            }

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
                                Ok(o) => {
                                    if consecutive_errors > 0 {
                                        info!(
                                            "Poll loop for '{}' recovered after {} error(s)",
                                            channel_name, consecutive_errors
                                        );
                                        poll_health.write().await.record_reconnect(&channel_name);
                                        consecutive_errors = 0;
                                    }
                                    o
                                }
                                Err(e) => {
                                    consecutive_errors += 1;
                                    warn!(
                                        "Poll error for '{}' (consecutive: {}): {e}",
                                        channel_name, consecutive_errors
                                    );
                                    poll_health.write().await.record_error(&channel_name, &e.to_string());
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
                                        consecutive_errors += 1;
                                        warn!(
                                            "Failed to parse poll output for '{}': {e}",
                                            channel_name
                                        );
                                        poll_health.write().await.record_error(&channel_name, &e.to_string());
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
                                        Some(&poll_health),
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
        let shutdown_tg_client = telegram_client.clone();
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
            info!("Gateway shutting down");

            // Clean up Telegram webhook on shutdown
            if let Some(client) = shutdown_tg_client {
                if let Err(e) = client.delete_webhook().await {
                    warn!("Failed to delete Telegram webhook on shutdown: {e}");
                } else {
                    info!("Telegram webhook removed");
                }
            }
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

async fn channel_health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let snapshot = state.health.read().await.snapshot();
    axum::Json(serde_json::json!({ "channels": snapshot }))
}

async fn list_channels_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let channels = state.registry.list_channels();
    axum::Json(serde_json::json!({ "channels": channels }))
}

fn extract_client_ip(
    headers: &HeaderMap,
    connect_info: &ConnectInfo<std::net::SocketAddr>,
) -> String {
    // Check X-Forwarded-For first
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(val) = xff.to_str() {
            if let Some(first) = val.split(',').next() {
                let trimmed = first.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }

    // Check X-Real-IP
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(val) = xri.to_str() {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    // Fall back to peer address
    connect_info.0.ip().to_string()
}

async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    let channel = match state.registry.get(&name) {
        Some(c) => c,
        None => {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(serde_json::json!({ "error": "Unknown channel" })),
            );
        }
    };

    // Rate limiting
    if let Some(ref limiter) = state.rate_limiter {
        let client_ip_str = extract_client_ip(&headers, &ConnectInfo(addr));
        let is_loopback = client_ip_str
            .parse::<IpAddr>()
            .map(|ip| SlidingWindowLimiter::is_exempt(&ip))
            .unwrap_or(false);

        if !is_loopback {
            let allowed = limiter.lock().await.check(&client_ip_str);
            if !allowed {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    axum::Json(serde_json::json!({ "error": "Rate limit exceeded" })),
                );
            }
        }
    }

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

    // Native Telegram handling
    if name == "telegram" {
        if let Some(ref tg_client) = state.telegram_client {
            let result = tokio::time::timeout(state.request_timeout, async {
                let inbound = crate::telegram::handle_telegram_webhook(
                    &headers,
                    &body,
                    state.telegram_secret.as_deref(),
                    &state.telegram_dedup,
                )
                .await?;

                let inbound = match inbound {
                    Some(msg) => msg,
                    None => return Ok::<_, anyhow::Error>("(skipped)".to_string()),
                };

                let chat_id: i64 = inbound
                    .channel_id
                    .as_deref()
                    .and_then(|id| id.parse().ok())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing or invalid chat_id in Telegram update")
                    })?;

                // Send typing indicator
                let _ = tg_client.send_typing(chat_id).await;

                // Invoke agent
                let (response_text, _session_id) =
                    handler::invoke_agent("telegram", &inbound, &state.config, Some(&state.health))
                        .await?;

                // Send response via Telegram API
                if let Err(e) = tg_client.send_message(chat_id, &response_text, None).await {
                    warn!("Failed to send Telegram response: {e}");
                    state
                        .health
                        .write()
                        .await
                        .record_error("telegram", &e.to_string());
                } else {
                    state.health.write().await.record_outbound("telegram");
                }

                Ok(response_text)
            })
            .await;

            return match result {
                Ok(Ok(_)) => (
                    StatusCode::OK,
                    axum::Json(serde_json::json!({ "ok": true })),
                ),
                Ok(Err(e)) => {
                    warn!("Telegram webhook error: {e:#}");
                    (
                        StatusCode::OK,
                        axum::Json(serde_json::json!({ "ok": true })),
                    )
                }
                Err(_) => (
                    StatusCode::GATEWAY_TIMEOUT,
                    axum::Json(serde_json::json!({ "error": "Request timed out" })),
                ),
            };
        }
    }

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
        handler::handle_webhook(
            channel,
            headers_json,
            body,
            &state.config,
            Some(&state.health),
        ),
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

/// Background task: claim pending deliveries and attempt to send them.
async fn drain_pending_deliveries(
    state: &Arc<AppState>,
    health: &Arc<RwLock<ChannelHealthRegistry>>,
) {
    let db = match Database::open() {
        Ok(db) => db,
        Err(e) => {
            warn!("Drain loop: failed to open database: {e}");
            return;
        }
    };

    let deliveries = match db.claim_pending_deliveries(20) {
        Ok(d) => d,
        Err(e) => {
            warn!("Drain loop: failed to claim deliveries: {e}");
            return;
        }
    };

    if deliveries.is_empty() {
        return;
    }

    info!(
        "Drain loop: processing {} pending delivery(ies)",
        deliveries.len()
    );

    for delivery in deliveries {
        let channel = match state.registry.get(&delivery.channel_name) {
            Some(c) => c,
            None => {
                warn!(
                    "Drain loop: channel '{}' not found, marking failed",
                    delivery.channel_name
                );
                let _ = db.mark_failed(&delivery.id, "channel not found", None);
                continue;
            }
        };

        let executor = crate::executor::ChannelExecutor::new(&channel.manifest, &channel.dir);
        let blocked_paths = &state.config.security.blocked_paths;

        let mut policy = RetryPolicy::default();
        if let Some(max) = channel.manifest.settings.retry_max_attempts {
            policy.max_retries = max;
        }
        if let Some(delay) = channel.manifest.settings.retry_initial_delay_ms {
            policy.initial_delay_ms = delay;
        }
        // For drain loop, use fewer retries to avoid blocking
        policy.max_retries = policy.max_retries.min(2);

        match crate::retry::send_with_retry(
            &executor,
            &delivery.payload_json,
            blocked_paths,
            &policy,
        )
        .await
        {
            crate::retry::RetryOutcome::Success(_) => {
                let _ = db.mark_delivered(&delivery.id);
                health.write().await.record_outbound(&delivery.channel_name);
            }
            crate::retry::RetryOutcome::PermanentFailure(e) => {
                let _ = db.mark_failed(&delivery.id, &e, None);
                health
                    .write()
                    .await
                    .record_error(&delivery.channel_name, &e);
            }
            crate::retry::RetryOutcome::Exhausted(e) => {
                let next_retry = chrono::Utc::now().timestamp() + 60;
                let _ = db.mark_failed(&delivery.id, &e, Some(next_retry));
                health
                    .write()
                    .await
                    .record_error(&delivery.channel_name, &e);
            }
        }
    }
}
