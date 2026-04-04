use serde::Deserialize;

/// Distinguishes SMS vs WhatsApp for routing and response logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwilioChannelType {
    Sms,
    WhatsApp,
}

impl TwilioChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sms => "sms",
            Self::WhatsApp => "whatsapp",
        }
    }
}

impl std::fmt::Display for TwilioChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Twilio webhook payload (form-urlencoded).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TwilioWebhook {
    /// The unique message SID.
    pub message_sid: String,
    /// The account SID that owns this message.
    #[serde(default)]
    pub account_sid: String,
    /// The sender phone number (E.164 format, e.g. "+14155551234").
    pub from: String,
    /// The recipient phone number.
    pub to: String,
    /// The message body text.
    #[serde(default)]
    pub body: String,
    /// Number of media items attached (SMS/MMS).
    #[serde(default)]
    pub num_media: Option<String>,
    /// First media URL (for voice/image/video attachments).
    #[serde(rename = "MediaUrl0", default)]
    pub media_url_0: Option<String>,
    /// Content type of the first media attachment.
    #[serde(rename = "MediaContentType0", default)]
    pub media_content_type_0: Option<String>,
}

impl TwilioWebhook {
    /// Returns the channel type based on the From number prefix.
    pub fn channel_type(&self) -> TwilioChannelType {
        if self.from.starts_with("whatsapp:") {
            TwilioChannelType::WhatsApp
        } else {
            TwilioChannelType::Sms
        }
    }

    /// Returns true if this webhook is from a WhatsApp message.
    pub fn is_whatsapp(&self) -> bool {
        self.channel_type() == TwilioChannelType::WhatsApp
    }

    /// Returns the sender number without the "whatsapp:" prefix.
    pub fn sender_number(&self) -> &str {
        self.from.strip_prefix("whatsapp:").unwrap_or(&self.from)
    }

    /// Returns the recipient number without the "whatsapp:" prefix.
    pub fn recipient_number(&self) -> &str {
        self.to.strip_prefix("whatsapp:").unwrap_or(&self.to)
    }

    /// Returns the first media (URL, content-type) if present.
    pub fn first_media(&self) -> Option<(&str, &str)> {
        match (&self.media_url_0, &self.media_content_type_0) {
            (Some(url), Some(ct)) => Some((url, ct)),
            _ => None,
        }
    }

    /// Returns true if the first media attachment is an audio type.
    pub fn has_audio_media(&self) -> bool {
        self.media_content_type_0
            .as_deref()
            .map(|ct| ct.starts_with("audio/"))
            .unwrap_or(false)
    }
}

