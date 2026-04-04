# Architecture

Borg is a Cargo workspace with eight crates. Each crate has a focused responsibility.

## Crate overview

```
crates/
├── cli/          # Binary: TUI, REPL, clap args, heartbeat display, onboarding, daemon/service
├── core/         # Library: agent loop, multi-provider LLM client, memory, identity, config
├── heartbeat/    # Library: proactive scheduler with quiet hours + dedup
├── tools/        # Library: tool manifest parsing, registry, subprocess executor
├── sandbox/      # Library: macOS Seatbelt + Linux Bubblewrap policies
├── apply-patch/  # Library: patch DSL parser + filesystem applicator
├── gateway/      # Library: webhook gateway for messaging channel integrations
└── plugins/      # Library: marketplace catalog, plugin installer, TUI integration
```

### `cli`

Entry point. Defines commands via clap: `start` (default, TUI with auto-gateway), `ask`, `init`, `daemon`, `gateway`, `doctor`, `wake`, `tasks`, `pairing`, `logs`, `usage`, `away`, `available`, `settings`, `add`, `remove`, `plugins`, `stop`, `restart`, `service`, and `uninstall`. Includes a full ratatui-based TUI with markdown rendering, slash command autocomplete, settings popup, plugins marketplace popup, and session management. Handles daemon mode for background task execution and system service installation (launchd on macOS, systemd on Linux).

Key files: `main.rs`, `repl.rs`, `onboarding.rs`, `plugins.rs`, `service.rs`, `tui/app.rs`, `tui/settings_popup.rs`, `tui/command_popup.rs`

### `core`

The heart of the project. Contains:

- **Agent** (`agent.rs`) — the conversation loop. Builds the system prompt, streams LLM responses, dispatches tool calls, and loops until the LLM returns a text-only response. Supports internal tag stripping, message persistence, and conversation compaction.
- **LLM client** (`llm.rs`) — multi-provider streaming SSE client. Supports OpenRouter, OpenAI, Anthropic, Gemini, DeepSeek, Groq, and Ollama. Handles chunked responses, tool call deltas, and error events.
- **Provider** (`provider.rs`) — provider enum, auto-detection from available API keys, and request/header configuration.
- **Config** (`config.rs`) — TOML configuration with serde defaults. Sections: llm, heartbeat, tools, sandbox, memory, skills, conversation, user, policy, debug, security, web, tasks, budget, gateway, plugins, agents, telemetry, browser, audio, tts, media, image_gen, credentials.
- **Memory** (`memory.rs`) — loads `MEMORY.md` and `memory/*.md` files into the system prompt, with semantic search (embeddings) and token budgeting. Supports global, local (per-project), and scoped memory.
- **Embeddings** (`embeddings.rs`) — embedding API client (OpenAI, OpenRouter, Gemini), cosine similarity, and semantic memory ranking with BM25 hybrid search.
- **Identity** (`identity.rs`) — loads and saves `IDENTITY.md`, the agent's personality prompt.
- **Skills** (`skills.rs`) — loads built-in and user skills, checks requirements (bins, env, any_bins, os), and formats them with progressive token budgeting.
- **Types** (`types.rs`) — shared types: `Message` (with timestamps), `ToolCall`, `ToolDefinition`, `Role`.
- **Tool handlers** (`tool_handlers.rs`) — built-in tool implementations (memory, patch, shell, browser, tasks, security audit, image gen, TTS, etc.).
- **Tool policy** (`tool_policy.rs`) — composable tool filtering with profiles (minimal/coding/messaging/full), allow/deny lists, and subagent restrictions.
- **Tool catalog** (`tool_catalog.rs`) — tool group definitions and profile presets.
- **Session** (`session.rs`) — session persistence with JSON serialization, auto-save, and auto-titling.
- **Database** (`db.rs`) — SQLite database with versioned migrations (currently V15). Tables: sessions, messages, scheduled_tasks, task_runs, token_usage, channel_sessions, channel_messages, settings, memory_embeddings, memory_chunks, pairing_requests, approved_senders, and more.
- **Settings** (`settings.rs`) — three-tier settings resolver: Database -> config.toml -> compiled defaults. 35+ configurable keys.
- **Conversation** (`conversation.rs`) — history compaction, token estimation, and conversation normalization.
- **Policy** (`policy.rs`) — execution policy with auto-approve/deny glob patterns for shell commands.
- **Secrets** (`secrets.rs`) — secret detection and redaction (AWS keys, GitHub tokens, JWTs, private keys, etc.).
- **Sanitize** (`sanitize.rs`) — prompt injection detection with scoring-based regex patterns and untrusted content wrapping.
- **Rate guard** (`rate_guard.rs`) — per-session rate limiting for tool calls, shell commands, file/memory writes, and web requests.
- **Web** (`web.rs`) — web fetching (HTML-to-text) and searching (DuckDuckGo/Brave).
- **Tasks** (`tasks.rs`) — scheduled task definitions (cron, interval, once) and next-run calculation.
- **Hooks** (`hooks.rs`) — lifecycle hook system (BeforeAgentStart, BeforeLlmCall, AfterLlmResponse, BeforeToolCall, AfterToolCall, TurnComplete, OnError).
- **Doctor** (`doctor.rs`) — diagnostic checks for config, provider, sandbox, tools, skills, memory, embeddings, gateway, budget, plugins, browser, agents, and host security.
- **Browser** (`browser.rs`) — Chrome detection, CDP session management, browser automation (navigate, click, type, screenshot, get_text, evaluate_js).
- **Host audit** (`host_audit.rs`) — host security audit checks (firewall, ports, SSH, permissions, encryption, updates, services).
- **Pairing** (`pairing.rs`) — sender pairing/access control for gateway messages.
- **Multi-agent** (`multi_agent/`) — multi-agent orchestration with spawn depth limits, role definitions, and concurrent execution.
- **Image gen** (`image_gen.rs`) — image generation via OpenAI/fal providers.
- **TTS** (`tts.rs`) — text-to-speech synthesis.
- **Telemetry** (`telemetry.rs`) — OpenTelemetry tracing and metrics.
- **Media** (`media.rs`) — image compression and media processing.
- **Integrations** (`integrations/`) — native tool integrations (Gmail, Outlook, Google Calendar, Notion, Linear).
- **Logging** (`logging.rs`) — daily JSONL logging of messages and tool calls.
- **Retry** (`retry.rs`) — retry logic with exponential backoff for LLM requests.
- **Tokenizer** (`tokenizer.rs`) — token estimation via tiktoken-rs (cl100k_base BPE tokenizer).
- **Truncate** (`truncate.rs`) — tool output truncation (head+tail strategy).

