//! Data structures for the native iMessage integration.

/// A raw message row from chat.db.
#[derive(Debug)]
pub struct IMessagePayload {
    pub rowid: i64,
    pub sender: Option<String>,
    pub is_from_me: bool,
    pub text: Option<String>,
    pub chat_identifier: Option<String>,
    pub is_group: bool,
}

/// Decision result after running inbound message through all filters.
pub enum InboundDecision {
    /// Message should be dispatched to the agent.
    Dispatch {
        sender_id: String,
        text: String,
        channel_id: String,
    },
    /// Message should be dropped silently.
    Drop { reason: String },
}
