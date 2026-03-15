# Configuration

Configuration lives at `~/.tamagotchi/config.toml`. All fields have defaults — the file can be empty or omitted entirely.

Run `tamagotchi init` to generate a config with default values.

## Full reference

```toml
[llm]
provider = "openrouter"               # openrouter | openai | anthropic | gemini
api_key_env = "OPENROUTER_API_KEY"    # env var name containing the API key
model = "anthropic/claude-sonnet-4"   # model identifier
temperature = 0.7                      # sampling temperature (0.0–2.0)
max_tokens = 4096                      # max tokens per LLM response
max_retries = 3                        # retry attempts on transient failures
initial_retry_delay_ms = 200           # initial backoff delay between retries
request_timeout_ms = 60000             # HTTP request timeout

[heartbeat]
enabled = false                        # enable proactive check-ins
interval = "30m"                       # firing interval (e.g., "30m", "1h", "45s")
cron = "0 */30 * * * *"               # optional cron expression (overrides interval)
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

[conversation]
max_history_tokens = 32000             # token budget for conversation history
max_iterations = 25                    # max agent loop iterations per turn
show_thinking = true                   # display LLM thinking/reasoning output

[user]
name = ""                              # your name (used in system prompt)
agent_name = ""                        # the agent's name

[policy]
auto_approve = []                      # glob patterns for auto-approved shell commands
deny = []                              # glob patterns for denied shell commands

[debug]
llm_logging = false                    # log all LLM requests/responses to daily JSONL files

[security]
secret_detection = true                # auto-redact secrets in tool output

[web]
enabled = true                         # enable web_fetch and web_search tools
search_provider = "duckduckgo"         # search backend: "duckduckgo" or "brave"
search_api_key_env = ""                # env var for Brave API key (if using brave)

[tasks]
enabled = false                        # enable scheduled task tools
max_concurrent = 3                     # max concurrently running tasks

[credentials]
# Custom key-value pairs for tool credentials
# Example: my_api_key = "MY_API_KEY_ENV_VAR"
```

## Sections

### `[llm]`

| Field | Default | Description |
|-------|---------|-------------|
| `provider` | auto-detected | LLM provider: `openrouter`, `openai`, `anthropic`, or `gemini` |
| `api_key_env` | `"OPENROUTER_API_KEY"` | Name of the environment variable holding your API key |
| `model` | `"anthropic/claude-sonnet-4"` | Model identifier (format depends on provider) |
| `temperature` | `0.7` | Controls randomness. Lower = more deterministic |
| `max_tokens` | `4096` | Maximum tokens the LLM can generate per response |
| `max_retries` | `3` | Number of retry attempts on transient LLM failures |
| `initial_retry_delay_ms` | `200` | Initial delay between retries (doubles each attempt) |
| `request_timeout_ms` | `60000` | HTTP request timeout in milliseconds |

If `provider` is omitted, it is auto-detected based on which API key environment variable is set (`OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`).

### `[heartbeat]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Whether the heartbeat scheduler runs |
| `interval` | `"30m"` | How often the agent checks in. Supports `s`, `m`, `h` suffixes |
| `cron` | none | Cron expression (overrides `interval` if set) |
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

### `[conversation]`

| Field | Default | Description |
|-------|---------|-------------|
| `max_history_tokens` | `32000` | Token budget for conversation history before compaction triggers |
| `max_iterations` | `25` | Maximum agent loop iterations per turn (prevents runaway tool loops) |
| `show_thinking` | `true` | Whether to display LLM thinking/reasoning output in the TUI |

### `[user]`

| Field | Default | Description |
|-------|---------|-------------|
| `name` | `""` | Your name (included in the system prompt for personalization) |
| `agent_name` | `""` | The agent's name (set during `tamagotchi init`) |

### `[policy]`

| Field | Default | Description |
|-------|---------|-------------|
| `auto_approve` | `[]` | Glob patterns for shell commands that run without confirmation |
| `deny` | `[]` | Glob patterns for shell commands that are always blocked |

Commands matching `deny` are rejected. Commands matching `auto_approve` run without prompting. All other commands require user confirmation.

### `[debug]`

| Field | Default | Description |
|-------|---------|-------------|
| `llm_logging` | `false` | Log all LLM requests and responses to daily JSONL files in `~/.tamagotchi/logs/` |

### `[security]`

| Field | Default | Description |
|-------|---------|-------------|
| `secret_detection` | `true` | Auto-redact detected secrets (API keys, tokens, passwords) in tool output |

### `[web]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Whether `web_fetch` and `web_search` tools are available |
| `search_provider` | `"duckduckgo"` | Search backend: `"duckduckgo"` (no key needed) or `"brave"` |
| `search_api_key_env` | `""` | Environment variable name for Brave Search API key |

### `[tasks]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `false` | Whether scheduled task tools are available |
| `max_concurrent` | `3` | Maximum number of tasks that can run concurrently |

Scheduled tasks require the daemon to be running (`tamagotchi daemon`). See [Heartbeat](heartbeat.md) for daemon mode details.

### `[credentials]`

A free-form key-value map for tool credentials. Values are environment variable names (not the secrets themselves).

```toml
[credentials]
weather_api = "WEATHER_API_KEY"
slack_token = "SLACK_BOT_TOKEN"
```

## Defaults

If `~/.tamagotchi/config.toml` does not exist, all defaults apply. You can also specify partial configs — any omitted field falls back to its default.

```toml
# This is a valid config — only overrides the model
[llm]
model = "meta/llama-3"
```
