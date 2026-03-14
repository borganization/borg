# Architecture

Tamagotchi is a Cargo workspace with six crates. Each crate has a focused responsibility.

## Crate overview

```
crates/
├── cli/          # Binary: REPL, clap args, heartbeat display
├── core/         # Library: agent loop, LLM client, memory, soul, config
├── heartbeat/    # Library: proactive scheduler with quiet hours + dedup
├── tools/        # Library: tool manifest parsing, registry, subprocess executor
├── sandbox/      # Library: macOS Seatbelt + Linux Bubblewrap policies
└── apply-patch/  # Library: patch DSL parser + filesystem applicator
```

### `cli`

Entry point. Defines three commands via clap: `chat` (default), `ask`, and `init`. The REPL lives here and handles user input, streaming output display, and heartbeat message rendering.

Key files: `main.rs`, `repl.rs`

### `core`

The heart of the project. Contains:

- **Agent** (`agent.rs`) — the conversation loop. Builds the system prompt, streams LLM responses, dispatches tool calls, and loops until the LLM returns a text-only response.
- **LLM client** (`llm.rs`) — OpenRouter SSE streaming client. Handles chunked responses, tool call deltas, and error events.
- **Config** (`config.rs`) — TOML configuration with serde defaults for every field.
- **Memory** (`memory.rs`) — loads `MEMORY.md` and `memory/*.md` files into the system prompt, respecting a token budget.
- **Soul** (`soul.rs`) — loads and saves `SOUL.md`, the agent's personality prompt.
- **Skills** (`skills.rs`) — loads built-in and user skills, checks requirements, and formats them for the system prompt.
- **Types** (`types.rs`) — shared types: `Message`, `ToolCall`, `ToolDefinition`, `Role`.

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
┌─────────┐     ┌──────────┐     ┌───────────┐
│  REPL   │────▶│  Agent   │────▶│ LLM (SSE) │
│ (cli)   │◀────│  (core)  │◀────│ OpenRouter │
└─────────┘     └──────────┘     └───────────┘
                     │
                     │ tool calls
                     ▼
              ┌──────────────┐
              │  Tool        │
              │  Dispatch    │
              └──────┬───────┘
                     │
         ┌───────────┼───────────┐
         ▼           ▼           ▼
    Built-in     User Tools   Shell
    (memory,     (registry    (run_shell)
     patch,       + sandbox
     skills)      + executor)
```

## System prompt assembly

Each turn, the agent builds a system prompt by concatenating:

1. **SOUL.md** — personality and behavioral instructions
2. **Current time** — `YYYY-MM-DD HH:MM:SS TZ`
3. **Memory context** — `MEMORY.md` + `memory/*.md` (sorted by recency, within token budget)
4. **Skills context** — available skills formatted for the LLM (within token budget)

Token budgets are estimated at ~4 characters per token.

## Agent loop

```
send_message(user_input)
    │
    ▼
build system prompt
build tool definitions (built-in + user tools)
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
    execute each tool call             │
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
