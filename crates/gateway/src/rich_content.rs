//! Cross-platform rich content types for messaging integrations.
//!
//! Provides a shared vocabulary of rich response elements (buttons, cards, polls, reactions,
//! file attachments) that can be rendered natively on each platform.

use serde::{Deserialize, Serialize};

/// A rich response that can include buttons, cards, polls, reactions, and file attachments.
///
/// The agent embeds this as a `<!-- rich:{...} -->` JSON block at the end of its text response.
/// The gateway server parses it out and renders platform-native elements.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RichResponse {
    /// Primary text content (may be empty if the original text is used).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,

    /// Rows of inline buttons (Telegram inline keyboards, Slack buttons, Discord components).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<ButtonRow>,

    /// Rich card / embed (Discord embed, Teams Adaptive Card, Google Chat card).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,

    /// Poll (currently Telegram-only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll: Option<Poll>,

    /// Emoji reaction to apply to the inbound message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reaction: Option<String>,

    /// Suppress notification sound (Telegram `disable_notification`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub silent: bool,

    /// File attachments to upload alongside the message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileAttachment>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// A row of buttons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonRow {
    pub buttons: Vec<Button>,
}

/// A single button with a label and action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Button {
    pub label: String,
    pub action: ButtonAction,
}

/// What happens when a button is pressed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum ButtonAction {
    /// Send a callback data string back to the bot.
    Callback(String),
    /// Open a URL in the user's browser.
    Url(String),
}

/// A rich card / embed with structured content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Hex color string (e.g. "#FF5733") for embed accent / card theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    /// Structured key-value fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<CardField>,

    /// Image URL to display in the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,

    /// Footer text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
}

/// A single field within a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardField {
    pub name: String,
    pub value: String,
    #[serde(default)]
    pub inline: bool,
}

/// A poll question with options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poll {
    pub question: String,
    pub options: Vec<String>,
    #[serde(default = "default_true")]
    pub anonymous: bool,
}

fn default_true() -> bool {
    true
}

/// A file to attach to the message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttachment {
    pub filename: String,
    pub mime_type: String,
    /// Base64-encoded file data.
    pub data: String,
}

// ── Rich response parsing ───────────────────────────────────────────────────

/// Marker prefix for the rich JSON block embedded in agent response text.
const RICH_MARKER_START: &str = "<!-- rich:";
const RICH_MARKER_END: &str = " -->";

/// Extract a `RichResponse` from agent response text.
///
/// Looks for a `<!-- rich:{...} -->` block at the end of the text.
/// Returns `(clean_text, Option<RichResponse>)`.
pub fn parse_rich_response(text: &str) -> (String, Option<RichResponse>) {
    if let Some(start_idx) = text.rfind(RICH_MARKER_START) {
        let json_start = start_idx + RICH_MARKER_START.len();
        if let Some(end_rel) = text[json_start..].find(RICH_MARKER_END) {
            let json_str = &text[json_start..json_start + end_rel];
            match serde_json::from_str::<RichResponse>(json_str) {
                Ok(rich) => {
                    let clean = text[..start_idx].trim_end().to_string();
                    return (clean, Some(rich));
                }
                Err(_) => {
                    // Malformed JSON — return text as-is
                    return (text.to_string(), None);
                }
            }
        }
    }
    (text.to_string(), None)
}

// ── Platform-specific rendering helpers ──────────────────────────────────────

/// Render buttons as a Telegram `InlineKeyboardMarkup` JSON value.
pub fn render_telegram_keyboard(buttons: &[ButtonRow]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = buttons
        .iter()
        .map(|row| {
            let btns: Vec<serde_json::Value> = row
                .buttons
                .iter()
                .map(|btn| match &btn.action {
                    ButtonAction::Callback(data) => serde_json::json!({
                        "text": btn.label,
                        "callback_data": data,
                    }),
                    ButtonAction::Url(url) => serde_json::json!({
                        "text": btn.label,
                        "url": url,
                    }),
                })
                .collect();
            serde_json::Value::Array(btns)
        })
        .collect();
    serde_json::json!({ "inline_keyboard": rows })
}

