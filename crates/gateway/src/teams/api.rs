use std::sync::RwLock;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::Client;
use tracing::warn;

use super::types::{ReplyActivity, TokenResponse};
use crate::chunker;

const TOKEN_ENDPOINT: &str = "https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token";
const MESSAGE_CHUNK_SIZE: usize = 4000;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 5;
const MAX_RETRY_AFTER_SECS: u64 = 300;
/// Buffer in seconds before token expiry to trigger refresh.
const TOKEN_EXPIRY_BUFFER_SECS: u64 = 60;

/// Known valid Microsoft service URL host suffixes for SSRF protection.
const ALLOWED_HOST_SUFFIXES: &[&str] = &[".botframework.com", ".teams.microsoft.com", ".skype.com"];

/// Cached OAuth2 token with expiry tracking.
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// A client for the Microsoft Bot Framework / Teams API.
pub struct TeamsClient {
    client: Client,
    app_id: String,
    app_secret: String,
    token_cache: RwLock<Option<CachedToken>>,
}

impl TeamsClient {
    pub fn new(app_id: &str, app_secret: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .unwrap_or_default(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            token_cache: RwLock::new(None),
        }
    }

    /// Get a valid OAuth2 access token, fetching a new one if the cache is empty or expired.
    pub async fn get_token(&self) -> Result<String> {
        // Check cache first
        {
            let cache = self
                .token_cache
                .read()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
            if let Some(ref cached) = *cache {
                if Instant::now() < cached.expires_at {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        // Fetch a new token
        let resp = self
            .client
            .post(TOKEN_ENDPOINT)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.app_id),
                ("client_secret", &self.app_secret),
                ("scope", "https://api.botframework.com/.default"),
            ])
            .send()
            .await
            .context("Failed to request Teams OAuth2 token")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            bail!("Token request failed ({status}): {body}");
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .context("Failed to parse token response")?;

        let expires_at = Instant::now()
            + Duration::from_secs(
                token_resp
                    .expires_in
                    .saturating_sub(TOKEN_EXPIRY_BUFFER_SECS),
            );

        let access_token = token_resp.access_token.clone();

        // Update cache
        {
            let mut cache = self
                .token_cache
                .write()
                .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
            *cache = Some(CachedToken {
                access_token: token_resp.access_token,
                expires_at,
            });
        }

        Ok(access_token)
    }

    /// Reply to a specific activity in a Teams conversation, automatically chunking long messages.
    pub async fn reply_to_activity(
        &self,
        service_url: &str,
        conversation_id: &str,
        activity_id: &str,
        text: &str,
    ) -> Result<()> {
        validate_service_url(service_url)?;
        let base = ensure_trailing_slash(service_url);

        let chunks = chunker::chunk_text(text, MESSAGE_CHUNK_SIZE);
        let chunks = if chunks.is_empty() {
            vec![text.to_string()]
        } else {
            chunks
        };

        for chunk in &chunks {
            let url = format!("{base}v3/conversations/{conversation_id}/activities/{activity_id}");
            let reply = ReplyActivity::message(chunk.as_str());
            self.send_with_retry(&url, &reply).await?;
        }

        Ok(())
    }

    /// Send a message to a Teams conversation (not a reply to a specific activity).
    /// Automatically chunks long messages.
    pub async fn send_to_conversation(
        &self,
        service_url: &str,
        conversation_id: &str,
        text: &str,
    ) -> Result<()> {
        validate_service_url(service_url)?;
        let base = ensure_trailing_slash(service_url);

        let chunks = chunker::chunk_text(text, MESSAGE_CHUNK_SIZE);
        let chunks = if chunks.is_empty() {
            vec![text.to_string()]
        } else {
            chunks
        };

        for chunk in &chunks {
            let url = format!("{base}v3/conversations/{conversation_id}/activities");
            let reply = ReplyActivity::message(chunk.as_str());
            self.send_with_retry(&url, &reply).await?;
        }

        Ok(())
    }

