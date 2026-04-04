//! Data structures for the native iMessage integration.

/// A raw message row from chat.db.
#[derive(Debug)]
pub struct IMessagePayload {
    /// SQLite row ID from chat.db.
    pub rowid: i64,
    /// Sender phone number or email.
    pub sender: Option<String>,
    /// Whether this message was sent by the current user.
    pub is_from_me: bool,
    /// Text content of the message.
    pub text: Option<String>,
    /// Chat identifier string (e.g. "iMessage;-;+1234567890").
    pub chat_identifier: Option<String>,
    /// Whether the message is from a group chat.
    pub is_group: bool,
}

/// Decision result after running inbound message through all filters.
pub enum InboundDecision {
    /// Message should be dispatched to the agent.
    Dispatch {
        /// Sender identifier (phone number or email).
        sender_id: String,
        /// Message text content.
        text: String,
        /// Chat identifier for routing the response.
        channel_id: String,
    },
    /// Message should be dropped silently.
    Drop {
        /// Reason the message was dropped.
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imessage_payload_debug() {
        let payload = IMessagePayload {
            rowid: 42,
            sender: Some("+1234567890".to_string()),
            is_from_me: false,
            text: Some("Hello".to_string()),
            chat_identifier: Some("iMessage;-;+1234567890".to_string()),
            is_group: false,
        };
        let debug = format!("{payload:?}");
        assert!(debug.contains("42"));
        assert!(debug.contains("+1234567890"));
    }

    #[test]
    fn imessage_payload_optional_fields() {
        let payload = IMessagePayload {
            rowid: 1,
            sender: None,
            is_from_me: true,
            text: None,
            chat_identifier: None,
            is_group: true,
        };
        assert!(payload.sender.is_none());
        assert!(payload.text.is_none());
        assert!(payload.chat_identifier.is_none());
        assert!(payload.is_from_me);
        assert!(payload.is_group);
    }

    #[test]
    fn inbound_decision_dispatch_variant() {
        let decision = InboundDecision::Dispatch {
            sender_id: "user1".to_string(),
            text: "hello".to_string(),
            channel_id: "ch1".to_string(),
        };
        match decision {
            InboundDecision::Dispatch {
                sender_id,
                text,
                channel_id,
            } => {
                assert_eq!(sender_id, "user1");
                assert_eq!(text, "hello");
                assert_eq!(channel_id, "ch1");
            }
            InboundDecision::Drop { .. } => panic!("expected Dispatch"),
        }
    }

    #[test]
    fn inbound_decision_drop_variant() {
        let decision = InboundDecision::Drop {
            reason: "test reason".to_string(),
        };
        match decision {
            InboundDecision::Drop { reason } => assert_eq!(reason, "test reason"),
            InboundDecision::Dispatch { .. } => panic!("expected Drop"),
        }
    }
}
