# Architecture

Tamagotchi is a Cargo workspace with six crates. Each crate has a focused responsibility.

## Crate overview

```
crates/
├── cli/          # Binary: REPL, TUI, clap args, heartbeat display, daemon/service
├── core/         # Library: agent loop, multi-provider LLM client, memory, soul, config
├── heartbeat/    # Library: proactive scheduler with quiet hours + dedup
├── tools/        # Library: tool manifest parsing, registry, subprocess executor
├── sandbox/      # Library: macOS Seatbelt + Linux Bubblewrap policies
└── apply-patch/  # Library: patch DSL parser + filesystem applicator
```

### `cli`

Entry point. Defines commands via clap: `chat` (default), `ask`, `init`, `daemon`, and `service` (install/uninstall/status). Includes both a line-based REPL and a full ratatui-based TUI with markdown rendering, slash command autocomplete, and session management. Also handles daemon mode for background task execution and system service installation (launchd on macOS, systemd on Linux).

Key files: `main.rs`, `repl.rs`, `service.rs`, `tui/app.rs`, `tui/command_popup.rs`

### `core`

The heart of the project. Contains:

- **Agent** (`agent.rs`) — the conversation loop. Builds the system prompt, streams LLM responses, dispatches tool calls, and loops until the LLM returns a text-only response.
- **LLM client** (`llm.rs`) — multi-provider streaming SSE client. Supports OpenRouter, OpenAI, Anthropic, and Gemini. Handles chunked responses, tool call deltas, and error events.
- **Provider** (`provider.rs`) — provider enum, auto-detection from available API keys, and request/header configuration.
- **Config** (`config.rs`) — TOML configuration with serde defaults for every field. 13 config sections.
- **Memory** (`memory.rs`) — loads `MEMORY.md` and `memory/*.md` files into the system prompt, respecting a token budget.
- **Soul** (`soul.rs`) — loads and saves `SOUL.md`, the agent's personality prompt.
- **Skills** (`skills.rs`) — loads built-in and user skills, checks requirements, and formats them for the system prompt.
- **Types** (`types.rs`) — shared types: `Message`, `ToolCall`, `ToolDefinition`, `Role`.
- **Session** (`session.rs`) — session persistence with JSON serialization, auto-save, and auto-titling.
- **Database** (`db.rs`) — SQLite database for sessions, scheduled tasks, and task run history.
- **Conversation** (`conversation.rs`) — history compaction, token estimation, and conversation normalization.
- **Policy** (`policy.rs`) — execution policy with auto-approve/deny glob patterns for shell commands.
- **Secrets** (`secrets.rs`) — secret detection and redaction (AWS keys, GitHub tokens, JWTs, private keys, etc.).
- **Web** (`web.rs`) — web fetching (HTML-to-text) and searching (DuckDuckGo/Brave).
- **Tasks** (`tasks.rs`) — scheduled task definitions (cron, interval, once) and next-run calculation.
- **Logging** (`logging.rs`) — daily JSONL logging of messages and tool calls.
- **Retry** (`retry.rs`) — retry logic with exponential backoff for LLM requests.
- **Tokenizer** (`tokenizer.rs`) — token estimation via tiktoken-rs (cl100k_base BPE tokenizer).
- **Truncate** (`truncate.rs`) — tool output truncation (head+tail strategy).

### `heartbeat`

A proactive scheduler that runs as a separate tokio task. Fires at a configured interval, skips during quiet hours, and suppresses duplicate or empty LLM responses. Messages render in cyan in the REPL.

Key file: `scheduler.rs`

### `tools`

Manages user-created tools:

- **Manifest** (`manifest.rs`) — parses `tool.toml` files and converts parameter definitions to JSON Schema.
- **Registry** (`registry.rs`) — scans `~/.tamagotchi/tools/`, loads manifests, and provides tool definitions to the agent.
- **Executor** (`executor.rs`) — resolves the runtime binary (python3, node, deno, bash), wraps the command with sandbox policy, and runs the subprocess with JSON args on stdin.

### `sandbox`

Platform-specific sandboxing for user tools:

- **Policy** (`policy.rs`) — `SandboxPolicy` struct and `wrap_command()` which delegates to the platform-specific implementation.
- **Seatbelt** (`seatbelt.rs`) — generates macOS `sandbox-exec` profiles (deny-all default with explicit allows).
- **Bubblewrap** (`bubblewrap.rs`) — builds `bwrap` arguments for Linux namespace isolation.

### `apply-patch`

A custom patch DSL for creating, modifying, and deleting files:

- **Parser** (`parser.rs`) — parses the patch DSL into structured operations (Add, Update, Delete).
- **Apply** (`apply.rs`) — applies parsed patches to the filesystem.

## Data flow

```
User input
    │
    ▼
┌─────────┐     ┌──────────┐     ┌───────────────┐
│REPL/TUI │────▶│  Agent   │────▶│  LLM (SSE)    │
│  (cli)  │◀────│  (core)  │◀────│  Multi-provider│
└─────────┘     └──────────┘     └───────────────┘
                     │
                     │ tool calls
                     ▼
              ┌──────────────┐
              │  Tool        │
              │  Dispatch    │
              └──────┬───────┘
                     │
         ┌───────┬───┼───────┬──────────┐
         ▼       ▼   ▼       ▼          ▼
    Built-in   User  Shell  Web       Scheduled
    (memory,   Tools (run_  (fetch,    Tasks
     patch,    (+sandbox)   search)   (cron,
     skills)    shell)                interval)
```

## System prompt assembly

Each turn, the agent builds a system prompt by concatenating:

1. **SOUL.md** — personality and behavioral instructions
2. **Current time** — `YYYY-MM-DD HH:MM:SS TZ`
3. **Memory context** — `MEMORY.md` + `memory/*.md` (sorted by recency, within token budget)
4. **Skills context** — available skills formatted for the LLM (within token budget)

Token budgets are estimated via tiktoken-rs (cl100k_base BPE tokenizer).

## Agent loop

```
send_message(user_input)
    │
    ▼
build system prompt
build tool definitions (built-in + user tools + conditional web/task tools)
    │
    ▼
stream LLM response ──────────────────┐
    │                                  │
    ▼                                  │
collect text + tool call deltas        │
    │                                  │
    ├── text only? ──▶ done            │
    │                                  │
    └── has tool calls?                │
         │                             │
         ▼                             │
    check execution policy             │
    execute each tool call             │
    redact secrets from output         │
    append results to history          │
         │                             │
         └─────────────────────────────┘
              (loop back to LLM)
```

## Dependency graph

```
cli ──▶ core
cli ──▶ heartbeat

core ──▶ tools
core ──▶ apply-patch

tools ──▶ sandbox
```
