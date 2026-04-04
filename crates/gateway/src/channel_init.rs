//! Channel client initialization extracted from `GatewayServer::run`.
//!
//! Each function resolves credentials, creates a client, runs a probe/auth check,
//! and returns the client (if successful) plus any metadata fields.

use std::sync::Arc;

use borg_core::config::Config;
use borg_core::db::Database;
use tracing::{info, warn};

use crate::discord::api::DiscordClient;
use crate::google_chat::api::GoogleChatClient;
use crate::signal::api::SignalClient;
use crate::slack::api::SlackClient;
use crate::teams::api::TeamsClient;
use crate::telegram::api::TelegramClient;
use crate::twilio::api::TwilioClient;

fn log_gateway_activity(message: &str) {
    if let Ok(adb) = Database::open_with_timeout(Database::GATEWAY_BUSY_TIMEOUT_MS) {
        borg_core::activity_log::log_activity(&adb, "info", "gateway", message);
    }
}

/// Initialize Telegram client. Returns (client, bot_username).
pub(crate) async fn init_telegram(
    config: &Config,
) -> (Option<Arc<TelegramClient>>, Option<String>, Option<String>) {
    let telegram_token = config.resolve_credential_or_env("TELEGRAM_BOT_TOKEN");
    let telegram_secret = config.resolve_credential_or_env("TELEGRAM_WEBHOOK_SECRET");

    let (client, bot_username) = match telegram_token {
        Some(token) => {
            let client = match TelegramClient::new(&token) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to create Telegram client: {e}");
                    return (None, None, telegram_secret);
                }
            };
            match client.get_me().await {
                Ok(me) => {
                    info!(
                        "Telegram native integration active (bot: @{})",
                        me.username.as_deref().unwrap_or(&me.first_name)
                    );
                    log_gateway_activity("Telegram native integration active");
                    let bot_username = me.username.clone();

                    // Set webhook if public_url is configured
                    if let Some(ref url) = config.gateway.public_url {
                        let webhook_url = format!("{url}/webhook/telegram");
                        if let Err(e) = client
                            .set_webhook(&webhook_url, telegram_secret.as_deref())
                            .await
                        {
                            warn!("Failed to set Telegram webhook: {e}");
                        } else {
                            info!("Telegram webhook set to {webhook_url}");
                        }
                    }

                    // Register bot command menu
                    {
                        use crate::commands::{NativeCommandRegistration, GATEWAY_COMMANDS};
                        if let Err(e) = client.register_commands(GATEWAY_COMMANDS).await {
                            warn!("Failed to register Telegram bot commands: {e}");
                        } else {
                            info!("Telegram bot menu commands registered");
                        }
                    }

                    (Some(Arc::new(client)), bot_username)
                }
                Err(e) => {
                    warn!("TELEGRAM_BOT_TOKEN set but getMe failed: {e}");
                    (None, None)
                }
            }
        }
        None => (None, None),
    };

    (client, bot_username, telegram_secret)
}

/// Initialize Slack client. Returns (client, signing_secret, bot_user_id).
pub(crate) async fn init_slack(
    config: &Config,
) -> (Option<Arc<SlackClient>>, Option<String>, Option<String>) {
    let slack_token = config.resolve_credential_or_env("SLACK_BOT_TOKEN");
    let slack_signing_secret = config.resolve_credential_or_env("SLACK_SIGNING_SECRET");

    let (client, bot_user_id) = match slack_token {
        Some(token) => match SlackClient::new(&token) {
            Ok(mut client) => match client.auth_test().await {
                Ok(resp) => {
                    info!(
                        "Slack native integration active (bot: {}, team: {})",
                        resp.user.as_deref().unwrap_or("unknown"),
                        resp.team.as_deref().unwrap_or("unknown"),
                    );
                    log_gateway_activity("Slack native integration active");
                    let bot_uid = client.bot_user_id().map(String::from);
                    (Some(Arc::new(client)), bot_uid)
                }
                Err(e) => {
                    warn!("SLACK_BOT_TOKEN set but auth.test failed: {e}");
                    (None, None)
                }
            },
            Err(e) => {
                warn!("Failed to create Slack HTTP client: {e}");
                (None, None)
            }
        },
        None => (None, None),
    };

    (client, slack_signing_secret, bot_user_id)
}

