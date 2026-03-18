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
}
