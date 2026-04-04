# Configuration

Configuration lives at `~/.borg/config.toml`. All fields have defaults — the file can be empty or omitted entirely.

Run `borg init` to generate a config with default values. Runtime overrides can be set via `borg settings set KEY VALUE` (stored in SQLite, highest priority).

## Settings resolution

Three-tier resolution (highest priority wins): **Database** -> **config.toml** -> **Compiled defaults**.

Use `borg settings get KEY` to see the effective value and its source.

## Full reference

```toml
[llm]
provider = "openrouter"               # openrouter | openai | anthropic | gemini | deepseek | groq | ollama
api_key_env = "OPENROUTER_API_KEY"    # env var name containing the API key
model = "anthropic/claude-sonnet-4"   # model identifier
temperature = 0.7                      # sampling temperature (0.0-2.0)
max_tokens = 4096                      # max tokens per LLM response
max_retries = 3                        # retry attempts on transient failures
initial_retry_delay_ms = 200           # initial backoff delay between retries
request_timeout_ms = 60000             # HTTP request timeout
# base_url = "https://custom-endpoint/v1/chat/completions"  # optional: override provider's default URL
thinking = "off"                       # extended thinking: off | low | medium | high | xhigh

# Alternative: SecretRef for API key (priority over api_key_env)
# api_key = { source = "exec", command = "security", args = ["find-generic-password", "-s", "openrouter", "-w"] }

# Fallback chain: tried in order when primary provider fails
# [[llm.fallback]]
# provider = "openai"
# model = "gpt-4.1"
# api_key_env = "OPENAI_API_KEY"

[heartbeat]
enabled = false                        # enable proactive check-ins
interval = "30m"                       # firing interval (e.g., "30m", "1h", "45s")
cron = "0 */30 * * * *"               # optional cron expression (overrides interval)
quiet_hours_start = "00:00"            # suppress heartbeats after this time
quiet_hours_end = "06:00"              # resume heartbeats after this time
# channels = ["telegram"]             # deliver heartbeat to channels (empty = TUI only)

[tools]
default_timeout_ms = 30000             # subprocess timeout for scripts

[tools.policy]
profile = "full"                       # tool profile: minimal | coding | messaging | full
allow = []                             # additional tools/groups to allow (e.g., "group:web")
deny = []                              # tools/groups to deny
subagent_deny = ["manage_tasks", "security_audit", "browser"]  # tools denied to sub-agents

[sandbox]
enabled = true                         # enable sandboxing for scripts
mode = "strict"                        # sandbox mode

[memory]
max_context_tokens = 8000              # token budget for memory in system prompt
# memory_scope = "my-project"          # optional: scope memory to a namespace
# flush_before_compaction = false       # flush memory before compaction
# flush_soft_threshold_tokens = 4000    # soft threshold for memory flush

[memory.embeddings]
enabled = true                         # enable semantic memory search
# provider = "openai"                  # optional: override (auto-detects from API keys)
# model = "text-embedding-3-small"     # optional: override embedding model
# dimension = 1536                     # optional: override vector dimension
# api_key_env = "OPENAI_API_KEY"       # optional: override API key env var
recency_weight = 0.2                   # 0.0=pure similarity, 1.0=pure recency
# chunk_size_tokens = 512              # chunk size for memory embedding
# chunk_overlap_tokens = 64            # overlap between chunks
# bm25_weight = 0.3                    # BM25 keyword weight in hybrid search
# vector_weight = 0.7                  # vector similarity weight in hybrid search

[skills]
enabled = true                         # enable skills system
max_context_tokens = 4000              # token budget for skills in system prompt

# Per-skill config overrides
# [skills.entries.my-skill]
# disabled = true

[conversation]
max_history_tokens = 32000             # token budget for conversation history
max_iterations = 25                    # max agent loop iterations per turn
show_thinking = true                   # display LLM thinking/reasoning output
tool_output_max_tokens = 8000          # max tokens per tool output before truncation
compaction_marker_tokens = 1000        # tokens reserved for compaction markers
max_transcript_chars = 100000          # max total transcript characters in TUI

[user]
name = ""                              # your name (used in system prompt)
agent_name = ""                        # the agent's name
timezone = ""                          # IANA timezone (e.g., "America/New_York") for quiet hours

[policy]
auto_approve = []                      # glob patterns for auto-approved shell commands
deny = []                              # glob patterns for denied shell commands

[debug]
llm_logging = false                    # log all LLM requests/responses to daily JSONL files

[security]
secret_detection = true                # auto-redact secrets in tool output
blocked_paths = [".ssh", ".aws", ".gnupg", ".config/gh", ".env", "credentials", "private_key"]
host_audit = true                      # enable host security checks in doctor + security_audit tool

[security.action_limits]
tool_calls_warn = 50
tool_calls_block = 100
shell_commands_warn = 20
shell_commands_block = 50
file_writes_warn = 15
file_writes_block = 30
memory_writes_warn = 10
memory_writes_block = 20
web_requests_warn = 20
web_requests_block = 50

# Gateway uses stricter defaults (override here if needed)
# [security.gateway_action_limits]
# tool_calls_warn = 50
# tool_calls_block = 150

[web]
enabled = true                         # enable web_fetch and web_search tools
search_provider = "duckduckgo"         # search backend: "duckduckgo" or "brave"
search_api_key_env = ""                # env var for Brave API key (if using brave)

[tasks]
max_concurrent = 3                     # max concurrently running tasks

[budget]
monthly_token_limit = 1000000          # 0 = unlimited
warning_threshold = 0.8                # warn at 80% usage

[browser]
enabled = true                         # enable/disable browser automation
headless = true                        # run headless (no visible window)
# executable = "/path/to/chrome"       # optional: override auto-detected Chrome path
cdp_port = 9222                        # Chrome DevTools Protocol port
no_sandbox = false                     # disable Chrome sandboxing (containers)
timeout_ms = 30000                     # default command timeout
startup_timeout_ms = 15000             # browser launch timeout

[gateway]
host = "127.0.0.1"                     # listen address
port = 7842                            # listen port
max_concurrent = 10                    # max concurrent webhook handlers
request_timeout_ms = 120000            # request processing timeout
rate_limit_per_minute = 60             # per-sender rate limit
# public_url = "https://example.ngrok-free.app"  # for Telegram webhook registration
# max_body_size = 10485760             # max request body (bytes)

# Telegram-specific
# telegram_poll_timeout_secs = 30
# telegram_circuit_failure_threshold = 5
# telegram_circuit_suspension_secs = 60
# telegram_dedup_capacity = 10000

# Channel allowlists
# slack_channel_allowlist = ["C01234567"]
# discord_guild_allowlist = ["123456789"]

# Signal integration
# signal_cli_host = "localhost"
# signal_cli_port = 8080

# Access control
dm_policy = "pairing"                  # pairing | open | disabled
pairing_ttl_secs = 3600               # pairing code expiration (default 60 min)

[gateway.channel_policies]
# telegram = "pairing"
# slack = "open"

# Group chat activation
# group_activation = "mention"         # mention (default) | always

# Auto-reply / away mode
[gateway.auto_reply]
enabled = false
# away_message = "I'm away right now"
# queue_messages = true

# Link understanding (auto-fetch URLs in messages)
[gateway.link_understanding]
enabled = false
# max_links = 3
# max_chars_per_link = 10000
# timeout_ms = 10000

# Per-channel LLM overrides via bindings
# [[gateway.bindings]]
# channel = "telegram"                 # glob pattern
# sender = "*"                         # glob pattern
# peer_kind = "direct"                 # direct | group
# provider = "anthropic"
# model = "claude-sonnet-4"
# temperature = 0.5
# identity = "~/.borg/alt-identity.md"
# memory_scope = "telegram"
# thinking = "medium"

[plugins]
enabled = true                         # enable plugin system
auto_verify = true                     # verify plugin file integrity

[agents]
enabled = true                         # enable multi-agent system
max_spawn_depth = 1                    # max nesting depth for sub-agents
max_children_per_agent = 5             # max sub-agents per parent
max_concurrent = 3                     # max concurrent sub-agents

[telemetry]
tracing_enabled = false                # enable OpenTelemetry tracing
metrics_enabled = false                # enable OpenTelemetry metrics
# otlp_endpoint = "http://localhost:4317"
# service_name = "borg"
# sampling_ratio = 1.0

[audio]
enabled = false                        # enable speech-to-text transcription
# max_file_size = 20971520             # 20MB
# min_file_size = 1024                 # 1KB
# language = "en"
# timeout_ms = 30000
# echo_transcript = true

# [[audio.models]]
# provider = "openai"
# model = "whisper-1"
# api_key_env = "OPENAI_API_KEY"

[tts]
enabled = false                        # enable text-to-speech
default_voice = "alloy"                # voice: alloy, echo, fable, onyx, nova, shimmer
default_format = "mp3"                 # format: mp3, opus, aac, flac
# max_text_length = 4096
# timeout_ms = 30000
# auto_mode = false                    # auto-generate voice replies in channels

# [[tts.models]]
# provider = "openai"
# model = "tts-1"
# api_key_env = "OPENAI_API_KEY"

[media]
# max_image_bytes = 6291456            # 6MB
# compression_enabled = true
# max_dimension_px = 2048

[image_gen]
enabled = false                        # enable image generation
# provider = "openai"                  # openai | fal
# model = "dall-e-3"
# api_key_env = "OPENAI_API_KEY"
# default_size = "1024x1024"

[credentials]
# Bare string = environment variable name (legacy)
JIRA_API_TOKEN = "JIRA_API_TOKEN"

# SecretRef: env var (explicit)
MY_SECRET = { source = "env", var = "MY_SECRET" }

# SecretRef: file
GITHUB_TOKEN = { source = "file", path = "~/.config/gh/token" }

# SecretRef: exec (e.g., macOS Keychain)
SLACK_BOT_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "slack", "-w"] }
```

