use anyhow::Result;

use super::types::{TwilioChannelType, TwilioWebhook};
use crate::handler::InboundMessage;

/// Parsed Twilio inbound message with typed channel information.
pub struct TwilioInbound {
    pub message: InboundMessage,
    pub channel_type: TwilioChannelType,
    /// Audio media URL for transcription (if the message contains audio).
    pub audio_url: Option<String>,
    /// MIME type of the audio media.
    pub audio_mime: Option<String>,
}

/// Parse a Twilio webhook body (form-urlencoded) into a typed inbound message.
pub fn parse_webhook(body: &str) -> Result<TwilioInbound> {
    let webhook: TwilioWebhook = serde_urlencoded::from_str(body)
        .map_err(|e| anyhow::anyhow!("Failed to parse Twilio webhook: {e}"))?;

    let channel_type = webhook.channel_type();

    // Extract audio media info
    let (audio_url, audio_mime) = if webhook.has_audio_media() {
        (
            webhook.media_url_0.clone(),
            webhook.media_content_type_0.clone(),
        )
    } else {
        (None, None)
    };

    if webhook.body.trim().is_empty() {
        let has_media = webhook
            .num_media
            .as_deref()
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(0)
            > 0;

        if has_media {
            return Ok(TwilioInbound {
                message: InboundMessage {
                    sender_id: webhook.sender_number().to_string(),
                    text: "[Media message]".to_string(),
                    channel_id: Some(channel_type.as_str().to_string()),
                    message_id: Some(webhook.message_sid),
                    thread_id: None,
                    thread_ts: None,
                    attachments: Vec::new(),
                },
                channel_type,
                audio_url,
                audio_mime,
            });
        }

        anyhow::bail!("Empty message body with no media");
    }

    Ok(TwilioInbound {
        message: InboundMessage {
            sender_id: webhook.sender_number().to_string(),
            text: webhook.body,
            channel_id: Some(channel_type.as_str().to_string()),
            message_id: Some(webhook.message_sid),
            thread_id: None,
            thread_ts: None,
            attachments: Vec::new(),
        },
        channel_type,
        audio_url,
        audio_mime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sms_inbound() {
        let body = "MessageSid=SM123&AccountSid=AC123&From=%2B14155551234&To=%2B14155555678&Body=Hello+there";
        let result = parse_webhook(body).unwrap();
        assert_eq!(result.message.sender_id, "+14155551234");
        assert_eq!(result.message.text, "Hello there");
        assert_eq!(result.channel_type, TwilioChannelType::Sms);
        assert_eq!(result.message.message_id.as_deref(), Some("SM123"));
    }

    #[test]
    fn parse_whatsapp_inbound() {
        let body = "MessageSid=SM456&AccountSid=AC123&From=whatsapp%3A%2B14155551234&To=whatsapp%3A%2B14155555678&Body=Hi+from+WhatsApp";
        let result = parse_webhook(body).unwrap();
        assert_eq!(result.message.sender_id, "+14155551234");
        assert_eq!(result.message.text, "Hi from WhatsApp");
        assert_eq!(result.channel_type, TwilioChannelType::WhatsApp);
    }

    #[test]
    fn parse_empty_body_no_media() {
        let body = "MessageSid=SM789&AccountSid=AC123&From=%2B14155551234&To=%2B14155555678&Body=";
        assert!(parse_webhook(body).is_err());
    }

    #[test]
    fn parse_media_message() {
        let body = "MessageSid=SM101&AccountSid=AC123&From=%2B14155551234&To=%2B14155555678&Body=&NumMedia=1";
        let result = parse_webhook(body).unwrap();
        assert_eq!(result.message.text, "[Media message]");
    }

    #[test]
    fn parse_invalid_body() {
        assert!(parse_webhook("not valid form data %%%").is_err());
    }
}
