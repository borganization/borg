# Heartbeat

The heartbeat system enables proactive check-ins — the agent can reach out to you at regular intervals without being prompted.

## How it works

1. A separate tokio task runs alongside the REPL
2. At each interval tick, the agent sends a brief prompt to the LLM with its personality and memory context
3. If the LLM has something useful to say, it responds; otherwise it returns an empty message
4. Heartbeat messages render in cyan in the REPL
5. Duplicate and empty responses are suppressed

## Configuration

```toml
[heartbeat]
enabled = false               # must be explicitly enabled
interval = "30m"              # check-in interval
cron = "0 */30 * * * *"       # optional cron expression (overrides interval)
quiet_hours_start = "23:00"   # stop heartbeats at this time
quiet_hours_end = "07:00"     # resume heartbeats at this time
```

### Interval format

| Format | Example | Duration |
|--------|---------|----------|
| Minutes | `"30m"` | 30 minutes |
| Hours | `"2h"` | 2 hours |
| Seconds | `"45s"` | 45 seconds |
| Bare number | `"120"` | 120 seconds |

### Quiet hours

Quiet hours prevent heartbeats during specified time ranges. The range correctly spans midnight — setting `"23:00"` to `"07:00"` suppresses heartbeats overnight.

If quiet hours are not configured, heartbeats fire at every interval.

## Deduplication

If the LLM responds with the exact same message as the previous heartbeat, it's suppressed. This prevents repetitive notifications when nothing has changed.

## Daemon mode

For running heartbeat check-ins and scheduled tasks without the interactive TUI, use daemon mode:

```sh
borg daemon
```

The daemon runs in the foreground as a background-friendly process. It executes heartbeat check-ins and any scheduled tasks on their configured intervals. Combine with `borg service install` to run it as a system service (launchd on macOS, systemd on Linux).

## Heartbeat prompt

Each heartbeat sends the agent's personality (SOUL.md), memory context, and current time, along with instructions to:
- Share something useful, timely, or caring if appropriate
- Respond with an empty message if there's nothing meaningful to say
- Keep it brief — one or two sentences max