/// Render a card as Slack Block Kit blocks JSON array.
pub fn render_slack_blocks(card: &Card, buttons: &[ButtonRow]) -> Vec<serde_json::Value> {
    let mut blocks = Vec::new();

    // Header section
    if let Some(title) = &card.title {
        blocks.push(serde_json::json!({
            "type": "header",
            "text": { "type": "plain_text", "text": title }
        }));
    }

    // Description section
    if let Some(desc) = &card.description {
        blocks.push(serde_json::json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": desc }
        }));
    }

    // Fields
    if !card.fields.is_empty() {
        let field_elements: Vec<serde_json::Value> = card
            .fields
            .iter()
            .map(|f| {
                serde_json::json!({
                    "type": "mrkdwn",
                    "text": format!("*{}*\n{}", f.name, f.value)
                })
            })
            .collect();
        blocks.push(serde_json::json!({
            "type": "section",
            "fields": field_elements
        }));
    }

    // Image
    if let Some(url) = &card.image_url {
        blocks.push(serde_json::json!({
            "type": "image",
            "image_url": url,
            "alt_text": card.title.as_deref().unwrap_or("image")
        }));
    }

    // Buttons as actions block
    if !buttons.is_empty() {
        let mut elements = Vec::new();
        for row in buttons {
            for btn in &row.buttons {
                match &btn.action {
                    ButtonAction::Callback(data) => {
                        elements.push(serde_json::json!({
                            "type": "button",
                            "text": { "type": "plain_text", "text": btn.label },
                            "action_id": data,
                        }));
                    }
                    ButtonAction::Url(url) => {
                        elements.push(serde_json::json!({
                            "type": "button",
                            "text": { "type": "plain_text", "text": btn.label },
                            "url": url,
                        }));
                    }
                }
            }
        }
        blocks.push(serde_json::json!({
            "type": "actions",
            "elements": elements
        }));
    }

    // Footer as context
    if let Some(footer) = &card.footer {
        blocks.push(serde_json::json!({
            "type": "context",
            "elements": [{ "type": "mrkdwn", "text": footer }]
        }));
    }

    blocks
}

/// Render a card as a Discord embed JSON value.
pub fn render_discord_embed(card: &Card) -> serde_json::Value {
    let mut embed = serde_json::Map::new();

    if let Some(title) = &card.title {
        embed.insert("title".into(), serde_json::json!(title));
    }
    if let Some(desc) = &card.description {
        embed.insert("description".into(), serde_json::json!(desc));
    }
    if let Some(color) = &card.color {
        // Parse hex color to integer
        if let Some(stripped) = color.strip_prefix('#') {
            if let Ok(n) = u32::from_str_radix(stripped, 16) {
                embed.insert("color".into(), serde_json::json!(n));
            }
        }
    }
    if !card.fields.is_empty() {
        let fields: Vec<serde_json::Value> = card
            .fields
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f.name,
                    "value": f.value,
                    "inline": f.inline,
                })
            })
            .collect();
        embed.insert("fields".into(), serde_json::Value::Array(fields));
    }
    if let Some(url) = &card.image_url {
        embed.insert("image".into(), serde_json::json!({ "url": url }));
    }
    if let Some(footer) = &card.footer {
        embed.insert("footer".into(), serde_json::json!({ "text": footer }));
    }

    serde_json::Value::Object(embed)
}

/// Render a card as a Teams Adaptive Card attachment JSON value.
pub fn render_teams_adaptive_card(card: &Card) -> serde_json::Value {
    let mut body = Vec::new();

    if let Some(title) = &card.title {
        body.push(serde_json::json!({
            "type": "TextBlock",
            "text": title,
            "weight": "Bolder",
            "size": "Medium",
        }));
    }
    if let Some(desc) = &card.description {
        body.push(serde_json::json!({
            "type": "TextBlock",
            "text": desc,
            "wrap": true,
        }));
    }

    if !card.fields.is_empty() {
        let mut facts: Vec<serde_json::Value> = card
            .fields
            .iter()
            .map(|f| serde_json::json!({ "title": f.name, "value": f.value }))
            .collect();
        let _ = &mut facts; // suppress unused warning
        body.push(serde_json::json!({
            "type": "FactSet",
            "facts": facts,
        }));
    }

    if let Some(url) = &card.image_url {
        body.push(serde_json::json!({
            "type": "Image",
            "url": url,
        }));
    }

    serde_json::json!({
        "contentType": "application/vnd.microsoft.card.adaptive",
        "content": {
            "type": "AdaptiveCard",
            "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
            "version": "1.4",
            "body": body,
        }
    })
}

