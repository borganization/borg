use crate::types::MediaData;
use anyhow::Result;

/// Parse a data URI like `data:image/png;base64,abc123` into a MediaData struct.
pub fn parse_data_uri(uri: &str) -> Result<MediaData> {
    let rest = uri
        .strip_prefix("data:")
        .ok_or_else(|| anyhow::anyhow!("not a data URI"))?;
    let (header, data) = rest
        .split_once(",")
        .ok_or_else(|| anyhow::anyhow!("malformed data URI"))?;
    let mime_type = header
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .to_string();
    Ok(MediaData {
        mime_type,
        data: data.to_string(),
        filename: None,
    })
}