### `heartbeat`

A proactive scheduler that runs as a separate tokio task. Fires at a configured interval or cron schedule, skips during quiet hours (timezone-aware), and suppresses duplicate or empty LLM responses. Supports channel delivery and wake signals. Messages render in cyan in the TUI.

Key file: `scheduler.rs`

### `sandbox`

Platform-specific sandboxing and script execution:

- **Policy** (`policy.rs`) — `SandboxPolicy` struct with `network`, `fs_read`, `fs_write`, `deny_read`, `deny_write` fields. Automatic `~/.borg/` protection and blocked path filtering.
- **Seatbelt** (`seatbelt.rs`) — generates macOS `sandbox-exec` profiles (deny-all default with explicit allows).
- **Bubblewrap** (`bubblewrap.rs`) — builds `bwrap` arguments for Linux namespace isolation.

### `apply-patch`

A custom patch DSL for creating, modifying, and deleting files:

- **Parser** (`parser.rs`) — parses the patch DSL into structured operations (Add, Update, Delete, Move).
- **Apply** (`apply.rs`) — applies parsed patches to the filesystem.

### `gateway`

Webhook gateway for messaging channel integrations. Handles inbound webhooks, sender pairing/access control, session routing, and outbound response delivery.

**Native integrations** (full Rust implementations):

