use serde_json::Value;

/// Send an HTTP request, returning an error if the response status is not successful.
pub async fn send_and_check(
    req: reqwest::RequestBuilder,
    service: &str,
) -> Result<reqwest::Response, String> {
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("{service} API error: {text}"));
    }
    Ok(resp)
}

/// Send an HTTP request and parse the response as JSON.
pub async fn send_json(req: reqwest::RequestBuilder, service: &str) -> Result<Value, String> {
    let resp = send_and_check(req, service).await?;
    resp.json().await.map_err(|e| format!("Parse error: {e}"))
}
