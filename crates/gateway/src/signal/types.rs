use serde::{Deserialize, Serialize};

// ── SSE Envelope (inbound from signal-cli daemon) ──

/// Top-level SSE event from signal-cli daemon.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalEnvelope {
    pub envelope: EnvelopeData,
    /// The bot's own phone number (e.g., "+1234567890").
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeData {
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_number: Option<String>,
    #[serde(default)]
    pub source_uuid: Option<String>,
    #[serde(default)]
    pub source_device: Option<i32>,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub data_message: Option<DataMessage>,
    #[serde(default)]
    pub sync_message: Option<SyncMessage>,
    #[serde(default)]
    pub receipt_message: Option<serde_json::Value>,
    #[serde(default)]
    pub typing_message: Option<TypingMessage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataMessage {
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub group_info: Option<GroupInfo>,
    #[serde(default)]
    pub attachments: Option<Vec<Attachment>>,
    #[serde(default)]
    pub reaction: Option<Reaction>,
    #[serde(default)]
    pub quote: Option<Quote>,
    #[serde(default)]
    pub mentions: Option<Vec<Mention>>,
    #[serde(default)]
    pub expires_in_seconds: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub group_id: String,
    #[serde(default, rename = "type")]
    pub group_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub emoji: String,
    #[serde(default)]
    pub target_author: Option<String>,
    #[serde(default)]
    pub target_sent_timestamp: Option<i64>,
    #[serde(default)]
    pub is_remove: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mention {
    #[serde(default)]
    pub start: Option<i32>,
    #[serde(default)]
    pub length: Option<i32>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub number: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingMessage {
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncMessage {
    #[serde(default)]
    pub sent_message: Option<SentMessage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentMessage {
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub destination_uuid: Option<String>,
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub group_info: Option<GroupInfo>,
}

// ── JSON-RPC 2.0 (outbound to signal-cli daemon) ──

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub params: serde_json::Value,
}

impl JsonRpcRequest {
    pub fn new(method: &str, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            id: Some(super::api::next_request_id()),
            params,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcResponse {
    #[serde(default)]
    pub jsonrpc: Option<String>,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
    #[serde(default)]
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_dm_text_envelope() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "sourceUuid": "abc-def-123",
                "sourceDevice": 1,
                "timestamp": 1700000000000,
                "dataMessage": {
                    "timestamp": 1700000000000,
                    "message": "Hello from Signal"
                }
            },
            "account": "+15559876543"
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(env.account.as_deref(), Some("+15559876543"));
        assert_eq!(env.envelope.source.as_deref(), Some("+15551234567"));
        assert_eq!(env.envelope.source_uuid.as_deref(), Some("abc-def-123"));
        let dm = env.envelope.data_message.unwrap();
        assert_eq!(dm.message.as_deref(), Some("Hello from Signal"));
        assert!(dm.group_info.is_none());
    }

    #[test]
    fn deserialize_group_text_envelope() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "timestamp": 1700000000000,
                "dataMessage": {
                    "timestamp": 1700000000000,
                    "message": "Hello group",
                    "groupInfo": {
                        "groupId": "group123abc",
                        "type": "DELIVER"
                    }
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        let dm = env.envelope.data_message.unwrap();
        let group = dm.group_info.unwrap();
        assert_eq!(group.group_id, "group123abc");
        assert_eq!(group.group_type.as_deref(), Some("DELIVER"));
    }

    #[test]
    fn deserialize_attachment_envelope() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "timestamp": 1700000000000,
                "dataMessage": {
                    "timestamp": 1700000000000,
                    "message": "Check this out",
                    "attachments": [
                        {
                            "contentType": "image/png",
                            "filename": "photo.png",
                            "id": "att-123",
                            "size": 102400
                        }
                    ]
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        let dm = env.envelope.data_message.unwrap();
        let attachments = dm.attachments.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(attachments[0].filename.as_deref(), Some("photo.png"));
        assert_eq!(attachments[0].size, Some(102400));
    }

    #[test]
    fn deserialize_reaction_envelope() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "timestamp": 1700000000000,
                "dataMessage": {
                    "timestamp": 1700000000000,
                    "reaction": {
                        "emoji": "👍",
                        "targetAuthor": "+15559876543",
                        "targetSentTimestamp": 1699999999000,
                        "isRemove": false
                    }
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        let dm = env.envelope.data_message.unwrap();
        let reaction = dm.reaction.unwrap();
        assert_eq!(reaction.emoji, "👍");
        assert_eq!(reaction.target_author.as_deref(), Some("+15559876543"));
        assert!(!reaction.is_remove);
    }

    #[test]
    fn deserialize_receipt_only_envelope() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "timestamp": 1700000000000,
                "receiptMessage": {
                    "when": 1700000000000,
                    "isDelivery": true,
                    "timestamps": [1699999999000]
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        assert!(env.envelope.data_message.is_none());
        assert!(env.envelope.receipt_message.is_some());
    }

    #[test]
    fn deserialize_sync_sent_message() {
        let json = r#"{
            "envelope": {
                "source": "+15559876543",
                "timestamp": 1700000000000,
                "syncMessage": {
                    "sentMessage": {
                        "destination": "+15551234567",
                        "timestamp": 1700000000000,
                        "message": "Sent from another device"
                    }
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        let sync = env.envelope.sync_message.unwrap();
        let sent = sync.sent_message.unwrap();
        assert_eq!(sent.destination.as_deref(), Some("+15551234567"));
        assert_eq!(sent.message.as_deref(), Some("Sent from another device"));
    }

    #[test]
    fn deserialize_minimal_envelope() {
        let json = r#"{
            "envelope": {
                "timestamp": 0
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        assert!(env.envelope.source.is_none());
        assert!(env.envelope.data_message.is_none());
        assert!(env.envelope.sync_message.is_none());
        assert!(env.account.is_none());
    }

    #[test]
    fn deserialize_typing_message() {
        let json = r#"{
            "envelope": {
                "source": "+15551234567",
                "timestamp": 1700000000000,
                "typingMessage": {
                    "action": "STARTED",
                    "timestamp": 1700000000000
                }
            }
        }"#;
        let env: SignalEnvelope = serde_json::from_str(json).unwrap();
        assert!(env.envelope.data_message.is_none());
        let typing = env.envelope.typing_message.unwrap();
        assert_eq!(typing.action.as_deref(), Some("STARTED"));
    }

    #[test]
    fn jsonrpc_response_success() {
        let json = r#"{
            "jsonrpc": "2.0",
            "result": { "version": "0.13.2" },
            "id": "1"
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn jsonrpc_response_error() {
        let json = r#"{
            "jsonrpc": "2.0",
            "error": {
                "code": -32601,
                "message": "Method not found"
            },
            "id": "1"
        }"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "Method not found");
    }

    #[test]
    fn jsonrpc_request_serialization() {
        let req = JsonRpcRequest::new(
            "send",
            serde_json::json!({ "recipient": "+15551234567", "message": "hi" }),
        );
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "send");
        assert!(req.id.is_some());
        let serialized = serde_json::to_string(&req).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"method\":\"send\""));
    }
}
