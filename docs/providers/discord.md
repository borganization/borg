# Discord Setup

## 1. Create a Discord Application

Go to the [Discord Developer Portal](https://discord.com/developers/applications) and click **New Application**.

## 2. Create a Bot

Under your application, go to **Bot** and click **Add Bot**. Copy the **Bot Token**.

Under **Privileged Gateway Intents**, enable:
- **Message Content Intent** -- required to read message text

## 3. Configure OAuth2

Under **OAuth2** > **URL Generator**:
- Select scopes: `bot`, `applications.commands`
- Select bot permissions: `Send Messages`, `Read Message History`, `Use Slash Commands`

Copy the generated URL and use it to invite the bot to your server.

## 4. Get the Public Key

Under **General Information**, copy the **Public Key** (used for interaction signature verification).

## 5. Store Credentials

Add credentials to `~/.borg/config.toml`:

```toml
[credentials]
# Option A: environment variable
DISCORD_BOT_TOKEN = "DISCORD_BOT_TOKEN"
DISCORD_PUBLIC_KEY = "DISCORD_PUBLIC_KEY"

# Option B: macOS Keychain
DISCORD_BOT_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "discord-bot", "-w"] }

# Option C: file
DISCORD_BOT_TOKEN = { source = "file", path = "~/.config/discord/token" }

# Option D: explicit env var
DISCORD_BOT_TOKEN = { source = "env", var = "DISCORD_BOT_TOKEN" }
```

## 6. Enable the Gateway

```toml
[gateway]
host = "127.0.0.1"
port = 7842
```

## 7. Set the Interactions Endpoint

Expose a public URL (e.g. via ngrok):

```sh
ngrok http 7842
```

In the Discord Developer Portal, go to **General Information** and set the **Interactions Endpoint URL** to:
```
https://your-domain.ngrok-free.app/webhook/discord
```

Discord will send a verification ping -- the gateway handles this automatically.

## 8. Start the Gateway

```sh
borg gateway
```

The gateway also runs automatically as part of the daemon.

## 9. Verify

Send a message to your bot in Discord (DM or in a server channel where the bot is present). You should get a response from your agent.

## Features

- Direct messages and server channel messages
- Ed25519 interaction signature verification
- Deferred responses for long-running agent turns
- Slash command support
- Automatic message chunking (2000 char limit)
- Typing indicators

## Additional configuration

### Guild allowlist

Restrict the bot to specific Discord servers:

```toml
[gateway]
discord_guild_allowlist = ["123456789012345678"]
```

### Group activation mode

```toml
[gateway]
group_activation = "mention"   # mention (default) | always
```

### Access control

```toml
[gateway.channel_policies]
discord = "pairing"   # pairing (default) | open | disabled
```
