use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::Page;
use futures::StreamExt;

use crate::config::BrowserConfig as BorgBrowserConfig;

/// Result of Chrome/Chromium executable detection.
pub struct ChromeDetection {
    /// The best candidate executable (first found in priority order).
    pub executable: Option<PathBuf>,
    /// All Chrome-like executables found on this system.
    pub all_found: Vec<PathBuf>,
}

/// Detect Chrome/Chromium executables on the system.
///
/// If `configured_path` is provided and the file exists, it is used as the primary
/// executable. Otherwise, platform-specific detection runs through known paths.
pub fn find_chrome(configured_path: Option<&str>) -> ChromeDetection {
    let mut all_found = Vec::new();

    // If a configured path is provided and exists, use it as primary.
    if let Some(path) = configured_path {
        let p = PathBuf::from(path);
        if p.exists() {
            all_found.push(p.clone());
            return ChromeDetection {
                executable: Some(p),
                all_found,
            };
        }
    }

    // Platform-specific known paths
    let candidates = platform_candidates();

    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            all_found.push(p);
        }
    }

    // Also check PATH via `which`
    let which_names = which_candidates();
    for name in &which_names {
        if let Ok(p) = which::which(name) {
            if !all_found.contains(&p) {
                all_found.push(p);
            }
        }
    }

    let executable = all_found.first().cloned();
    ChromeDetection {
        executable,
        all_found,
    }
}

/// Check whether the `agent-browser` CLI is available on PATH.
pub fn detect_agent_browser() -> bool {
    which::which("agent-browser").is_ok()
}

/// Persistent CDP browser session. Lazy-launched on first `browser` tool call.
pub struct BrowserSession {
    #[allow(dead_code)]
    browser: Browser,
    page: Page,
    _event_handle: tokio::task::JoinHandle<()>,
}

impl BrowserSession {
    /// Launch Chrome and open a default page.
    pub async fn launch(config: &BorgBrowserConfig) -> Result<Self> {
        let detection = find_chrome(config.executable.as_deref());
        let chrome_path = detection.executable.context(
            "No Chrome/Chromium found. Install Chrome or set browser.executable in config.",
        )?;

        let mut builder = BrowserConfig::builder();
        builder = builder.chrome_executable(chrome_path);

        if config.headless {
            builder = builder.arg("--headless=new");
        }

        if config.no_sandbox {
            builder = builder.no_sandbox();
        }

        builder = builder
            .arg(format!("--remote-debugging-port={}", config.cdp_port))
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage");

        let browser_config = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build browser config: {e}"))?;

        let launch_timeout = Duration::from_millis(config.startup_timeout_ms);
        let (browser, mut handler) =
            tokio::time::timeout(launch_timeout, Browser::launch(browser_config))
                .await
                .context("Browser launch timed out")?
                .context("Failed to launch browser")?;

        let event_handle = tokio::spawn(async move { while handler.next().await.is_some() {} });

        let page = browser
            .new_page("about:blank")
            .await
            .context("Failed to create initial page")?;

        Ok(Self {
            browser,
            page,
            _event_handle: event_handle,
        })
    }

    /// Navigate to a URL. Returns page title and URL.
    /// Only http/https URLs are allowed for security.
    pub async fn navigate(&self, url: &str, timeout: Duration) -> Result<String> {
        // Validate URL scheme to prevent file://, javascript:, chrome:// access
        let scheme = url.split(':').next().unwrap_or("");
        if !matches!(scheme, "http" | "https") {
            anyhow::bail!("Blocked URL scheme: {scheme}. Only http/https allowed.");
        }

        tokio::time::timeout(timeout, async {
            self.page.goto(url).await.context("Navigation failed")?;
            let title = self
                .page
                .get_title()
                .await
                .context("Failed to get title")?
                .unwrap_or_default();
            let current_url = self
                .page
                .url()
                .await
                .context("Failed to get URL")?
                .unwrap_or_default();
            Ok(format!("Navigated to {current_url}\nTitle: {title}"))
        })
        .await
        .context("Navigate timed out")?
    }

