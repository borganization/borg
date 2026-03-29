# Microsoft Teams Setup

## 1. Register a Bot in Azure

Go to the [Azure Portal](https://portal.azure.com/) and create a new **Bot Channels Registration** (or **Azure Bot** resource):

1. Create a new Azure AD App Registration
2. Note the **Application (client) ID** and create a **Client Secret**
3. Under **Channels**, add the **Microsoft Teams** channel

## 2. Store Credentials

Add credentials to `~/.borg/config.toml`:

```toml
[credentials]
# Option A: environment variable
TEAMS_APP_ID = "TEAMS_APP_ID"
TEAMS_APP_SECRET = "TEAMS_APP_SECRET"

# Option B: macOS Keychain
TEAMS_APP_ID = { source = "exec", command = "security", args = ["find-generic-password", "-s", "teams-app-id", "-w"] }
TEAMS_APP_SECRET = { source = "exec", command = "security", args = ["find-generic-password", "-s", "teams-app-secret", "-w"] }

# Option C: file
TEAMS_APP_SECRET = { source = "file", path = "~/.config/teams/secret" }
```

## 3. Enable the Gateway

```toml
[gateway]
host = "127.0.0.1"
port = 7842
```

## 4. Set the Messaging Endpoint

Expose a public URL (e.g. via ngrok):

```sh
ngrok http 7842
```

In the Azure Bot Configuration, set the **Messaging endpoint** to:
```
https://your-domain.ngrok-free.app/webhook/teams
```

## 5. Start the Gateway

```sh
borg gateway
```

The gateway also runs automatically as part of the daemon.

## 6. Verify

Message the bot in Microsoft Teams. You should get a response from your agent.

## Features

- Direct messages and channel messages
- Service principal authentication (OAuth2 token exchange)
- Adaptive card support
- Automatic message chunking

## Additional configuration

### Access control

```toml
[gateway.channel_policies]
teams = "open"   # trust Teams workspace auth
```
