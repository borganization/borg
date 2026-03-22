use crate::handler::InboundMessage;

use super::types::SignalEnvelope;

/// Convert a signal-cli SSE envelope into an `InboundMessage`.
///
/// Returns `None` for:
/// - Own messages (source matches `own_account`)
/// - Sync messages (messages sent from another device on the same account)
/// - Receipt-only or typing-only envelopes
/// - Envelopes with no text content and no reaction
pub fn parse_envelope(envelope: &SignalEnvelope, own_account: &str) -> Option<InboundMessage> {
    let env = &envelope.envelope;

    // Determine sender — prefer source, fall back to source_number, then source_uuid
    let sender = env
        .source
        .as_deref()
        .or(env.source_number.as_deref())
        .or(env.source_uuid.as_deref())?;

    // Filter own messages (loop protection)
    if sender == own_account {
        return None;
    }
    // Also check if the envelope's account field matches the sender
    if let Some(ref account) = envelope.account {
        if sender == account {
            return None;
        }
    }

    // Filter sync messages (sent from another device on our account)
    if env.sync_message.is_some() {
        return None;
    }

    // Only process data messages
    let data_msg = env.data_message.as_ref()?;

    // Build text from message content
    let mut text = data_msg.message.clone().unwrap_or_default();

    // Append attachment placeholders
    if let Some(ref attachments) = data_msg.attachments {
        for att in attachments {
            let desc = att
                .filename
                .as_deref()
                .or(att.content_type.as_deref())
                .unwrap_or("file");
            if text.is_empty() {
                text = format!("[Attachment: {desc}]");
            } else {
                text = format!("{text}\n[Attachment: {desc}]");
            }
        }
    }

    // Handle reaction-only messages
    let reaction = data_msg.reaction.as_ref().map(|r| {
        if r.is_remove {
            format!("-{}", r.emoji)
        } else {
            r.emoji.clone()
        }
    });

    // If no text and no reaction, skip
    if text.is_empty() && reaction.is_none() {
        return None;
    }

    // For reaction-only messages, set text to describe the reaction
    if text.is_empty() {
        if let Some(ref r) = reaction {
            text = format!("[Reaction: {r}]");
        }
    }

    // Map group_info -> channel_id
    let channel_id = data_msg.group_info.as_ref().map(|g| g.group_id.clone());

    // Use the data message timestamp as message_id
    let message_id = if data_msg.timestamp != 0 {
        Some(data_msg.timestamp.to_string())
    } else {
        None
    };

    Some(InboundMessage {
        sender_id: sender.to_string(),
        text,
        channel_id,
        thread_id: None,
        message_id,
        thread_ts: None,
        attachments: vec![],
        reaction,
        metadata: serde_json::json!({
            "source_uuid": env.source_uuid,
            "timestamp": env.timestamp,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::types::*;

    fn make_envelope(json: &str) -> SignalEnvelope {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn dm_text_message() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "sourceUuid": "uuid-123",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "Hello"
                    }
                },
                "account": "+15559876543"
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.sender_id, "+15551234567");
        assert_eq!(msg.text, "Hello");
        assert!(msg.channel_id.is_none());
        assert_eq!(msg.message_id.as_deref(), Some("1700000000000"));
    }

    #[test]
    fn group_text_message() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "Hello group",
                        "groupInfo": {
                            "groupId": "group123"
                        }
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.text, "Hello group");
        assert_eq!(msg.channel_id.as_deref(), Some("group123"));
    }

    #[test]
    fn own_message_filtered_by_account() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15559876543",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "My own message"
                    }
                },
                "account": "+15559876543"
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }

    #[test]
    fn own_message_filtered_by_own_account_param() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15559876543",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "My own message"
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }

    #[test]
    fn sync_message_filtered() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15559876543",
                    "timestamp": 1700000000000,
                    "syncMessage": {
                        "sentMessage": {
                            "destination": "+15551234567",
                            "message": "Sent from another device"
                        }
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15550000000").is_none());
    }

    #[test]
    fn receipt_only_filtered() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "receiptMessage": {
                        "when": 1700000000000,
                        "isDelivery": true
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }

    #[test]
    fn typing_only_filtered() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "typingMessage": {
                        "action": "STARTED"
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }

    #[test]
    fn attachment_adds_placeholder() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "Look at this",
                        "attachments": [
                            {
                                "contentType": "image/jpeg",
                                "filename": "photo.jpg",
                                "size": 50000
                            }
                        ]
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert!(msg.text.contains("Look at this"));
        assert!(msg.text.contains("[Attachment: photo.jpg]"));
    }

    #[test]
    fn attachment_only_message() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "attachments": [
                            {
                                "contentType": "application/pdf"
                            }
                        ]
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.text, "[Attachment: application/pdf]");
    }

    #[test]
    fn reaction_message() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "reaction": {
                            "emoji": "👍",
                            "targetAuthor": "+15559876543",
                            "targetSentTimestamp": 1699999000000,
                            "isRemove": false
                        }
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.reaction.as_deref(), Some("👍"));
        assert!(msg.text.contains("[Reaction: 👍]"));
    }

    #[test]
    fn reaction_remove() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "reaction": {
                            "emoji": "👍",
                            "targetAuthor": "+15559876543",
                            "targetSentTimestamp": 1699999000000,
                            "isRemove": true
                        }
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.reaction.as_deref(), Some("-👍"));
    }

    #[test]
    fn empty_data_message_filtered() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "source": "+15551234567",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }

    #[test]
    fn uuid_sender_fallback() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "sourceUuid": "uuid-abc-123",
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "From UUID sender"
                    }
                }
            }"#,
        );
        let msg = parse_envelope(&env, "+15559876543").unwrap();
        assert_eq!(msg.sender_id, "uuid-abc-123");
    }

    #[test]
    fn no_source_returns_none() {
        let env = make_envelope(
            r#"{
                "envelope": {
                    "timestamp": 1700000000000,
                    "dataMessage": {
                        "timestamp": 1700000000000,
                        "message": "No sender"
                    }
                }
            }"#,
        );
        assert!(parse_envelope(&env, "+15559876543").is_none());
    }
}
