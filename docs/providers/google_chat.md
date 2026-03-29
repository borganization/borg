# Google Chat Setup

## 1. Create a Google Chat App

Go to the [Google Cloud Console](https://console.cloud.google.com/):

1. Create a new project (or use an existing one)
2. Enable the **Google Chat API**
3. Under **Configuration**, set up the Chat app:
   - **App name** and **description**
   - **Connection settings**: select **HTTP endpoint URL**
   - Set the URL to your gateway's webhook endpoint

## 2. Store Credentials

Add the webhook verification token to `~/.borg/config.toml`:

```toml
[credentials]
# Option A: environment variable
GOOGLE_CHAT_WEBHOOK_TOKEN = "GOOGLE_CHAT_WEBHOOK_TOKEN"

# Option B: macOS Keychain
GOOGLE_CHAT_WEBHOOK_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "gchat-token", "-w"] }
```

## 3. Enable the Gateway

```toml
[gateway]
host = "127.0.0.1"
port = 7842
```

## 4. Set the Webhook URL

Expose a public URL (e.g. via ngrok):

```sh
ngrok http 7842
```

In the Google Chat API configuration, set the HTTP endpoint URL to:
```
https://your-domain.ngrok-free.app/webhook/google-chat
```

The gateway also accepts `/webhook/google_chat` and `/webhook/googlechat` as aliases.

## 5. Start the Gateway

```sh
borg gateway
```

The gateway also runs automatically as part of the daemon.

## 6. Verify

Add the bot to a Google Chat space or send it a direct message. You should get a response from your agent.

## Features

- Direct messages and space messages
- Token verification
- Bot-to-space messaging

## Additional configuration

### Access control

```toml
[gateway.channel_policies]
google-chat = "open"   # trust Google Workspace auth
```
