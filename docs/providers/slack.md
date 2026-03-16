# Slack Setup

## 1. Create a Slack App

Go to [api.slack.com/apps](https://api.slack.com/apps) and click **Create New App** > **From scratch**.

## 2. Configure Bot Token Scopes

Under **OAuth & Permissions**, add the following Bot Token Scopes:

- `chat:write` — send messages
- `app_mentions:read` — receive @mentions
- `channels:history` — read messages in public channels the bot is in
- `im:history` — read direct messages

Install the app to your workspace and copy the **Bot User OAuth Token** (`xoxb-...`).

## 3. Store Credentials

Add the bot token and signing secret to `~/.tamagotchi/config.toml`:

```toml
[credentials]
# Option A: environment variable
SLACK_BOT_TOKEN = "SLACK_BOT_TOKEN"
SLACK_SIGNING_SECRET = "SLACK_SIGNING_SECRET"

# Option B: macOS Keychain
SLACK_BOT_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "slack-bot", "-w"] }
SLACK_SIGNING_SECRET = { source = "exec", command = "security", args = ["find-generic-password", "-s", "slack-signing", "-w"] }

# Option C: file
SLACK_BOT_TOKEN = { source = "file", path = "~/.config/slack/bot-token" }

# Option D: explicit env var
SLACK_BOT_TOKEN = { source = "env", var = "SLACK_BOT_TOKEN" }
```

The signing secret is found under **Basic Information** > **App Credentials** in your Slack app settings.

## 4. Enable the Gateway

```toml
[gateway]
enabled = true
host = "127.0.0.1"
port = 7842
```

## 5. Enable Event Subscriptions

Expose a public URL (e.g. via ngrok):

```sh
ngrok http 7842
```

In your Slack app settings, go to **Event Subscriptions**:

1. Toggle **Enable Events** to On
2. Set the Request URL to `https://your-domain.ngrok-free.app/webhook/slack`
3. Slack will send a challenge request — the gateway handles this automatically
4. Subscribe to bot events:
   - `message.im` — direct messages
   - `app_mention` — @mentions in channels

## 6. Start the Gateway

```sh
tamagotchi gateway
```

Or enable it as part of the daemon by setting `gateway.enabled = true`.

## 7. Verify

Invite the bot to a channel or send it a direct message. You should get a response from your agent.

## Features

- Direct messages and @mention events
- HMAC-SHA256 request signature verification
- Replay protection (5-minute timestamp window)
- Thread-aware replies (responds in-thread when messaged in a thread)
- Automatic message chunking (4000 char limit)
- Rate-limit retry with exponential backoff
- Bot message filtering (prevents self-reply loops)
