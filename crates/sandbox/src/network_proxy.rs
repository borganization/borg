//! Lightweight HTTP proxy for domain-level network filtering in sandboxed tools.
//!
//! The proxy listens on a random local port and only allows connections to domains
//! in the allowlist. Sandboxed tools are configured to use this proxy via
//! `http_proxy` / `https_proxy` environment variables.
//!
//! Supports:
//! - HTTP CONNECT tunneling (for HTTPS)
//! - Plain HTTP forwarding (via Host header)

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, warn};

/// Configuration for the network proxy.
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Allowed domain names (e.g., "api.github.com", "pypi.org").
    pub allowed_domains: HashSet<String>,
}

/// A running network proxy instance.
pub struct NetworkProxy {
    addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

impl NetworkProxy {
    /// Start a new network proxy on a random local port.
    /// Returns the proxy instance with its bound address.
    pub async fn start(config: ProxyConfig) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let config = Arc::new(config);

        debug!("Network proxy listening on {addr}");

        let handle = tokio::spawn(async move {
            run_proxy(listener, config, shutdown_rx).await;
        });

        Ok(Self {
            addr,
            shutdown: shutdown_tx,
            handle,
        })
    }

    /// The local address the proxy is listening on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The proxy URL for use in http_proxy/https_proxy env vars.
    pub fn proxy_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Environment variables to set on sandboxed tool processes.
    pub fn env_vars(&self) -> Vec<(String, String)> {
        let url = self.proxy_url();
        vec![
            ("http_proxy".to_string(), url.clone()),
            ("https_proxy".to_string(), url.clone()),
            ("HTTP_PROXY".to_string(), url.clone()),
            ("HTTPS_PROXY".to_string(), url),
        ]
    }

    /// Shut down the proxy.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.handle.await;
    }
}

async fn run_proxy(
    listener: TcpListener,
    config: Arc<ProxyConfig>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer)) => {
                        let config = config.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, peer, &config).await {
                                debug!("Proxy connection from {peer} error: {e}");
                            }
                        });
                    }
                    Err(e) => {
                        warn!("Proxy accept error: {e}");
                    }
                }
            }
            _ = shutdown.changed() => {
                debug!("Network proxy shutting down");
                return;
            }
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    config: &ProxyConfig,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }

    let method = parts[0];
    let target = parts[1];

    if method.eq_ignore_ascii_case("CONNECT") {
        handle_connect(reader, peer, target, config).await
    } else {
        handle_http(reader, peer, &request_line, config).await
    }
}

/// Handle HTTPS CONNECT tunneling.
async fn handle_connect(
    mut reader: BufReader<TcpStream>,
    peer: SocketAddr,
    target: &str,
    config: &ProxyConfig,
) -> anyhow::Result<()> {
    // target is "host:port"
    let domain = extract_domain(target);

    // Drain remaining headers
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
    }

    if !is_domain_allowed(&domain, &config.allowed_domains) {
        debug!("Proxy blocked CONNECT to {domain} from {peer}");
        let response = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
        reader.get_mut().write_all(response.as_bytes()).await?;
        return Ok(());
    }

    debug!("Proxy allowing CONNECT to {target} from {peer}");

    // Connect to target
    match TcpStream::connect(target).await {
        Ok(upstream) => {
            let response = "HTTP/1.1 200 Connection Established\r\n\r\n";
            reader.get_mut().write_all(response.as_bytes()).await?;

            // Tunnel bidirectionally
            let mut client = reader.into_inner();
            let (mut client_read, mut client_write) = client.split();
            let (mut upstream_read, mut upstream_write) = upstream.into_split();

            tokio::select! {
                r = tokio::io::copy(&mut client_read, &mut upstream_write) => { let _ = r; }
                r = tokio::io::copy(&mut upstream_read, &mut client_write) => { let _ = r; }
            }
        }
        Err(e) => {
            let response = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{}",
                e.to_string().len(),
                e
            );
            reader.get_mut().write_all(response.as_bytes()).await?;
        }
    }

    Ok(())
}

/// Handle plain HTTP forwarding.
async fn handle_http(
    mut reader: BufReader<TcpStream>,
    peer: SocketAddr,
    request_line: &str,
    config: &ProxyConfig,
) -> anyhow::Result<()> {
    // Read headers to find Host
    let mut headers = Vec::new();
    let mut host = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("host:") {
            host = line
                .split_once(':')
                .map(|(_, v)| v.trim().to_string())
                .unwrap_or_default();
        }
        headers.push(line);
    }

    // Extract domain from Host header or URL
    let domain = if !host.is_empty() {
        extract_domain(&host)
    } else {
        // Try to extract from absolute URL
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() >= 2 {
            extract_domain_from_url(parts[1])
        } else {
            String::new()
        }
    };

    if !is_domain_allowed(&domain, &config.allowed_domains) {
        debug!("Proxy blocked HTTP request to {domain} from {peer}");
        let response = "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n";
        reader.get_mut().write_all(response.as_bytes()).await?;
        return Ok(());
    }

    debug!("Proxy allowing HTTP request to {domain} from {peer}");

    // Forward the request
    let port = extract_port(&host, 80);
    let target = format!("{domain}:{port}");
    match TcpStream::connect(&target).await {
        Ok(mut upstream) => {
            // Send original request line and headers
            upstream.write_all(request_line.as_bytes()).await?;
            for header in &headers {
                upstream.write_all(header.as_bytes()).await?;
            }
            upstream.write_all(b"\r\n").await?;

            // Bidirectional copy
            let mut client = reader.into_inner();
            let (mut client_read, mut client_write) = client.split();
            let (mut upstream_read, mut upstream_write) = upstream.into_split();

            tokio::select! {
                r = tokio::io::copy(&mut client_read, &mut upstream_write) => { let _ = r; }
                r = tokio::io::copy(&mut upstream_read, &mut client_write) => { let _ = r; }
            }
        }
        Err(e) => {
            let body = format!("Failed to connect to {target}: {e}");
            let response = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            reader.get_mut().write_all(response.as_bytes()).await?;
        }
    }

    Ok(())
}