/// Twilio message status callback (delivery receipt).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct StatusCallback {
    /// The message SID.
    pub message_sid: String,
    /// The message status (queued, sent, delivered, failed, undelivered).
    pub message_status: String,
    /// The account SID.
    #[serde(default)]
    pub account_sid: String,
    /// The sender number.
    #[serde(default)]
    pub from: String,
    /// The recipient number.
    #[serde(default)]
    pub to: String,
    /// Error code (if failed/undelivered).
    #[serde(default)]
    pub error_code: Option<String>,
    /// Error message (if failed/undelivered).
    #[serde(default)]
    pub error_message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whatsapp_detection() {
        let wh = TwilioWebhook {
            message_sid: "SM123".into(),
            account_sid: "AC123".into(),
            from: "whatsapp:+14155551234".into(),
            to: "whatsapp:+14155555678".into(),
            body: "hello".into(),
            num_media: None,
            media_url_0: None,
            media_content_type_0: None,
        };
        assert!(wh.is_whatsapp());
        assert_eq!(wh.sender_number(), "+14155551234");
        assert_eq!(wh.recipient_number(), "+14155555678");
    }

    #[test]
    fn sms_detection() {
        let wh = TwilioWebhook {
            message_sid: "SM456".into(),
            account_sid: "AC123".into(),
            from: "+14155551234".into(),
            to: "+14155555678".into(),
            body: "hello sms".into(),
            num_media: None,
            media_url_0: None,
            media_content_type_0: None,
        };
        assert!(!wh.is_whatsapp());
        assert_eq!(wh.sender_number(), "+14155551234");
    }

    #[test]
    fn channel_type_as_str() {
        assert_eq!(TwilioChannelType::Sms.as_str(), "sms");
        assert_eq!(TwilioChannelType::WhatsApp.as_str(), "whatsapp");
    }

    #[test]
    fn channel_type_display() {
        assert_eq!(format!("{}", TwilioChannelType::Sms), "sms");
        assert_eq!(format!("{}", TwilioChannelType::WhatsApp), "whatsapp");
    }

    #[test]
    fn has_audio_media_detection() {
        let wh = TwilioWebhook {
            message_sid: "SM123".into(),
            account_sid: "AC123".into(),
            from: "whatsapp:+14155551234".into(),
            to: "whatsapp:+14155555678".into(),
            body: String::new(),
            num_media: Some("1".into()),
            media_url_0: Some("https://api.twilio.com/media/123".into()),
            media_content_type_0: Some("audio/ogg".into()),
        };
        assert!(wh.has_audio_media());
        let (url, ct) = wh.first_media().unwrap();
        assert!(url.contains("twilio.com"));
        assert_eq!(ct, "audio/ogg");
    }

    #[test]
    fn no_audio_media_for_image() {
        let wh = TwilioWebhook {
            message_sid: "SM123".into(),
            account_sid: "AC123".into(),
            from: "+14155551234".into(),
            to: "+14155555678".into(),
            body: String::new(),
            num_media: Some("1".into()),
            media_url_0: Some("https://api.twilio.com/media/456".into()),
            media_content_type_0: Some("image/jpeg".into()),
        };
        assert!(!wh.has_audio_media());
    }

    #[test]
    fn no_media_returns_none() {
        let wh = TwilioWebhook {
            message_sid: "SM123".into(),
            account_sid: "AC123".into(),
            from: "+14155551234".into(),
            to: "+14155555678".into(),
            body: "hello".into(),
            num_media: None,
            media_url_0: None,
            media_content_type_0: None,
        };
        assert!(!wh.has_audio_media());
        assert!(wh.first_media().is_none());
    }

    #[test]
    fn deserialize_status_callback_delivered() {
        let body = "MessageSid=SM123&MessageStatus=delivered&AccountSid=AC123&From=%2B14155551234&To=%2B14155555678";
        let cb: StatusCallback = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(cb.message_sid, "SM123");
        assert_eq!(cb.message_status, "delivered");
        assert!(cb.error_code.is_none());
    }

    #[test]
    fn deserialize_status_callback_failed() {
        let body = "MessageSid=SM456&MessageStatus=failed&AccountSid=AC123&From=%2B14155551234&To=%2B14155555678&ErrorCode=30008&ErrorMessage=Unknown+error";
        let cb: StatusCallback = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(cb.message_sid, "SM456");
        assert_eq!(cb.message_status, "failed");
        assert_eq!(cb.error_code.as_deref(), Some("30008"));
        assert_eq!(cb.error_message.as_deref(), Some("Unknown error"));
    }

    #[test]
    fn first_media_partial_returns_none() {
        // Only URL set, no content type — should return None
        let wh = TwilioWebhook {
            message_sid: "SM123".into(),
            account_sid: "AC123".into(),
            from: "+14155551234".into(),
            to: "+14155555678".into(),
            body: "hello".into(),
            num_media: Some("1".into()),
            media_url_0: Some("https://api.twilio.com/media/123".into()),
            media_content_type_0: None,
        };
        assert!(
            wh.first_media().is_none(),
            "partial media (URL only) should return None"
        );

        // Only content type set, no URL — should return None
        let wh2 = TwilioWebhook {
            message_sid: "SM456".into(),
            account_sid: "AC123".into(),
            from: "+14155551234".into(),
            to: "+14155555678".into(),
            body: "hello".into(),
            num_media: Some("1".into()),
            media_url_0: None,
            media_content_type_0: Some("image/jpeg".into()),
        };
        assert!(
            wh2.first_media().is_none(),
            "partial media (content-type only) should return None"
        );
    }

    #[test]
    fn deserialize_status_callback_minimal() {
        let body = "MessageSid=SM789&MessageStatus=queued";
        let cb: StatusCallback = serde_urlencoded::from_str(body).unwrap();
        assert_eq!(cb.message_sid, "SM789");
        assert_eq!(cb.message_status, "queued");
        assert!(cb.from.is_empty());
    }
}
