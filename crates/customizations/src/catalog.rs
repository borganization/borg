use crate::{Category, CredentialSpec, CustomizationKind, Platform, TemplateFile, TemplateTarget};

/// A static customization definition — embedded in the binary.
#[derive(Debug, Clone)]
pub struct CustomizationDef {
    pub id: &'static str,
    pub name: &'static str,
    pub category: Category,
    pub kind: CustomizationKind,
    pub description: &'static str,
    pub required_credentials: &'static [CredentialSpec],
    pub required_bins: &'static [&'static str],
    pub templates: &'static [TemplateFile],
    pub platform: Platform,
}

// ── Embedded templates via include_str! ──

// Telegram
const TELEGRAM_CHANNEL_TOML: &str = include_str!("../templates/messaging/telegram/channel.toml");
const TELEGRAM_PARSE_INBOUND: &str =
    include_str!("../templates/messaging/telegram/parse_inbound.py");
const TELEGRAM_SEND_OUTBOUND: &str =
    include_str!("../templates/messaging/telegram/send_outbound.py");
const TELEGRAM_VERIFY: &str = include_str!("../templates/messaging/telegram/verify.py");

// WhatsApp
const WHATSAPP_CHANNEL_TOML: &str = include_str!("../templates/messaging/whatsapp/channel.toml");
const WHATSAPP_PARSE_INBOUND: &str =
    include_str!("../templates/messaging/whatsapp/parse_inbound.py");
const WHATSAPP_SEND_OUTBOUND: &str =
    include_str!("../templates/messaging/whatsapp/send_outbound.py");
const WHATSAPP_VERIFY: &str = include_str!("../templates/messaging/whatsapp/verify.py");

// iMessage
const IMESSAGE_CHANNEL_TOML: &str = include_str!("../templates/messaging/imessage/channel.toml");
const IMESSAGE_POLL: &str = include_str!("../templates/messaging/imessage/poll_messages.py");
const IMESSAGE_OUTBOUND: &str = include_str!("../templates/messaging/imessage/send_outbound.sh");
const IMESSAGE_POLICY: &str = include_str!("../templates/messaging/imessage/policy.json");
const IMESSAGE_STATE: &str = include_str!("../templates/messaging/imessage/state.json");

// SMS
const SMS_CHANNEL_TOML: &str = include_str!("../templates/messaging/sms/channel.toml");
const SMS_PARSE_INBOUND: &str = include_str!("../templates/messaging/sms/parse_inbound.py");
const SMS_SEND_OUTBOUND: &str = include_str!("../templates/messaging/sms/send_outbound.py");
const SMS_VERIFY: &str = include_str!("../templates/messaging/sms/verify.py");

// Gmail
const GMAIL_TOOL_TOML: &str = include_str!("../templates/email/gmail/tool.toml");
const GMAIL_MAIN: &str = include_str!("../templates/email/gmail/main.py");

// Outlook
const OUTLOOK_TOOL_TOML: &str = include_str!("../templates/email/outlook/tool.toml");
const OUTLOOK_MAIN: &str = include_str!("../templates/email/outlook/main.py");

// Google Calendar
const GCAL_TOOL_TOML: &str = include_str!("../templates/productivity/google-calendar/tool.toml");
const GCAL_MAIN: &str = include_str!("../templates/productivity/google-calendar/main.py");

// Notion
const NOTION_TOOL_TOML: &str = include_str!("../templates/productivity/notion/tool.toml");
const NOTION_MAIN: &str = include_str!("../templates/productivity/notion/main.py");

// Linear
const LINEAR_TOOL_TOML: &str = include_str!("../templates/productivity/linear/tool.toml");
const LINEAR_MAIN: &str = include_str!("../templates/productivity/linear/main.py");

// ── Catalog entries ──