/// Extract the domain from "host:port" or just "host".
fn extract_domain(host_port: &str) -> String {
    host_port
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// Extract the domain from an absolute URL like "http://example.com/path".
fn extract_domain_from_url(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host_part = without_scheme.split('/').next().unwrap_or("");
    extract_domain(host_part)
}

/// Extract the port from "host:port", defaulting to `default_port`.
fn extract_port(host_port: &str, default_port: u16) -> u16 {
    host_port
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(default_port)
}

/// Check if a domain is in the allowlist.
/// Supports exact match and wildcard suffix match (e.g., ".github.com" matches "api.github.com").
fn is_domain_allowed(domain: &str, allowed: &HashSet<String>) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let domain = domain.to_lowercase();
    if allowed.contains(&domain) {
        return true;
    }
    // Check wildcard suffixes (entries starting with ".")
    for pattern in allowed {
        if pattern.starts_with('.') && domain.ends_with(pattern.as_str()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_domain_host_port() {
        assert_eq!(extract_domain("api.github.com:443"), "api.github.com");
    }

    #[test]
    fn extract_domain_host_only() {
        assert_eq!(extract_domain("example.com"), "example.com");
    }

    #[test]
    fn extract_domain_case_insensitive() {
        assert_eq!(extract_domain("API.GitHub.COM:443"), "api.github.com");
    }

    #[test]
    fn extract_domain_from_url_http() {
        assert_eq!(
            extract_domain_from_url("http://example.com/path"),
            "example.com"
        );
    }

    #[test]
    fn extract_domain_from_url_https() {
        assert_eq!(
            extract_domain_from_url("https://api.github.com/repos"),
            "api.github.com"
        );
    }

    #[test]
    fn extract_domain_from_url_with_port() {
        assert_eq!(
            extract_domain_from_url("http://localhost:8080/api"),
            "localhost"
        );
    }

    #[test]
    fn extract_port_with_port() {
        assert_eq!(extract_port("example.com:8080", 80), 8080);
    }

    #[test]
    fn extract_port_default() {
        assert_eq!(extract_port("example.com", 80), 80);
    }

    #[test]
    fn domain_allowed_exact() {
        let mut allowed = HashSet::new();
        allowed.insert("api.github.com".to_string());
        assert!(is_domain_allowed("api.github.com", &allowed));
    }

    #[test]
    fn domain_not_allowed() {
        let mut allowed = HashSet::new();
        allowed.insert("api.github.com".to_string());
        assert!(!is_domain_allowed("evil.com", &allowed));
    }

    #[test]
    fn domain_allowed_wildcard_suffix() {
        let mut allowed = HashSet::new();
        allowed.insert(".github.com".to_string());
        assert!(is_domain_allowed("api.github.com", &allowed));
        assert!(is_domain_allowed("raw.github.com", &allowed));
        assert!(!is_domain_allowed("github.com", &allowed));
    }

    #[test]
    fn domain_allowed_empty_allowlist() {
        let allowed = HashSet::new();
        assert!(!is_domain_allowed("anything.com", &allowed));
    }

    #[test]
    fn domain_allowed_case_insensitive() {
        let mut allowed = HashSet::new();
        allowed.insert("api.github.com".to_string());
        assert!(is_domain_allowed("API.GitHub.COM", &allowed));
    }

    #[tokio::test]
    async fn proxy_starts_and_stops() {
        let config = ProxyConfig {
            allowed_domains: HashSet::new(),
        };
        let proxy = NetworkProxy::start(config).await.unwrap();
        assert!(proxy.addr().port() > 0);
        assert!(proxy.proxy_url().starts_with("http://127.0.0.1:"));
        proxy.shutdown().await;
    }

    #[tokio::test]
    async fn proxy_env_vars() {
        let config = ProxyConfig {
            allowed_domains: HashSet::new(),
        };
        let proxy = NetworkProxy::start(config).await.unwrap();
        let env = proxy.env_vars();
        assert_eq!(env.len(), 4);
        assert!(env.iter().any(|(k, _)| k == "http_proxy"));
        assert!(env.iter().any(|(k, _)| k == "https_proxy"));
        assert!(env.iter().any(|(k, _)| k == "HTTP_PROXY"));
        assert!(env.iter().any(|(k, _)| k == "HTTPS_PROXY"));
        proxy.shutdown().await;
    }

    #[tokio::test]
    async fn proxy_blocks_unlisted_domain() {
        let config = ProxyConfig {
            allowed_domains: HashSet::from(["allowed.com".to_string()]),
        };
        let proxy = NetworkProxy::start(config).await.unwrap();
        let addr = proxy.addr();

        // Connect and send a CONNECT to a blocked domain
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"CONNECT blocked.com:443 HTTP/1.1\r\nHost: blocked.com\r\n\r\n")
            .await
            .unwrap();

        let mut buf = vec![0u8; 1024];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf)
            .await
            .unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(
            response.contains("403 Forbidden"),
            "Expected 403, got: {response}"
        );

        proxy.shutdown().await;
    }
}
