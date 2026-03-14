# Configuration

Configuration lives at `~/.tamagotchi/config.toml`. All fields have defaults — the file can be empty or omitted entirely.

Run `tamagotchi init` to generate a config with default values.

## Full reference

```toml
[llm]
api_key_env = "OPENROUTER_API_KEY"    # env var name containing the API key
model = "anthropic/claude-sonnet-4"   # OpenRouter model identifier
temperature = 0.7                      # sampling temperature (0.0–2.0)
max_tokens = 4096                      # max tokens per LLM response

[heartbeat]
enabled = false                        # enable proactive check-ins
interval = "30m"                       # firing interval (e.g., "30m", "1h", "45s")
quiet_hours_start = "23:00"            # suppress heartbeats after this time
quiet_hours_end = "07:00"              # resume heartbeats after this time

[tools]
default_timeout_ms = 30000             # subprocess timeout for user tools

[sandbox]
enabled = true                         # enable sandboxing for user tools
mode = "strict"                        # sandbox mode

[memory]
max_context_tokens = 8000              # token budget for memory in system prompt

[skills]
enabled = true                         # enable skills system
max_context_tokens = 4000              # token budget for skills in system prompt
```

## Sections

### `[llm]`

| Field | Default | Description |
|-------|---------|-------------|
| `api_key_env` | `"OPENROUTER_API_KEY"` | Name of the environment variable holding your API key |
| `model` | `"anthropic/claude-sonnet-4"` | Any model available on [OpenRouter](https://openrouter.ai/models) |
| `temperature` | `0.7` | Controls randomness. Lower = more deterministic |
| `max_tokens` | `4096` | Maximum tokens the LLM can generate per response |

### `[heartbeat]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Whether the heartbeat scheduler runs |
| `interval` | `"30m"` | How often the agent checks in. Supports `s`, `m`, `h` suffixes |
| `quiet_hours_start` | none | Time to stop heartbeats (24h format, e.g., `"23:00"`) |
| `quiet_hours_end` | none | Time to resume heartbeats (e.g., `"07:00"`) |

Quiet hours span midnight correctly — `"23:00"` to `"07:00"` means heartbeats are suppressed overnight.

See [Heartbeat](heartbeat.md) for details.

### `[tools]`

| Field | Default | Description |
|-------|---------|-------------|
| `default_timeout_ms` | `30000` | Maximum time a user tool can run before being killed |

### `[sandbox]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Whether user tools run inside a sandbox |
| `mode` | `"strict"` | Sandbox strictness level |

See [Sandboxing](sandboxing.md) for how sandbox policies work.

### `[memory]`

| Field | Default | Description |
|-------|---------|-------------|
| `max_context_tokens` | `8000` | Token budget for memory content injected into the system prompt |

See [Memory](memory.md) for how memory files are loaded.

### `[skills]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Whether to load and inject skills into the system prompt |
| `max_context_tokens` | `4000` | Token budget for skills content |

See [Skills](skills.md) for the skills system.

## Defaults

If `~/.tamagotchi/config.toml` does not exist, all defaults apply. You can also specify partial configs — any omitted field falls back to its default.

```toml
# This is a valid config — only overrides the model
[llm]
model = "meta/llama-3"
```