pub static CATALOG: &[CustomizationDef] = &[
    // ── Messaging ──
    CustomizationDef {
        id: "messaging/telegram",
        name: "Telegram",
        category: Category::Messaging,
        kind: CustomizationKind::Channel,
        description: "Telegram bot integration for bidirectional messaging",
        required_credentials: &[CredentialSpec {
            key: "TELEGRAM_BOT_TOKEN",
            label: "Telegram Bot Token",
            help_url: "https://core.telegram.org/bots#botfather",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "telegram/channel.toml",
                content: TELEGRAM_CHANNEL_TOML,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "telegram/parse_inbound.py",
                content: TELEGRAM_PARSE_INBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "telegram/send_outbound.py",
                content: TELEGRAM_SEND_OUTBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "telegram/verify.py",
                content: TELEGRAM_VERIFY,
                target: TemplateTarget::Channels,
            },
        ],
        platform: Platform::All,
    },
    CustomizationDef {
        id: "messaging/whatsapp",
        name: "WhatsApp",
        category: Category::Messaging,
        kind: CustomizationKind::Channel,
        description: "WhatsApp Business API via Twilio",
        required_credentials: &[
            CredentialSpec {
                key: "TWILIO_ACCOUNT_SID",
                label: "Twilio Account SID",
                help_url: "https://www.twilio.com/docs/iam/api/account",
                is_optional: false,
            },
            CredentialSpec {
                key: "TWILIO_AUTH_TOKEN",
                label: "Twilio Auth Token",
                help_url: "https://www.twilio.com/docs/iam/api/account",
                is_optional: false,
            },
        ],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "whatsapp/channel.toml",
                content: WHATSAPP_CHANNEL_TOML,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "whatsapp/parse_inbound.py",
                content: WHATSAPP_PARSE_INBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "whatsapp/send_outbound.py",
                content: WHATSAPP_SEND_OUTBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "whatsapp/verify.py",
                content: WHATSAPP_VERIFY,
                target: TemplateTarget::Channels,
            },
        ],
        platform: Platform::All,
    },
    CustomizationDef {
        id: "messaging/imessage",
        name: "iMessage",
        category: Category::Messaging,
        kind: CustomizationKind::Channel,
        description: "Bidirectional iMessage via macOS Messages (macOS only)",
        required_credentials: &[],
        required_bins: &["osascript", "python3"],
        templates: &[
            TemplateFile {
                relative_path: "imessage/channel.toml",
                content: IMESSAGE_CHANNEL_TOML,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "imessage/poll_messages.py",
                content: IMESSAGE_POLL,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "imessage/send_outbound.sh",
                content: IMESSAGE_OUTBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "imessage/policy.json",
                content: IMESSAGE_POLICY,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "imessage/state.json",
                content: IMESSAGE_STATE,
                target: TemplateTarget::Channels,
            },
        ],
        platform: Platform::MacOS,
    },
    CustomizationDef {
        id: "messaging/sms",
        name: "SMS",
        category: Category::Messaging,
        kind: CustomizationKind::Channel,
        description: "SMS messaging via Twilio",
        required_credentials: &[
            CredentialSpec {
                key: "TWILIO_ACCOUNT_SID",
                label: "Twilio Account SID",
                help_url: "https://www.twilio.com/docs/iam/api/account",
                is_optional: false,
            },
            CredentialSpec {
                key: "TWILIO_AUTH_TOKEN",
                label: "Twilio Auth Token",
                help_url: "https://www.twilio.com/docs/iam/api/account",
                is_optional: false,
            },
        ],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "sms/channel.toml",
                content: SMS_CHANNEL_TOML,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "sms/parse_inbound.py",
                content: SMS_PARSE_INBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "sms/send_outbound.py",
                content: SMS_SEND_OUTBOUND,
                target: TemplateTarget::Channels,
            },
            TemplateFile {
                relative_path: "sms/verify.py",
                content: SMS_VERIFY,
                target: TemplateTarget::Channels,
            },
        ],
        platform: Platform::All,
    },
    // ── Email ──
    CustomizationDef {
        id: "email/gmail",
        name: "Gmail",
        category: Category::Email,
        kind: CustomizationKind::Tool,
        description: "Send and search emails via Gmail API",
        required_credentials: &[CredentialSpec {
            key: "GMAIL_API_KEY",
            label: "Gmail OAuth Token",
            help_url: "https://developers.google.com/gmail/api/quickstart/python",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "gmail/tool.toml",
                content: GMAIL_TOOL_TOML,
                target: TemplateTarget::Tools,
            },
            TemplateFile {
                relative_path: "gmail/main.py",
                content: GMAIL_MAIN,
                target: TemplateTarget::Tools,
            },
        ],
        platform: Platform::All,
    },
    CustomizationDef {
        id: "email/outlook",
        name: "Outlook",
        category: Category::Email,
        kind: CustomizationKind::Tool,
        description: "Send and search emails via Microsoft Graph API",
        required_credentials: &[CredentialSpec {
            key: "MS_GRAPH_TOKEN",
            label: "Microsoft Graph OAuth Token",
            help_url: "https://learn.microsoft.com/en-us/graph/auth/",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "outlook/tool.toml",
                content: OUTLOOK_TOOL_TOML,
                target: TemplateTarget::Tools,
            },
            TemplateFile {
                relative_path: "outlook/main.py",
                content: OUTLOOK_MAIN,
                target: TemplateTarget::Tools,
            },
        ],
        platform: Platform::All,
    },
    // ── Productivity ──
    CustomizationDef {
        id: "productivity/google-calendar",
        name: "Google Calendar",
        category: Category::Productivity,
        kind: CustomizationKind::Tool,
        description: "Manage Google Calendar events",
        required_credentials: &[CredentialSpec {
            key: "GOOGLE_CALENDAR_TOKEN",
            label: "Google Calendar OAuth Token",
            help_url: "https://developers.google.com/calendar/api/quickstart/python",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "google-calendar/tool.toml",
                content: GCAL_TOOL_TOML,
                target: TemplateTarget::Tools,
            },
            TemplateFile {
                relative_path: "google-calendar/main.py",
                content: GCAL_MAIN,
                target: TemplateTarget::Tools,
            },
        ],
        platform: Platform::All,
    },
    CustomizationDef {
        id: "productivity/notion",
        name: "Notion",
        category: Category::Productivity,
        kind: CustomizationKind::Tool,
        description: "Query and create Notion pages and databases",
        required_credentials: &[CredentialSpec {
            key: "NOTION_API_KEY",
            label: "Notion Integration Token",
            help_url: "https://www.notion.so/my-integrations",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "notion/tool.toml",
                content: NOTION_TOOL_TOML,
                target: TemplateTarget::Tools,
            },
            TemplateFile {
                relative_path: "notion/main.py",
                content: NOTION_MAIN,
                target: TemplateTarget::Tools,
            },
        ],
        platform: Platform::All,
    },
    CustomizationDef {
        id: "productivity/linear",
        name: "Linear",
        category: Category::Productivity,
        kind: CustomizationKind::Tool,
        description: "Manage Linear issues and projects",
        required_credentials: &[CredentialSpec {
            key: "LINEAR_API_KEY",
            label: "Linear API Key",
            help_url: "https://linear.app/settings/api",
            is_optional: false,
        }],
        required_bins: &["python3"],
        templates: &[
            TemplateFile {
                relative_path: "linear/tool.toml",
                content: LINEAR_TOOL_TOML,
                target: TemplateTarget::Tools,
            },
            TemplateFile {
                relative_path: "linear/main.py",
                content: LINEAR_MAIN,
                target: TemplateTarget::Tools,
            },
        ],
        platform: Platform::All,
    },
];

