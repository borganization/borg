---
name: discord
description: "Interact with Discord via run_shell + curl and the Discord REST API. Use when: sending messages, reacting, managing pins, or reading channels in Discord. Requires DISCORD_BOT_TOKEN env var and curl."
requires:
  bins: ["curl"]
  env: ["DISCORD_BOT_TOKEN"]
---

# Discord Skill

Interact with Discord using `run_shell` and `curl` against the Discord REST API.

## Setup

Set `DISCORD_BOT_TOKEN` in your environment. The bot needs appropriate permissions (Send Messages, Add Reactions, Manage Messages, Read Message History).

## Common Operations

### Send a message

```bash
curl -s -X POST "https://discord.com/api/v10/channels/CHANNEL_ID/messages" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"Hello from Tamagotchi"}'
```

### React to a message

```bash
curl -s -X PUT "https://discord.com/api/v10/channels/CHANNEL_ID/messages/MESSAGE_ID/reactions/%E2%9C%85/@me" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN"
```

### Read channel messages

```bash
curl -s "https://discord.com/api/v10/channels/CHANNEL_ID/messages?limit=20" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN"
```

### Edit a message

```bash
curl -s -X PATCH "https://discord.com/api/v10/channels/CHANNEL_ID/messages/MESSAGE_ID" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"content":"Updated message"}'
```

### Delete a message

```bash
curl -s -X DELETE "https://discord.com/api/v10/channels/CHANNEL_ID/messages/MESSAGE_ID" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN"
```

### Pin a message

```bash
curl -s -X PUT "https://discord.com/api/v10/channels/CHANNEL_ID/pins/MESSAGE_ID" \
  -H "Authorization: Bot $DISCORD_BOT_TOKEN"
```

## Notes

- Use snowflake IDs for channels, messages, users, and guilds
- Emoji in reactions must be URL-encoded (e.g. `%E2%9C%85` for check mark)
- Mention users as `<@USER_ID>` in message content
- Avoid markdown tables in Discord messages
- Rate limits apply; respect `X-RateLimit-*` headers