    /// Send a single request with 429 retry logic.
    async fn send_with_retry(&self, url: &str, body: &ReplyActivity) -> Result<()> {
        let mut attempts = 0u32;

        loop {
            let token = self.get_token().await?;

            let resp = self
                .client
                .post(url)
                .bearer_auth(&token)
                .json(body)
                .send()
                .await
                .context("Failed to send Teams message")?;

            let status = resp.status();

            if status.is_success() {
                return Ok(());
            }

            // Handle 429 rate limiting
            if status.as_u16() == 429 {
                attempts += 1;
                if attempts > MAX_RETRIES {
                    bail!("Teams API rate limited after {MAX_RETRIES} retries");
                }
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1);
                let capped = retry_after.min(MAX_RETRY_AFTER_SECS);
                warn!(
                    "Teams rate limited, retry after {capped}s (attempt {attempts}/{MAX_RETRIES})"
                );
                tokio::time::sleep(Duration::from_secs(capped)).await;
                continue;
            }

            let error_body = resp.text().await.unwrap_or_default();
            bail!("Teams API request failed ({status}): {error_body}");
        }
    }
}

/// Validate that a service URL is a legitimate Microsoft Bot Framework endpoint.
/// Prevents SSRF by requiring HTTPS and a known Microsoft host suffix.
fn validate_service_url(url: &str) -> Result<()> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| anyhow::anyhow!("Invalid service URL: {e}"))?;

    if parsed.scheme() != "https" {
        bail!("Service URL must use HTTPS: {url}");
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Service URL has no host: {url}"))?;

    let is_allowed = ALLOWED_HOST_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix));

    if !is_allowed {
        bail!("Service URL host not allowed: {host}");
    }

    Ok(())
}

/// Ensure a URL has a trailing slash for path concatenation.
fn ensure_trailing_slash(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_construction_reply() {
        let base = ensure_trailing_slash("https://smba.trafficmanager.net/teams/");
        let url = format!(
            "{base}v3/conversations/{conv}/activities/{act}",
            conv = "c1",
            act = "a1"
        );
        assert_eq!(
            url,
            "https://smba.trafficmanager.net/teams/v3/conversations/c1/activities/a1"
        );
    }

    #[test]
    fn url_construction_send() {
        let base = ensure_trailing_slash("https://smba.trafficmanager.net/teams");
        let url = format!("{base}v3/conversations/{conv}/activities", conv = "c1");
        assert_eq!(
            url,
            "https://smba.trafficmanager.net/teams/v3/conversations/c1/activities"
        );
    }

    #[test]
    fn validate_service_url_valid_botframework() {
        assert!(validate_service_url("https://smba.trafficmanager.net.botframework.com/").is_ok());
    }

    #[test]
    fn validate_service_url_valid_teams() {
        assert!(validate_service_url("https://api.teams.microsoft.com/").is_ok());
    }

    #[test]
    fn validate_service_url_valid_skype() {
        assert!(validate_service_url("https://smba.skype.com/teams/").is_ok());
    }

    #[test]
    fn validate_service_url_rejects_http() {
        let result = validate_service_url("http://smba.trafficmanager.net.botframework.com/");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("HTTPS"));
    }

    #[test]
    fn validate_service_url_rejects_unknown_host() {
        let result = validate_service_url("https://evil.example.com/");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not allowed"));
    }

    #[test]
    fn validate_service_url_rejects_invalid_url() {
        let result = validate_service_url("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn trailing_slash_added() {
        assert_eq!(
            ensure_trailing_slash("https://example.com"),
            "https://example.com/"
        );
    }

    #[test]
    fn trailing_slash_not_doubled() {
        assert_eq!(
            ensure_trailing_slash("https://example.com/"),
            "https://example.com/"
        );
    }

    #[test]
    fn chunking_integration() {
        let long_text: String = "a".repeat(8500);
        let chunks = chunker::chunk_text(&long_text, MESSAGE_CHUNK_SIZE);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 4000);
        assert_eq!(chunks[1].len(), 4000);
        assert_eq!(chunks[2].len(), 500);
    }

    #[test]
    fn token_endpoint_url() {
        assert_eq!(
            TOKEN_ENDPOINT,
            "https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token"
        );
    }
}
