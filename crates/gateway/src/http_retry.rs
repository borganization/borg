use std::time::Duration;

use anyhow::{bail, Result};
use tracing::warn;

/// Configuration for HTTP 429 rate-limit retry behavior.
pub struct RateLimitPolicy {
    pub max_retries: u32,
    pub max_retry_after_secs: u64,
    pub service_name: &'static str,
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            max_retry_after_secs: 300,
            service_name: "API",
        }
    }
}

/// Send an HTTP request with automatic 429 rate-limit retry.
///
/// `make_request` is called on each attempt and must return a fresh `reqwest::Response`.
/// On 429, parses the `Retry-After` header, sleeps, and retries up to `policy.max_retries`.
/// Returns the first non-429 response, or errors after exhausting retries.
pub async fn send_with_rate_limit_retry<F, Fut>(
    policy: &RateLimitPolicy,
    mut make_request: F,
) -> Result<reqwest::Response>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<reqwest::Response>>,
{
    let mut attempts = 0u32;

    loop {
        let resp = make_request().await?;

        if resp.status().as_u16() != 429 {
            return Ok(resp);
        }

        attempts += 1;
        if attempts > policy.max_retries {
            bail!(
                "{} rate limited after {} retries",
                policy.service_name,
                policy.max_retries
            );
        }

        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1);
        let capped = retry_after.min(policy.max_retry_after_secs);
        warn!(
            "{} rate limited, retry after {capped}s (attempt {attempts}/{})",
            policy.service_name, policy.max_retries
        );
        tokio::time::sleep(Duration::from_secs(capped)).await;
    }
}
