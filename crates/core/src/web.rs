use anyhow::{Context, Result};
use scraper::{Html, Selector};
use std::sync::LazyLock;

use crate::config::WebConfig;

const MAX_FETCH_CHARS: usize = 50000;
const MAX_SEARCH_RESULTS: usize = 8;
const USER_AGENT: &str = "Mozilla/5.0 (compatible; Borg/0.1)";

/// Fetch a URL and return its text content. HTML is stripped to plain text.
pub async fn web_fetch(url: &str, max_chars: Option<usize>) -> Result<String> {
    let max = max_chars.unwrap_or(MAX_FETCH_CHARS);

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status} fetching {url}");
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("Failed to read body from {url}"))?;
    const MAX_BODY_BYTES: usize = 10 * 1024 * 1024; // 10 MB
    if bytes.len() > MAX_BODY_BYTES {
        anyhow::bail!(
            "Response body too large ({} bytes, max {MAX_BODY_BYTES})",
            bytes.len()
        );
    }
    let body = String::from_utf8_lossy(&bytes).into_owned();

    let text = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        html_to_text(&body)
    } else {
        body
    };

    let char_count = text.chars().count();
    if char_count > max {
        let truncated: String = text.chars().take(max).collect();
        Ok(format!(
            "{truncated}\n\n[truncated — showing {max} of {char_count} chars]"
        ))
    } else {
        Ok(text)
    }
}

/// Search the web and return formatted results.
pub async fn web_search(query: &str, config: &WebConfig) -> Result<String> {
    match config.search_provider.as_str() {
        "brave" => brave_search(query, config).await,
        _ => duckduckgo_search(query).await,
    }
}

/// Search via DuckDuckGo HTML endpoint (no API key needed).
async fn duckduckgo_search(query: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .send()
        .await
        .context("Failed to reach DuckDuckGo")?;

    let html = resp.text().await?;
    let document = Html::parse_document(&html);

    static RESULT_SEL: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(".result").unwrap_or_else(|_| panic!("bad selector")));
    static TITLE_SEL: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse(".result__a").unwrap_or_else(|_| panic!("bad selector")));
    static SNIPPET_SEL: LazyLock<Selector> = LazyLock::new(|| {
        Selector::parse(".result__snippet").unwrap_or_else(|_| panic!("bad selector"))
    });
    let result_selector = &*RESULT_SEL;
    let title_selector = &*TITLE_SEL;
    let snippet_selector = &*SNIPPET_SEL;

    let mut results = Vec::new();
    for (i, result) in document.select(result_selector).enumerate() {
        if i >= MAX_SEARCH_RESULTS {
            break;
        }

        let title: String = result
            .select(title_selector)
            .next()
            .map(|e| e.text().collect())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let url: String = result
            .select(title_selector)
            .next()
            .and_then(|e| e.value().attr("href"))
            .unwrap_or("")
            .to_string();

        let snippet: String = result
            .select(snippet_selector)
            .next()
            .map(|e| e.text().collect())
            .unwrap_or_default();

        results.push(format!(
            "{}. {}\n   URL: {}\n   {}",
            results.len() + 1,
            title.trim(),
            url,
            snippet.trim()
        ));
    }

    if results.is_empty() {
        Ok(format!("No search results found for: {query}"))
    } else {
        Ok(format!(
            "Search results for \"{query}\":\n\n{}",
            results.join("\n\n")
        ))
    }
}

/// Search via Brave Search API (requires API key).
async fn brave_search(query: &str, config: &WebConfig) -> Result<String> {
    let api_key_env = config
        .search_api_key_env
        .as_deref()
        .unwrap_or("BRAVE_SEARCH_API_KEY");
    let api_key = std::env::var(api_key_env)
        .with_context(|| format!("Brave Search API key not found. Set {api_key_env}"))?;

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .query(&[("q", query), ("count", "8")])
        .send()
        .await
        .context("Failed to reach Brave Search API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Brave Search returned HTTP {status}: {body}");
    }

    let body: serde_json::Value = resp.json().await?;
    let mut results = Vec::new();

    if let Some(web_results) = body["web"]["results"].as_array() {
        for (i, r) in web_results.iter().enumerate() {
            if i >= MAX_SEARCH_RESULTS {
                break;
            }
            let title = r["title"].as_str().unwrap_or("");
            let url = r["url"].as_str().unwrap_or("");
            let desc = r["description"].as_str().unwrap_or("");
            results.push(format!("{}. {title}\n   URL: {url}\n   {desc}", i + 1));
        }
    }

    if results.is_empty() {
        Ok(format!("No search results found for: {query}"))
    } else {
        Ok(format!(
            "Search results for \"{query}\":\n\n{}",
            results.join("\n\n")
        ))
    }
}

/// Extract readable text from HTML, trying to focus on main content.
fn html_to_text(html: &str) -> String {
    let document = Html::parse_document(html);

    // Try content-rich selectors first
    let content_selectors = ["main", "article", "#content", ".content", "body"];
    for sel_str in &content_selectors {
        if let Ok(sel) = Selector::parse(sel_str) {
            if let Some(el) = document.select(&sel).next() {
                let text = extract_text(&el);
                if text.len() > 100 {
                    return text;
                }
            }
        }
    }

    // Fallback: all text from root
    let text: String = document.root_element().text().collect::<Vec<_>>().join(" ");
    collapse_whitespace(&text)
}

/// Extract text from an element, skipping script/style/noscript content.
fn extract_text(element: &scraper::ElementRef<'_>) -> String {
    use scraper::node::Node;

    let skip_tags: &[&str] = &["script", "style", "noscript"];

    let mut parts = Vec::new();
    for node in element.descendants() {
        if let Node::Text(text_node) = node.value() {
            // Check if any ancestor is a script/style/noscript element
            let mut in_skip = false;
            let mut ancestor = node.parent();
            while let Some(a) = ancestor {
                if let Node::Element(el) = a.value() {
                    if skip_tags.contains(&el.name.local.as_ref()) {
                        in_skip = true;
                        break;
                    }
                }
                ancestor = a.parent();
            }
            if !in_skip {
                let trimmed = text_node.text.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
        }
    }
    parts.join(" ")
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_basic() {
        let html = r#"<html><body><h1>Hello</h1><p>World</p></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn html_to_text_strips_tags() {
        let html = r#"<html><body><p>This is <b>bold</b> and <i>italic</i>.</p></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("bold") && text.contains("italic"));
    }

    #[test]
    fn html_to_text_handles_empty() {
        let text = html_to_text("");
        assert!(text.is_empty() || text.trim().is_empty());
    }

    #[test]
    fn collapse_whitespace_works() {
        assert_eq!(collapse_whitespace("  hello   world  "), "hello world");
    }
}
