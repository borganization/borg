# Telegram Setup

## 1. Create a Bot

Open Telegram, message [@BotFather](https://t.me/BotFather), and run `/newbot`. Copy the bot token it gives you.

## 2. Store the Token

Add the token to `~/.borg/config.toml` under `[credentials]`:

```toml
# Option A: environment variable
TELEGRAM_BOT_TOKEN = "TELEGRAM_BOT_TOKEN"

# Option B: macOS Keychain
TELEGRAM_BOT_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "telegram-bot", "-w"] }

# Option C: file
TELEGRAM_BOT_TOKEN = { source = "file", path = "~/.config/telegram/token" }

# Option D: explicit env var
TELEGRAM_BOT_TOKEN = { source = "env", var = "TELEGRAM_BOT_TOKEN" }
```

## 3. Enable the Gateway

```toml
[gateway]
enabled = true
host = "127.0.0.1"
port = 7842
```

## 4. Choose a Mode

### Webhook Mode (recommended for production)

Expose a public URL (e.g. via ngrok) and set it in config:

```toml
[gateway]
public_url = "https://your-domain.ngrok-free.app"
```

The gateway will automatically register `https://your-domain.ngrok-free.app/webhook/telegram` with Telegram.

Quick start with ngrok:

```sh
ngrok http 7842
```

### Polling Mode (no public URL needed)

If `public_url` is not set, the gateway automatically uses long-polling via `getUpdates`. No port forwarding or public server required.

## 5. Webhook Secret (Optional)

For webhook mode, add a secret to verify incoming requests:

```toml
[credentials]
TELEGRAM_WEBHOOK_SECRET = "your-random-secret-string"
```

The secret is passed to Telegram during webhook registration and verified on each incoming request via the `X-Telegram-Bot-Api-Secret-Token` header.

## 6. Start the Gateway

```sh
borg gateway
```

The gateway also runs automatically as part of the daemon.

## 7. Verify

Open your bot in Telegram and send `/start`. You should get a response from your agent.

## Features

- Text messages, edited messages, callback queries
- Photo, document, video, audio, voice, and sticker messages (with placeholders)
- Forum/topic support in supergroups
- Automatic message chunking (4000 char limit)
- Markdown-to-HTML response formatting
- Rate-limit retry with exponential backoff
- Update deduplication
- Typing indicators with circuit breaker protection
- Sequential per-chat message processing