## Sections

### `[llm]`

| Field | Default | Description |
|-------|---------|-------------|
| `provider` | auto-detected | LLM provider: `openrouter`, `openai`, `anthropic`, `gemini`, `deepseek`, `groq`, or `ollama` |
| `api_key_env` | `"OPENROUTER_API_KEY"` | Name of the environment variable holding your API key |
| `model` | `"anthropic/claude-sonnet-4"` | Model identifier (format depends on provider) |
| `temperature` | `0.7` | Controls randomness. Lower = more deterministic |
| `max_tokens` | `4096` | Maximum tokens the LLM can generate per response |
| `max_retries` | `3` | Number of retry attempts on transient LLM failures |
| `initial_retry_delay_ms` | `200` | Initial delay between retries (doubles each attempt) |
| `request_timeout_ms` | `60000` | HTTP request timeout in milliseconds |
| `base_url` | none | Override the provider's default API endpoint |
| `thinking` | `"off"` | Extended thinking level: `off`, `low`, `medium`, `high`, `xhigh` |

If `provider` is omitted, it is auto-detected based on which API key environment variable is set (`OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `DEEPSEEK_API_KEY`, `GROQ_API_KEY`, or a running Ollama instance).

### `[heartbeat]`

| Field | Default | Description |
|-------|---------|-------------|
| `interval` | `"30m"` | How often the agent checks in. Supports `s`, `m`, `h` suffixes |
| `cron` | none | Cron expression (overrides `interval` if set) |
| `quiet_hours_start` | `"00:00"` | Time to stop heartbeats (24h format) |
| `quiet_hours_end` | `"06:00"` | Time to resume heartbeats |
| `channels` | `[]` | Channels to deliver heartbeat to (e.g., `["telegram"]`) |

Quiet hours use the timezone from `[user] timezone`. See [Heartbeat](heartbeat.md) for details.

### `[tools]`

| Field | Default | Description |
|-------|---------|-------------|
| `default_timeout_ms` | `30000` | Maximum time a user tool can run before being killed |

#### `[tools.policy]`

| Field | Default | Description |
|-------|---------|-------------|
| `profile` | `"full"` | Tool profile preset: `minimal`, `coding`, `messaging`, `full` |
| `allow` | `[]` | Additional tools or groups to allow (e.g., `"group:web"`) |
| `deny` | `[]` | Tools or groups to deny |
| `subagent_deny` | `["manage_tasks", "security_audit", "browser"]` | Tools denied to sub-agents |

### `[sandbox]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Whether scripts run inside a sandbox |
| `mode` | `"strict"` | Sandbox strictness level |

See [Sandboxing](sandboxing.md) for how sandbox policies work.

### `[memory]`

| Field | Default | Description |
|-------|---------|-------------|
| `max_context_tokens` | `8000` | Token budget for memory content injected into the system prompt |
| `memory_scope` | none | Optional namespace for scoped memory |

#### `[memory.embeddings]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Enable semantic memory search |
| `provider` | auto-detected | Embedding provider: `openai`, `openrouter`, or `gemini` |
| `model` | provider default | Embedding model |
| `dimension` | model default | Vector dimension |
| `recency_weight` | `0.2` | Blend weight (0.0=pure similarity, 1.0=pure recency) |
| `bm25_weight` | `0.3` | BM25 keyword score weight in hybrid search |
| `vector_weight` | `0.7` | Vector similarity weight in hybrid search |

See [Memory](memory.md) for details.

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
| `tool_output_max_tokens` | `8000` | Max tokens per tool output before truncation |
| `compaction_marker_tokens` | `1000` | Tokens reserved for compaction markers |
| `max_transcript_chars` | `100000` | Max total transcript characters in TUI |

### `[user]`

| Field | Default | Description |
|-------|---------|-------------|
| `name` | `""` | Your name (included in the system prompt for personalization) |
| `agent_name` | `""` | The agent's name (set during `borg init`) |
| `timezone` | `""` | IANA timezone string (e.g., `"America/New_York"`) for quiet hours |

### `[policy]`

| Field | Default | Description |
|-------|---------|-------------|
| `auto_approve` | `[]` | Glob patterns for shell commands that run without confirmation |
| `deny` | `[]` | Glob patterns for shell commands that are always blocked |

Commands matching `deny` are rejected. Commands matching `auto_approve` run without prompting. All other commands require user confirmation.

### `[debug]`

| Field | Default | Description |
|-------|---------|-------------|
| `llm_logging` | `false` | Log all LLM requests and responses to daily JSONL files in `~/.borg/logs/` |

### `[security]`

| Field | Default | Description |
|-------|---------|-------------|
| `secret_detection` | `true` | Auto-redact detected secrets in tool output |
| `blocked_paths` | `[".ssh", ".aws", ...]` | Paths blocked from tool sandbox access |
| `host_audit` | `true` | Enable host security checks in doctor + security_audit tool |

Rate limits are configured per action type with warn and block thresholds. See `[security.action_limits]` in the full reference above.

### `[web]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Whether `web_fetch` and `web_search` tools are available |
| `search_provider` | `"duckduckgo"` | Search backend: `"duckduckgo"` (no key needed) or `"brave"` |
| `search_api_key_env` | `""` | Environment variable name for Brave Search API key |

### `[tasks]`

| Field | Default | Description |
|-------|---------|-------------|
| `max_concurrent` | `3` | Maximum number of tasks that can run concurrently |

Scheduled tasks require the daemon to be running (`borg daemon`). See [Heartbeat](heartbeat.md) for daemon mode details.

### `[budget]`

| Field | Default | Description |
|-------|---------|-------------|
| `monthly_token_limit` | `1000000` | Monthly token limit (0 = unlimited) |
| `warning_threshold` | `0.8` | Warn at this fraction of the budget |

### `[browser]`

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Enable/disable browser automation |
| `headless` | `true` | Run headless (no visible window) |
| `executable` | auto-detected | Override Chrome/Chromium path |
| `cdp_port` | `9222` | Chrome DevTools Protocol port |
| `no_sandbox` | `false` | Disable Chrome sandboxing (for containers) |
| `timeout_ms` | `30000` | Default command timeout |
| `startup_timeout_ms` | `15000` | Browser launch timeout |

### `[gateway]`

| Field | Default | Description |
|-------|---------|-------------|
| `host` | `"127.0.0.1"` | Listen address |
| `port` | `7842` | Listen port |
| `max_concurrent` | `10` | Max concurrent webhook handlers |
| `request_timeout_ms` | `120000` | Request processing timeout |
| `rate_limit_per_minute` | `60` | Per-sender rate limit |
| `public_url` | none | Public URL for webhook registration (Telegram) |
| `dm_policy` | `"pairing"` | Access control: `pairing`, `open`, or `disabled` |
| `pairing_ttl_secs` | `3600` | Pairing code expiration |

See provider docs in [providers/](providers/) for per-channel setup.

### `[credentials]`

A key-value map for credential resolution. Values can be bare strings (env var names) or `SecretRef` objects:

- `{ source = "env", var = "NAME" }` — read from environment variable
- `{ source = "file", path = "~/.config/secret" }` — read from file
- `{ source = "exec", command = "cmd", args = [...] }` — execute command (e.g., macOS Keychain)

## Defaults

If `~/.borg/config.toml` does not exist, all defaults apply. You can also specify partial configs — any omitted field falls back to its default.

```toml
# This is a valid config — only overrides the model
[llm]
model = "meta/llama-3"
```
