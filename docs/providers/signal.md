# Signal Setup

Signal integration uses `signal-cli` as a daemon. Messages are received via Server-Sent Events (SSE), not webhooks.

## 1. Install signal-cli

Install [signal-cli](https://github.com/AsamK/signal-cli):

```sh
# macOS
brew install signal-cli

# Linux (download from releases)
# See https://github.com/AsamK/signal-cli/releases
```

## 2. Register a Phone Number

Register a phone number with Signal via signal-cli:

```sh
signal-cli -a +1234567890 register
signal-cli -a +1234567890 verify CODE
```

## 3. Start the signal-cli Daemon

Run signal-cli in daemon mode:

```sh
signal-cli -a +1234567890 daemon --http
```

By default, the daemon listens on `localhost:8080`.

## 4. Store Credentials

Add your Signal account to `~/.borg/config.toml`:

```toml
[credentials]
SIGNAL_ACCOUNT = "+1234567890"
```

## 5. Configure the Gateway

```toml
[gateway]
host = "127.0.0.1"
port = 7842

# Optional: override signal-cli daemon location
# signal_cli_host = "localhost"
# signal_cli_port = 8080
```

## 6. Start the Gateway

```sh
borg gateway
```

The gateway connects to the signal-cli daemon via SSE to receive incoming messages and uses the HTTP API to send responses.

## 7. Verify

Send a message to the registered phone number on Signal. You should get a response from your agent.

## Features

- Direct messages
- Server-Sent Events (SSE) for real-time message reception
- Message deduplication
- Automatic message chunking

## Additional configuration

### Access control

```toml
[gateway.channel_policies]
signal = "pairing"   # pairing (default) | open | disabled
```

### signal-cli connection

```toml
[gateway]
signal_cli_host = "localhost"   # signal-cli daemon host
signal_cli_port = 8080          # signal-cli daemon port
```
