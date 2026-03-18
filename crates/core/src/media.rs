use crate::types::MediaData;
use anyhow::Result;

/// Parse a data URI like `data:image/png;base64,abc123` into a MediaData struct.
///
/// Validates the `;base64` marker is present and the data portion is non-empty.
pub fn parse_data_uri(uri: &str) -> Result<MediaData> {
    let rest = uri
        .strip_prefix("data:")
        .ok_or_else(|| anyhow::anyhow!("not a data URI"))?;
    let (header, data) = rest
        .split_once(",")
        .ok_or_else(|| anyhow::anyhow!("malformed data URI: missing comma"))?;
    if !header.contains(";base64") {
        anyhow::bail!("data URI missing ;base64 marker");
    }
    if data.is_empty() {
        anyhow::bail!("data URI has empty data");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_data_uri() {
        let uri = "data:image/png;base64,abc123";
        let media = parse_data_uri(uri).unwrap();
        assert_eq!(media.mime_type, "image/png");
        assert_eq!(media.data, "abc123");
        assert!(media.filename.is_none());
    }

    #[test]
    fn parse_rejects_missing_prefix() {
        assert!(parse_data_uri("not-a-data-uri").is_err());
    }

    #[test]
    fn parse_rejects_missing_comma() {
        assert!(parse_data_uri("data:image/png;base64").is_err());
    }

    #[test]
    fn parse_rejects_missing_base64_marker() {
        assert!(parse_data_uri("data:text/plain,hello").is_err());
    }

    #[test]
    fn parse_rejects_empty_data() {
        assert!(parse_data_uri("data:image/png;base64,").is_err());
    }
}