/// Initialize Twilio client. Returns (client, auth_token, phone_number, whatsapp_number).
pub(crate) fn init_twilio(
    config: &Config,
) -> (
    Option<Arc<TwilioClient>>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let twilio_account_sid = config.resolve_credential_or_env("TWILIO_ACCOUNT_SID");
    let twilio_auth_token = config.resolve_credential_or_env("TWILIO_AUTH_TOKEN");
    let twilio_phone_number = config.resolve_credential_or_env("TWILIO_PHONE_NUMBER");
    let twilio_whatsapp_number = config.resolve_credential_or_env("TWILIO_WHATSAPP_NUMBER");

    let client = match (&twilio_account_sid, &twilio_auth_token) {
        (Some(sid), Some(token)) => {
            info!(
                "Twilio native integration active (account: {}...)",
                &sid[..sid.len().min(8)]
            );
            match TwilioClient::new(sid, token) {
                Ok(client) => Some(Arc::new(client)),
                Err(e) => {
                    warn!("Failed to create Twilio HTTP client: {e}");
                    None
                }
            }
        }
        _ => None,
    };

    (
        client,
        twilio_auth_token,
        twilio_phone_number,
        twilio_whatsapp_number,
    )
}

/// Initialize Discord client. Returns (client, public_key).
pub(crate) async fn init_discord(config: &Config) -> (Option<Arc<DiscordClient>>, Option<String>) {
    let discord_token = config.resolve_credential_or_env("DISCORD_BOT_TOKEN");
    let discord_public_key = config.resolve_credential_or_env("DISCORD_PUBLIC_KEY");

    let client = match discord_token {
        Some(token) => {
            let mut client = match DiscordClient::new(&token) {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to create Discord client: {e}");
                    return (None, discord_public_key);
                }
            };
            match client.get_current_user().await {
                Ok(user) => {
                    info!("Discord native integration active (bot: {})", user.username);
                    client.set_application_id(user.id.clone());

                    // Register global slash commands
                    {
                        use crate::commands::{NativeCommandRegistration, GATEWAY_COMMANDS};
                        if let Err(e) = client.register_commands(GATEWAY_COMMANDS).await {
                            warn!("Failed to register Discord slash commands: {e}");
                        } else {
                            info!("Discord slash commands registered");
                        }
                    }

                    Some(Arc::new(client))
                }
                Err(e) => {
                    warn!("DISCORD_BOT_TOKEN set but /users/@me failed: {e}");
                    None
                }
            }
        }
        None => None,
    };

    (client, discord_public_key)
}

/// Initialize Teams client. Returns (client, app_secret).
pub(crate) fn init_teams(
    config: &Config,
) -> anyhow::Result<(Option<Arc<TeamsClient>>, Option<String>)> {
    let teams_app_id = config.resolve_credential_or_env("TEAMS_APP_ID");
    let teams_app_secret = config.resolve_credential_or_env("TEAMS_APP_SECRET");

    let client = match (&teams_app_id, &teams_app_secret) {
        (Some(app_id), Some(app_secret)) => {
            info!(
                "Teams native integration active (app: {}...)",
                &app_id[..app_id.len().min(8)]
            );
            Some(Arc::new(TeamsClient::new(app_id, app_secret)?))
        }
        _ => None,
    };

    Ok((client, teams_app_secret))
}

/// Initialize Google Chat client. Returns (client, webhook_token).
pub(crate) fn init_google_chat(
    config: &Config,
) -> anyhow::Result<(Option<Arc<GoogleChatClient>>, Option<String>)> {
    let google_chat_service_token = config.resolve_credential_or_env("GOOGLE_CHAT_SERVICE_TOKEN");
    let google_chat_token = config.resolve_credential_or_env("GOOGLE_CHAT_WEBHOOK_TOKEN");

    let client = match google_chat_service_token {
        Some(token) => {
            info!("Google Chat native integration active");
            Some(Arc::new(GoogleChatClient::new(&token)?))
        }
        None => None,
    };

    Ok((client, google_chat_token))
}

/// Initialize Signal client.
pub(crate) async fn init_signal(config: &Config) -> Option<Arc<SignalClient>> {
    let signal_account = config.resolve_credential_or_env("SIGNAL_ACCOUNT");
    match signal_account {
        Some(ref account) => {
            let host = config
                .gateway
                .signal_cli_host
                .as_deref()
                .unwrap_or("localhost");
            let port = config.gateway.signal_cli_port.unwrap_or(8080);
            let base_url = format!("http://{host}:{port}");
            match SignalClient::new(&base_url, account) {
                Ok(client) => match client.probe().await {
                    Ok(version) => {
                        info!(
                            "Signal native integration active (account: {}, daemon: v{})",
                            account, version
                        );
                        Some(Arc::new(client))
                    }
                    Err(e) => {
                        warn!("SIGNAL_ACCOUNT set but signal-cli daemon not reachable: {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("Failed to create Signal client: {e}");
                    None
                }
            }
        }
        None => None,
    }
}