    /// Click an element by CSS selector.
    pub async fn click(&self, selector: &str, timeout: Duration) -> Result<String> {
        tokio::time::timeout(timeout, async {
            let el = self
                .page
                .find_element(selector)
                .await
                .with_context(|| format!("Element not found: {selector}"))?;
            el.click().await.context("Click failed")?;
            Ok(format!("Clicked element: {selector}"))
        })
        .await
        .context("Click timed out")?
    }

    /// Type text into an element by CSS selector.
    pub async fn type_text(&self, selector: &str, text: &str, timeout: Duration) -> Result<String> {
        tokio::time::timeout(timeout, async {
            let el = self
                .page
                .find_element(selector)
                .await
                .with_context(|| format!("Element not found: {selector}"))?;
            el.click().await.context("Focus click failed")?;
            el.type_str(text).await.context("Type failed")?;
            Ok(format!("Typed into {selector}"))
        })
        .await
        .context("Type timed out")?
    }

    /// Take a screenshot. Returns (description, png_bytes).
    pub async fn screenshot(
        &self,
        selector: Option<&str>,
        timeout: Duration,
    ) -> Result<(String, Vec<u8>)> {
        tokio::time::timeout(timeout, async {
            if let Some(sel) = selector {
                let el = self
                    .page
                    .find_element(sel)
                    .await
                    .with_context(|| format!("Element not found: {sel}"))?;
                let bytes = el
                    .screenshot(CaptureScreenshotFormat::Png)
                    .await
                    .context("Element screenshot failed")?;
                Ok((format!("Screenshot of element: {sel}"), bytes))
            } else {
                let bytes = self
                    .page
                    .screenshot(
                        chromiumoxide::page::ScreenshotParams::builder()
                            .format(CaptureScreenshotFormat::Png)
                            .build(),
                    )
                    .await
                    .context("Page screenshot failed")?;
                Ok(("Full page screenshot".to_string(), bytes))
            }
        })
        .await
        .context("Screenshot timed out")?
    }

    /// Get text content from the page or a specific element.
    pub async fn get_text(&self, selector: Option<&str>, timeout: Duration) -> Result<String> {
        tokio::time::timeout(timeout, async {
            if let Some(sel) = selector {
                let el = self
                    .page
                    .find_element(sel)
                    .await
                    .with_context(|| format!("Element not found: {sel}"))?;
                let text = el
                    .inner_text()
                    .await
                    .context("Failed to get inner text")?
                    .unwrap_or_default();
                Ok(text)
            } else {
                let text = self
                    .page
                    .find_element("body")
                    .await
                    .context("No body element")?
                    .inner_text()
                    .await
                    .context("Failed to get body text")?
                    .unwrap_or_default();
                Ok(text)
            }
        })
        .await
        .context("Get text timed out")?
    }

    /// Evaluate a JavaScript expression and return the result.
    pub async fn evaluate_js(&self, expression: &str, timeout: Duration) -> Result<String> {
        tokio::time::timeout(timeout, async {
            let result: serde_json::Value = self
                .page
                .evaluate(expression)
                .await
                .context("JS evaluation failed")?
                .into_value()
                .context("Failed to deserialize JS result")?;
            Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
        })
        .await
        .context("Evaluate JS timed out")?
    }

    /// Close the browser and abort the event handler.
    pub async fn close(self) -> Result<()> {
        // Drop browser first to send CDP Browser.close, then abort the event loop
        drop(self.browser);
        self._event_handle.abort();
        Ok(())
    }
}

/// Validate arguments for a browser action. Returns an error message if invalid.
pub fn validate_browser_args(action: &str, args: &serde_json::Value) -> Option<String> {
    match action {
        "navigate" => {
            if args.get("url").and_then(|v| v.as_str()).is_none() {
                return Some("navigate requires 'url' parameter".to_string());
            }
        }
        "click" => {
            if args.get("selector").and_then(|v| v.as_str()).is_none() {
                return Some("click requires 'selector' parameter".to_string());
            }
        }
        "type" => {
            if args.get("selector").and_then(|v| v.as_str()).is_none() {
                return Some("type requires 'selector' parameter".to_string());
            }
            if args.get("text").and_then(|v| v.as_str()).is_none() {
                return Some("type requires 'text' parameter".to_string());
            }
        }
        "evaluate_js" => {
            if args.get("expression").and_then(|v| v.as_str()).is_none() {
                return Some("evaluate_js requires 'expression' parameter".to_string());
            }
        }
        "screenshot" | "get_text" | "close" => {}
        _ => {
            return Some(format!("Unknown browser action: {action}"));
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn platform_candidates() -> Vec<&'static str> {
    vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ]
}

#[cfg(target_os = "linux")]
fn platform_candidates() -> Vec<&'static str> {
    vec![]
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_candidates() -> Vec<&'static str> {
    vec![]
}

