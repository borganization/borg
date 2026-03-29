# iMessage Setup

iMessage integration is macOS-only. It monitors the iMessage database directly -- no webhook or external service needed.

## Requirements

- **macOS only** (uses `#[cfg(target_os = "macos")]`)
- **Full Disk Access**: The terminal running Borg needs Full Disk Access to read `~/Library/Messages/chat.db`
- **osascript**: Used to send replies via AppleScript (included with macOS)
- **python3**: Used for message parsing

## 1. Grant Full Disk Access

Go to **System Settings** > **Privacy & Security** > **Full Disk Access** and add your terminal application (Terminal.app, iTerm2, etc.).

## 2. Enable via Plugin

```sh
borg add imessage
```

Or install via the TUI: run `borg` and use `/plugins` to find and install iMessage.

## 3. Start the Gateway

```sh
borg gateway
```

The gateway monitors `~/Library/Messages/chat.db` for new messages using SQLite polling. No public URL or port forwarding is required.

## 4. Verify

Send an iMessage to the Mac running Borg. You should get a response from your agent.

## Features

- Direct iMessage monitoring (no external API)
- Automatic echo detection (won't reply to its own messages)
- Reflection guards (prevents reply loops)
- Self-chat cache (handles messages to/from self)
- Message sanitization

## Additional configuration

### Access control

```toml
[gateway.channel_policies]
imessage = "pairing"   # pairing (default) | open | disabled
```

## Limitations

- macOS only -- not available on Linux or Windows
- Requires Full Disk Access permission
- Uses AppleScript for sending, which may prompt for accessibility permissions
