---
name: slack
description: "Interact with Slack via run_shell + curl and the Slack Web API. Use when: sending messages, reacting, pinning, reading channels, or managing Slack workflows. Requires SLACK_BOT_TOKEN env var and curl."
requires:
  bins: ["curl"]
  env: ["SLACK_BOT_TOKEN"]
---

# Slack Skill

Interact with Slack using `run_shell` and `curl` against the Slack Web API.

## Setup

Set `SLACK_BOT_TOKEN` in your environment. The bot needs appropriate OAuth scopes (chat:write, reactions:write, pins:write, channels:history, users:read).

## Common Operations

### Send a message

```bash
curl -s -X POST https://slack.com/api/chat.postMessage \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"channel":"C123","text":"Hello from Borg"}'
```

### React to a message

```bash
curl -s -X POST https://slack.com/api/reactions.add \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"channel":"C123","timestamp":"1712023032.1234","name":"white_check_mark"}'
```

### Read channel history

```bash
curl -s "https://slack.com/api/conversations.history?channel=C123&limit=20" \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN"
```

### Pin a message

```bash
curl -s -X POST https://slack.com/api/pins.add \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"channel":"C123","timestamp":"1712023032.1234"}'
```

### Get user info

```bash
curl -s "https://slack.com/api/users.info?user=U123" \
  -H "Authorization: Bearer $SLACK_BOT_TOKEN"
```

## Notes

- Channel IDs look like `C0123456789`, user IDs like `U0123456789`
- Message timestamps (ts) are used as message IDs (e.g. `1712023032.1234`)
- Responses are JSON; check the `ok` field for success
- Rate limits apply; avoid rapid-fire requests