#[cfg(target_os = "macos")]
fn which_candidates() -> Vec<&'static str> {
    vec!["google-chrome", "chromium"]
}

#[cfg(target_os = "linux")]
fn which_candidates() -> Vec<&'static str> {
    vec![
        "google-chrome-stable",
        "google-chrome",
        "chromium-browser",
        "chromium",
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn which_candidates() -> Vec<&'static str> {
    vec!["google-chrome", "chromium"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_agent_browser_returns_bool() {
        // Should not panic regardless of whether agent-browser is installed.
        let _result = detect_agent_browser();
    }

    #[test]
    fn find_chrome_with_no_configured_path() {
        let detection = find_chrome(None);
        // Struct should be well-formed; executable may or may not be present.
        if let Some(ref exe) = detection.executable {
            assert!(detection.all_found.contains(exe));
        }
    }

    #[test]
    fn find_chrome_with_invalid_configured_path() {
        let detection = find_chrome(Some("/nonexistent/path/to/chrome"));
        // Should fall through gracefully to platform detection.
        assert!(!detection
            .all_found
            .contains(&PathBuf::from("/nonexistent/path/to/chrome")));
    }

    #[test]
    fn find_chrome_with_valid_configured_path() {
        // Use /bin/sh as a stand-in for a valid executable path.
        let detection = find_chrome(Some("/bin/sh"));
        assert_eq!(detection.executable, Some(PathBuf::from("/bin/sh")));
        assert!(detection.all_found.contains(&PathBuf::from("/bin/sh")));
    }

    #[test]
    fn validate_navigate_requires_url() {
        let args = serde_json::json!({"action": "navigate"});
        assert!(validate_browser_args("navigate", &args).is_some());

        let args = serde_json::json!({"action": "navigate", "url": "https://example.com"});
        assert!(validate_browser_args("navigate", &args).is_none());
    }

    #[test]
    fn validate_click_requires_selector() {
        let args = serde_json::json!({"action": "click"});
        assert!(validate_browser_args("click", &args).is_some());

        let args = serde_json::json!({"action": "click", "selector": "#btn"});
        assert!(validate_browser_args("click", &args).is_none());
    }

    #[test]
    fn validate_type_requires_selector_and_text() {
        let args = serde_json::json!({"action": "type", "selector": "#input"});
        assert!(validate_browser_args("type", &args).is_some());

        let args = serde_json::json!({"action": "type", "text": "hello"});
        assert!(validate_browser_args("type", &args).is_some());

        let args = serde_json::json!({"action": "type", "selector": "#input", "text": "hello"});
        assert!(validate_browser_args("type", &args).is_none());
    }

    #[test]
    fn validate_evaluate_js_requires_expression() {
        let args = serde_json::json!({"action": "evaluate_js"});
        assert!(validate_browser_args("evaluate_js", &args).is_some());

        let args = serde_json::json!({"action": "evaluate_js", "expression": "1+1"});
        assert!(validate_browser_args("evaluate_js", &args).is_none());
    }

    #[test]
    fn validate_screenshot_no_required_params() {
        let args = serde_json::json!({"action": "screenshot"});
        assert!(validate_browser_args("screenshot", &args).is_none());
    }

    #[test]
    fn validate_unknown_action() {
        let args = serde_json::json!({});
        assert!(validate_browser_args("unknown_action", &args).is_some());
    }

    #[tokio::test]
    #[ignore] // Requires Chrome installed
    async fn browser_session_launch_fails_without_chrome() {
        let config = BorgBrowserConfig {
            enabled: true,
            headless: true,
            executable: Some("/nonexistent/chrome".to_string()),
            cdp_port: 0,
            no_sandbox: false,
            timeout_ms: 5000,
            startup_timeout_ms: 5000,
        };
        let result = BrowserSession::launch(&config).await;
        assert!(result.is_err());
    }
}