| Integration | Directory | Transport | Credentials |
|------------|-----------|-----------|-------------|
| Telegram | `telegram/` | Webhook + polling | `TELEGRAM_BOT_TOKEN` |
| Slack | `slack/` | Webhook | `SLACK_BOT_TOKEN`, `SLACK_SIGNING_SECRET` |
| Discord | `discord/` | Webhook | `DISCORD_BOT_TOKEN`, `DISCORD_PUBLIC_KEY` |
| Teams | `teams/` | Webhook | `TEAMS_APP_ID`, `TEAMS_APP_SECRET` |
| Google Chat | `google_chat/` | Webhook | `GOOGLE_CHAT_WEBHOOK_TOKEN` |
| Signal | `signal/` | SSE (daemon) | `SIGNAL_ACCOUNT` |
| Twilio | `twilio/` | Webhook | `TWILIO_ACCOUNT_SID`, `TWILIO_AUTH_TOKEN` |
| iMessage | `imessage/` | DB polling (macOS) | None |

Key files: `server.rs` (Axum HTTP server, webhook routes), `handler.rs` (message processing pipeline, pairing, agent invocation), `manifest.rs` (channel.toml parsing), `registry.rs` (channel discovery), `executor.rs` (script-based channel subprocess execution)

Features: per-channel agent routing via bindings, rate limiting, auto-reply/away mode, link understanding, group chat activation modes (mention vs always), sender pairing, TTS voice replies, message chunking.

### `plugins`

Plugin marketplace for one-click installation of channel and tool integrations:

- **Catalog** (`catalog.rs`) — plugin registry with all messaging, email, and productivity integrations. Native plugins require only credentials; non-native plugins use embedded template files.
- **Installer** (`installer.rs`) — installs templates to `~/.borg/channels/`.
- **Verifier** (`verifier.rs`) — file integrity checking for installed plugins.
- **Keychain** (`keychain.rs`) — platform-specific credential storage helpers.

## Data flow

```
User input (TUI/REPL)                External Service (webhook)
    │                                         │
    ▼                                         ▼
┌─────────┐     ┌──────────┐     ┌───────────────┐     ┌──────────┐
│REPL/TUI │────▶│  Agent   │────▶│  LLM (SSE)    │     │ Gateway  │
│  (cli)  │◀────│  (core)  │◀────│  Multi-provider│◀───│ (server) │
└─────────┘     └──────────┘     └───────────────┘     └──────────┘
                     │                                       │
                     │ tool calls                   verify → parse
                     ▼                              → agent → respond
              ┌──────────────┐
              │  Tool        │
              │  Dispatch    │
              └──────┬───────┘
                     │
         ┌───────┬───┼───────┬──────────┬──────────┐
         ▼       ▼   ▼       ▼          ▼          ▼
    Built-in   User  Shell  Web       Tasks     Browser
    (memory,   Tools (run_  (fetch,   (cron,    (CDP,
     patch,    (+sandbox)   search)   interval)  Chrome)
     skills)    shell)
```

## System prompt assembly

Each turn, the agent builds a system prompt by concatenating:

1. **IDENTITY.md** — personality and behavioral instructions (per-binding identity supported)
2. **Security policy** — prompt injection defense, role boundary enforcement
3. **Current time** — `YYYY-MM-DD HH:MM:SS TZ`
4. **Memory context** — `MEMORY.md` + `memory/*.md` (ranked by semantic similarity + recency, within token budget)
5. **Skills context** — available skills formatted for the LLM (progressive loading within token budget)
6. **SETUP.md** — first-conversation instructions (injected once, then deleted)

Token budgets are estimated via tiktoken-rs (cl100k_base BPE tokenizer).

## Agent loop

```
send_message(user_input)
    │
    ▼
build system prompt
build tool definitions (built-in + conditional tools, filtered by tool policy)
    │
    ▼
stream LLM response ──────────────────┐
    │                                  │
    ▼                                  │
collect text + tool call deltas        │
strip <internal>...</internal> tags    │
    │                                  │
    ├── text only? ──▶ done            │
    │                                  │
    └── has tool calls?                │
         │                             │
         ▼                             │
    check rate limits                  │
    check execution policy             │
    execute each tool call             │
    redact secrets from output         │
    truncate large outputs             │
    persist message to SQLite          │
    append results to history          │
         │                             │
         └─────────────────────────────┘
              (loop back to LLM)
```

## Dependency graph

```
cli ──▶ core
cli ──▶ heartbeat
cli ──▶ gateway
cli ──▶ plugins

core ──▶ tools
core ──▶ apply-patch

gateway ──▶ core

tools ──▶ sandbox
```