/// Look up a customization by ID.
pub fn find_by_id(id: &str) -> Option<&'static CustomizationDef> {
    CATALOG.iter().find(|c| c.id == id)
}

/// Get all entries for a given category.
pub fn by_category(category: Category) -> Vec<&'static CustomizationDef> {
    CATALOG.iter().filter(|c| c.category == category).collect()
}

/// All categories in display order.
pub fn categories() -> &'static [Category] {
    &[Category::Messaging, Category::Email, Category::Productivity]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty() {
        assert!(!CATALOG.is_empty());
    }

    #[test]
    fn all_ids_unique() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
    }

    #[test]
    fn find_by_id_works() {
        assert!(find_by_id("messaging/telegram").is_some());
        assert!(find_by_id("nonexistent").is_none());
    }

    #[test]
    fn templates_have_content() {
        for entry in CATALOG {
            assert!(!entry.templates.is_empty(), "no templates for {}", entry.id);
            for tmpl in entry.templates {
                assert!(
                    !tmpl.content.is_empty(),
                    "empty template {}",
                    tmpl.relative_path
                );
            }
        }
    }

    #[test]
    fn categories_cover_all_entries() {
        for cat in categories() {
            let entries = by_category(*cat);
            assert!(!entries.is_empty(), "no entries for {cat}");
        }
    }
}