/// Render a card as a Google Chat `cardsV2` entry.
pub fn render_google_chat_card(card: &Card) -> serde_json::Value {
    let mut sections = Vec::new();

    // Description widget
    if let Some(desc) = &card.description {
        sections.push(serde_json::json!({
            "widgets": [{
                "textParagraph": { "text": desc }
            }]
        }));
    }

    // Fields as decorated text widgets
    if !card.fields.is_empty() {
        let widgets: Vec<serde_json::Value> = card
            .fields
            .iter()
            .map(|f| {
                serde_json::json!({
                    "decoratedText": {
                        "topLabel": f.name,
                        "text": f.value,
                    }
                })
            })
            .collect();
        sections.push(serde_json::json!({ "widgets": widgets }));
    }

    // Image widget
    if let Some(url) = &card.image_url {
        sections.push(serde_json::json!({
            "widgets": [{
                "image": { "imageUrl": url }
            }]
        }));
    }

    let mut card_json = serde_json::Map::new();
    if let Some(title) = &card.title {
        card_json.insert("header".into(), serde_json::json!({ "title": title }));
    }
    card_json.insert("sections".into(), serde_json::Value::Array(sections));

    serde_json::json!({
        "cardId": "richCard",
        "card": serde_json::Value::Object(card_json),
    })
}

