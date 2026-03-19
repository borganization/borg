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
}