/// Render buttons as Discord action row components JSON.
pub fn render_discord_components(buttons: &[ButtonRow]) -> Vec<serde_json::Value> {
    buttons
        .iter()
        .map(|row| {
            let components: Vec<serde_json::Value> = row
                .buttons
                .iter()
                .map(|btn| match &btn.action {
                    ButtonAction::Callback(data) => serde_json::json!({
                        "type": 2, // Button
                        "style": 1, // Primary
                        "label": btn.label,
                        "custom_id": data,
                    }),
                    ButtonAction::Url(url) => serde_json::json!({
                        "type": 2, // Button
                        "style": 5, // Link
                        "label": btn.label,
                        "url": url,
                    }),
                })
                .collect();
            serde_json::json!({
                "type": 1, // ActionRow
                "components": components,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rich_response_default_is_empty() {
        let rich = RichResponse::default();
        assert!(rich.text.is_empty());
        assert!(rich.buttons.is_empty());
        assert!(rich.card.is_none());
        assert!(rich.poll.is_none());
        assert!(rich.reaction.is_none());
        assert!(!rich.silent);
        assert!(rich.files.is_empty());
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let rich = RichResponse {
            text: "Hello".into(),
            buttons: vec![ButtonRow {
                buttons: vec![
                    Button {
                        label: "Yes".into(),
                        action: ButtonAction::Callback("yes".into()),
                    },
                    Button {
                        label: "Docs".into(),
                        action: ButtonAction::Url("https://example.com".into()),
                    },
                ],
            }],
            card: Some(Card {
                title: Some("Status".into()),
                description: Some("All systems operational".into()),
                color: Some("#00FF00".into()),
                fields: vec![CardField {
                    name: "Uptime".into(),
                    value: "99.9%".into(),
                    inline: true,
                }],
                image_url: None,
                footer: Some("Last checked 5m ago".into()),
            }),
            poll: None,
            reaction: Some("thumbsup".into()),
            silent: true,
            files: vec![FileAttachment {
                filename: "report.txt".into(),
                mime_type: "text/plain".into(),
                data: "SGVsbG8=".into(),
            }],
        };

        let json = serde_json::to_string(&rich).unwrap();
        let deserialized: RichResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.text, "Hello");
        assert_eq!(deserialized.buttons.len(), 1);
        assert_eq!(deserialized.buttons[0].buttons.len(), 2);
        assert!(deserialized.card.is_some());
        assert_eq!(deserialized.reaction.as_deref(), Some("thumbsup"));
        assert!(deserialized.silent);
        assert_eq!(deserialized.files.len(), 1);
    }

    #[test]
    fn skip_serializing_empty_fields() {
        let rich = RichResponse::default();
        let json = serde_json::to_value(&rich).unwrap();
        // Default RichResponse should serialize to just `{}`
        let obj = json.as_object().unwrap();
        assert!(obj.is_empty(), "Expected empty object, got: {json}");
    }

    #[test]
    fn button_action_callback_serde() {
        let action = ButtonAction::Callback("click_me".into());
        let json = serde_json::to_string(&action).unwrap();
        let parsed: ButtonAction = serde_json::from_str(&json).unwrap();
        match parsed {
            ButtonAction::Callback(data) => assert_eq!(data, "click_me"),
            _ => panic!("Expected Callback"),
        }
    }

    #[test]
    fn button_action_url_serde() {
        let action = ButtonAction::Url("https://example.com".into());
        let json = serde_json::to_string(&action).unwrap();
        let parsed: ButtonAction = serde_json::from_str(&json).unwrap();
        match parsed {
            ButtonAction::Url(url) => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected Url"),
        }
    }

    #[test]
    fn poll_defaults() {
        let json = r#"{"question": "Lunch?", "options": ["Pizza", "Sushi"]}"#;
        let poll: Poll = serde_json::from_str(json).unwrap();
        assert_eq!(poll.question, "Lunch?");
        assert_eq!(poll.options, vec!["Pizza", "Sushi"]);
        assert!(poll.anonymous); // default true
    }

    #[test]
    fn poll_non_anonymous() {
        let json = r#"{"question": "Vote", "options": ["A", "B"], "anonymous": false}"#;
        let poll: Poll = serde_json::from_str(json).unwrap();
        assert!(!poll.anonymous);
    }

    #[test]
    fn file_attachment_serde() {
        let file = FileAttachment {
            filename: "data.csv".into(),
            mime_type: "text/csv".into(),
            data: "YSxiLGM=".into(),
        };
        let json = serde_json::to_string(&file).unwrap();
        let parsed: FileAttachment = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.filename, "data.csv");
        assert_eq!(parsed.mime_type, "text/csv");
        assert_eq!(parsed.data, "YSxiLGM=");
    }

    // ── parse_rich_response tests ───────────────────────────────────────

    #[test]
    fn parse_rich_response_extracts_json() {
        let text = r#"Here is your answer.

<!-- rich:{"reaction":"thumbsup","silent":true} -->"#;
        let (clean, rich) = parse_rich_response(text);
        assert_eq!(clean, "Here is your answer.");
        let rich = rich.unwrap();
        assert_eq!(rich.reaction.as_deref(), Some("thumbsup"));
        assert!(rich.silent);
    }

    #[test]
    fn parse_rich_response_no_marker() {
        let text = "Just plain text.";
        let (clean, rich) = parse_rich_response(text);
        assert_eq!(clean, "Just plain text.");
        assert!(rich.is_none());
    }

    #[test]
    fn parse_rich_response_malformed_json() {
        let text = "Answer <!-- rich:{not valid json -->";
        let (clean, rich) = parse_rich_response(text);
        assert_eq!(clean, text);
        assert!(rich.is_none());
    }

    #[test]
    fn parse_rich_response_with_buttons() {
        let rich_json = serde_json::json!({
            "buttons": [{
                "buttons": [
                    {"label": "OK", "action": {"type": "Callback", "value": "ok"}}
                ]
            }]
        });
        let text = format!("Click a button\n\n<!-- rich:{} -->", rich_json);
        let (clean, rich) = parse_rich_response(&text);
        assert_eq!(clean, "Click a button");
        let rich = rich.unwrap();
        assert_eq!(rich.buttons.len(), 1);
        assert_eq!(rich.buttons[0].buttons[0].label, "OK");
    }

    // ── Platform render tests ───────────────────────────────────────────

    #[test]
    fn render_telegram_keyboard_callback_and_url() {
        let buttons = vec![ButtonRow {
            buttons: vec![
                Button {
                    label: "Click".into(),
                    action: ButtonAction::Callback("cb1".into()),
                },
                Button {
                    label: "Open".into(),
                    action: ButtonAction::Url("https://example.com".into()),
                },
            ],
        }];
        let kb = render_telegram_keyboard(&buttons);
        let rows = kb["inline_keyboard"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        let row = rows[0].as_array().unwrap();
        assert_eq!(row[0]["text"], "Click");
        assert_eq!(row[0]["callback_data"], "cb1");
        assert_eq!(row[1]["text"], "Open");
        assert_eq!(row[1]["url"], "https://example.com");
    }

    #[test]
    fn render_discord_embed_with_color() {
        let card = Card {
            title: Some("Test".into()),
            description: Some("A test card".into()),
            color: Some("#FF5733".into()),
            fields: vec![CardField {
                name: "Key".into(),
                value: "Val".into(),
                inline: true,
            }],
            image_url: None,
            footer: Some("footer".into()),
        };
        let embed = render_discord_embed(&card);
        assert_eq!(embed["title"], "Test");
        assert_eq!(embed["description"], "A test card");
        assert_eq!(embed["color"], 0xFF5733);
        assert_eq!(embed["fields"][0]["name"], "Key");
        assert_eq!(embed["footer"]["text"], "footer");
    }

    #[test]
    fn render_slack_blocks_with_card_and_buttons() {
        let card = Card {
            title: Some("Report".into()),
            description: Some("Summary here".into()),
            color: None,
            fields: vec![],
            image_url: None,
            footer: None,
        };
        let buttons = vec![ButtonRow {
            buttons: vec![Button {
                label: "Approve".into(),
                action: ButtonAction::Callback("approve".into()),
            }],
        }];
        let blocks = render_slack_blocks(&card, &buttons);
        // header + section + actions = 3 blocks
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[1]["type"], "section");
        assert_eq!(blocks[2]["type"], "actions");
    }

    #[test]
    fn render_teams_adaptive_card_structure() {
        let card = Card {
            title: Some("Alert".into()),
            description: Some("Service down".into()),
            color: None,
            fields: vec![CardField {
                name: "Region".into(),
                value: "US-East".into(),
                inline: false,
            }],
            image_url: None,
            footer: None,
        };
        let attachment = render_teams_adaptive_card(&card);
        assert_eq!(
            attachment["contentType"],
            "application/vnd.microsoft.card.adaptive"
        );
        assert_eq!(attachment["content"]["type"], "AdaptiveCard");
        assert_eq!(attachment["content"]["version"], "1.4");
        let body = attachment["content"]["body"].as_array().unwrap();
        assert!(body.len() >= 2); // title + description + factset
    }

    #[test]
    fn render_google_chat_card_structure() {
        let card = Card {
            title: Some("Summary".into()),
            description: Some("Everything OK".into()),
            color: None,
            fields: vec![],
            image_url: None,
            footer: None,
        };
        let card_v2 = render_google_chat_card(&card);
        assert_eq!(card_v2["cardId"], "richCard");
        assert!(card_v2["card"]["header"]["title"].is_string());
    }

    #[test]
    fn render_discord_components_structure() {
        let buttons = vec![ButtonRow {
            buttons: vec![Button {
                label: "Click".into(),
                action: ButtonAction::Callback("click".into()),
            }],
        }];
        let components = render_discord_components(&buttons);
        assert_eq!(components.len(), 1);
        assert_eq!(components[0]["type"], 1); // ActionRow
        let inner = components[0]["components"].as_array().unwrap();
        assert_eq!(inner[0]["type"], 2); // Button
        assert_eq!(inner[0]["style"], 1); // Primary
        assert_eq!(inner[0]["label"], "Click");
    }
}
