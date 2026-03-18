# Borg

AI personal assistant agent built in Rust. The agent itself is the plugin system — it writes its own tools at runtime rather than relying on a static extension framework.

## Architecture

Cargo workspace with 8 crates:

```
crates/
  cli/              Binary: REPL, clap args, heartbeat display, onboarding TUI
  core/             Library: agent loop, multi-provider LLM client, memory, identity, config
  heartbeat/        Library: proactive scheduler with quiet hours + dedup
  tools/            Library: tool manifest parsing, registry, subprocess executor
  sandbox/          Library: macOS Seatbelt + Linux Bubblewrap policies
  apply-patch/      Library: patch DSL parser + filesystem applicator
  gateway/          Library: webhook gateway for messaging channel integrations
  plugins/          Library: marketplace catalog, plugin installer, TUI integration
```

**Data directory:** `~/.borg/` — config, personality, memory, user-created tools, logs.

## Build & Test

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Binary name is `borg`. Requires one of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY` at runtime (see `.env.example`).

All integrations are compiled unconditionally into a single binary. iMessage is macOS-only via `#[cfg(target_os = "macos")]`.

## CLI Commands

- `borg` or `borg chat` — interactive REPL
- `borg ask "message"` — one-shot query
- `borg init` — interactive onboarding wizard (name, personality, provider, model selection)
- `borg add <name>` — set up an integration's credentials (e.g. `borg add telegram`)
- `borg remove <name>` — remove an integration's credentials
- `borg plugins` — list all integrations with configured/unconfigured status
- `borg gateway` — start webhook gateway server for messaging channels
- `borg doctor` — run diagnostics (config, provider, sandbox, tools, skills, memory, gateway, budget, host security)
- `/plugins` (TUI command) — open marketplace popup to install/uninstall messaging, email, and productivity integrations

## Plugins

Plugin marketplace for one-click installation of channel and tool integrations. Plugin files are embedded in the binary via `include_str!` and installed to `~/.borg/channels/` or `~/.borg/tools/`. Categories: Messaging (WhatsApp, iMessage, SMS), Email (Gmail, Outlook), Productivity (Google Calendar, Notion, Linear). **Note:** Telegram and Slack are native Rust integrations in the gateway crate (not plugins).

## Agent Loop

`core/agent.rs` — streams LLM response, parses tool calls, executes them, appends results, loops until text-only response.

- **Internal tag stripping**: `<internal>...</internal>` blocks are stripped from output in real-time during streaming (prevents chain-of-thought leakage)
- **Message persistence**: Each message is written to SQLite (`messages` table) immediately when added to history, enabling crash recovery
- **Message timestamps**: All messages carry RFC3339 timestamps for temporal reasoning and compaction summaries

System prompt assembled each turn: `IDENTITY.md` + current time + memory context + skills context (all token-budgeted).

## Built-in Tools

| Tool | Purpose |
|------|---------|
| `write_memory` | Write/append to memory files (IDENTITY.md, MEMORY.md, or topic files). Supports `scope` param (`global`/`local`) |
| `read_memory` | Read a memory file |
| `list_tools` | List user-created tools |
| `apply_patch` | Create/update/delete files in the current working directory via patch DSL |
| `create_tool` | Create/modify files in `~/.borg/tools/` via patch DSL |
| `run_shell` | Execute a shell command |
| `list_skills` | List all skills with status and source |
| `apply_skill_patch` | Create/modify files in `~/.borg/skills/` via patch DSL |
| `read_pdf` | Extract text from a PDF file with token-aware truncation |
| `create_channel` | Create/modify channel integrations in `~/.borg/channels/` via patch DSL |
| `list_channels` | List all messaging channels with status and webhook paths |
| `security_audit` | Run host security audit (firewall, ports, SSH, permissions, encryption, updates, services). Requires `security.host_audit = true` |

## User Tools

Located at `~/.borg/tools/<name>/tool.toml` + entrypoint script. The agent creates these via `apply_patch`. Registry auto-reloads after patching.

**tool.toml format:**
```toml
name = "example"
description = "What it does"
runtime = "python"        # python | node | deno | bash
entrypoint = "main.py"
timeout_ms = 30000

[sandbox]
network = false
fs_read = []
fs_write = []

[parameters]
type = "object"
[parameters.properties.arg_name]
type = "string"
description = "Argument description"
[parameters.required]
values = ["arg_name"]
```

Tool receives JSON args on stdin, returns result on stdout.

## Patch DSL

Used by `apply_patch` to create/modify/delete files. Follows the codex apply-patch format where **every content line must have a prefix** (`+` for added content, ` ` for context, `-` for removed lines). This prevents ambiguity when file content contains `***` markers.

```
*** Begin Patch
*** Add File: tool-name/tool.toml
+name = "example"
+description = "What it does"
*** Add File: tool-name/main.py
+import sys
+print("hello")
*** Update File: tool-name/main.py
@@
 context
-old line
+new line
*** Delete File: tool-name/old.py
*** End Patch
```

## Config

`~/.borg/config.toml`:

```toml
[llm]
provider = "openrouter"             # openrouter | openai | anthropic | gemini (auto-detected if omitted)
api_key_env = "OPENROUTER_API_KEY"
model = "anthropic/claude-sonnet-4"
temperature = 0.7
max_tokens = 4096

[heartbeat]
enabled = false
interval = "30m"
cron = "0 */30 * * * *"          # optional, overrides interval
quiet_hours_start = "23:00"
quiet_hours_end = "07:00"

[tools]
default_timeout_ms = 30000

[sandbox]
enabled = true
mode = "strict"

[memory]
max_context_tokens = 8000

[skills]
enabled = true
max_context_tokens = 4000

[security]
secret_detection = true
blocked_paths = [".ssh", ".aws", ".gnupg", ".config/gh", ".env", "credentials", "private_key"]
host_audit = true                # enable host security checks in doctor + security_audit tool
hitl_dangerous_ops = true          # confirm before file deletion, IDENTITY.md changes
action_limits.tool_calls_warn = 50
action_limits.tool_calls_block = 100
action_limits.shell_commands_warn = 20
action_limits.shell_commands_block = 50
action_limits.file_writes_warn = 15
action_limits.file_writes_block = 30
action_limits.memory_writes_warn = 10
action_limits.memory_writes_block = 20
action_limits.web_requests_warn = 20
action_limits.web_requests_block = 50

[budget]
monthly_token_limit = 1000000    # 0 = unlimited
warning_threshold = 0.8          # warn at 80% usage

[credentials]
JIRA_API_TOKEN = "JIRA_API_TOKEN"                    # legacy: bare string = env var name
SLACK_BOT_TOKEN = { source = "exec", command = "security", args = ["find-generic-password", "-s", "slack", "-w"] }
GITHUB_TOKEN = { source = "file", path = "~/.config/gh/token" }
MY_SECRET = { source = "env", var = "MY_SECRET" }    # explicit env SecretRef
```

## Memory System

- `~/.borg/MEMORY.md` — loaded every turn
- `~/.borg/memory/*.md` — loaded by recency until token budget exhausted
- **Per-project local memory**: `$CWD/.borg/memory/*.md` — loaded in addition to global memory when present
- `write_memory` tool accepts `scope: "local"` to write to project-local memory
- Token estimation via `tiktoken-rs` (cl100k_base BPE tokenizer)

## Identity (IDENTITY.md)

`~/.borg/IDENTITY.md` is injected into the system prompt. The agent can update it via `write_memory` targeting `IDENTITY.md`. Changes persist across sessions.

During `borg init`, the onboarding wizard generates a personalized IDENTITY.md based on the user's chosen agent name and personality style (Professional, Casual, Snarky, Nurturing, or Minimal).

## Skills

Skills are instruction bundles (SKILL.md files with YAML frontmatter) that teach the agent how to use external CLI tools via `run_shell`. Distinct from "tools" which are sandboxed executable scripts. Built-in skills are embedded via `include_str!` and always compiled in. During `borg init`, bundled skills are extracted to `~/.borg/skills/`.

- **Built-in skills**: Embedded via `include_str!` in `crates/core/skills/*/SKILL.md` (slack, discord, github, weather, skill-creator, git, http, search, docker, database, notes, calendar, 1password, browser)
- **User skills**: `~/.borg/skills/<name>/SKILL.md` — created via `apply_skill_patch`
- User skills with the same name override built-in skills
- Requirements (bins/env vars) are checked at load time against both process env and `[credentials]` store; unavailable skills are still listed but flagged
- **Credential injection**: Resolved credentials from `[credentials]` are injected as env vars into `run_shell` commands, so skills can reference them without the user exporting them in the shell
- **Progressive loading**: Metadata (name + description) always loaded for all skills (~50 tokens each); full SKILL.md body loaded only for available skills within token budget
- **References**: User skills can have `references/*.md` files stored on the Skill struct (not auto-loaded into context)
- **Scripts**: User skills can have `scripts/` directories; paths stored on the Skill struct for use with `run_shell`

**SKILL.md format:**
```markdown
---
name: my-skill
description: "What it does and when to use it."
requires:
  bins: ["curl"]
  env: ["API_TOKEN"]
---

# Skill Title

Instructions and command examples here.
```

## Messaging Channels (Gateway)

Channels are user-created integrations that receive webhooks from external services, route messages to the agent, and send responses back. They follow the same pattern as tools.

**Native integrations:** Telegram and Slack are handled natively in Rust (`crates/gateway/src/telegram/`, `crates/gateway/src/slack/`) using `reqwest` — no Python scripts needed. Set `TELEGRAM_BOT_TOKEN` or `SLACK_BOT_TOKEN`/`SLACK_SIGNING_SECRET` env vars (or configure via `[credentials]` store). Telegram optionally uses `gateway.public_url` for automatic webhook registration.

**Location:** `~/.borg/channels/<name>/`

**channel.toml format:**
```toml
name = "my-slack"
description = "Slack workspace integration"
runtime = "python"              # python | node | deno | bash

[scripts]
inbound = "parse_inbound.py"   # Receives {headers, body} JSON -> normalized message
outbound = "send_outbound.py"  # Receives {text, sender_id, channel_id, token} JSON -> sends
verify = "verify.py"           # Optional: webhook signature verification

[sandbox]
network = true
fs_read = ["/etc/ssl"]
fs_write = []

[auth]
secret_env = "SLACK_SIGNING_SECRET"  # For webhook verification
token_env = "SLACK_BOT_TOKEN"        # Passed to outbound script

[settings]
webhook_path = "/webhook/my-slack"   # Default: /webhook/<name>
timeout_ms = 15000
max_concurrent = 5
```

**Message flow:** External Service -> POST /webhook/<name> -> verify -> parse inbound -> agent -> outbound script

**Config:**
```toml
[gateway]
host = "127.0.0.1"
port = 7842
max_concurrent = 10
request_timeout_ms = 120000
```

**CLI:** `borg gateway` starts the gateway server standalone. The gateway also runs automatically as part of the daemon.

**Built-in tools:** `create_channel` (patch DSL to `~/.borg/channels/`), `list_channels`

**Database:** V3 migration adds `channel_sessions` and `channel_messages` tables.

## Lifecycle Hooks

`crates/core/src/hooks.rs` — extensible hook system for intercepting agent loop events.

**Hook points**: `BeforeAgentStart`, `BeforeLlmCall`, `AfterLlmResponse`, `BeforeToolCall`, `AfterToolCall`, `TurnComplete`, `OnError`

**Hook actions**: `Continue` (no-op), `InjectContext(String)` (append to system prompt), `Skip` (skip current action, e.g. tool call)

Hooks implement the `Hook` trait and are registered on the agent via `agent.hook_registry_mut().register(...)`. Multiple hooks can be registered; `InjectContext` results are merged, `Skip` short-circuits.

## Doctor Command

`crates/core/src/doctor.rs` — diagnostic checks for config, provider, sandbox, tools, skills, memory, data directory, budget, and host security.

Available via `borg doctor` CLI subcommand or `/doctor` TUI command.

## Database

SQLite at `~/.borg/borg.db` with versioned migrations:
- **V1**: sessions, scheduled_tasks, task_runs, meta, token_usage tables
- **V2**: messages table (crash recovery), retry_count on scheduled_tasks
- **V3**: channel_sessions + channel_messages tables (gateway)
- Schema version tracked in `meta` table; migrations run automatically on `Database::open()`

## Signal Handling & Graceful Shutdown

- Global `CancellationToken` wired to SIGINT (Ctrl+C) and SIGTERM (Unix)
- Daemon drains in-progress tasks via semaphore before exiting
- Agent loop respects cancellation between iterations

## Daemon & Concurrent Tasks

- Daemon uses `tokio::sync::Semaphore` (capacity from `tasks.max_concurrent`, default 3)
- Each task spawned as independent tokio task with 5-minute timeout
- Failed tasks are recorded with error details in `task_runs` table

## Heartbeat

Separate tokio task. Fires at configured interval, skips during quiet hours, suppresses duplicate/empty responses. Renders in cyan in the REPL.

## Sandboxing

User tools run sandboxed:
- **macOS**: `sandbox-exec` with generated Seatbelt profile (deny-all default, explicit allows)
- **Linux**: `bwrap` with namespace isolation (read-only mounts, network unshare)
- **Filesystem blocklist**: Paths in `[security] blocked_paths` (defaults: `.ssh`, `.aws`, `.gnupg`, etc.) are filtered from tool `fs_read`/`fs_write` before sandbox profile generation

Policy derived from each tool's `[sandbox]` section in `tool.toml`.

## Prompt Injection Defense

Six-layer defense against prompt injection attacks:

1. **Input Sanitization** (`crates/core/src/sanitize.rs`): Scoring-based injection detection with regex patterns. Flags suspicious content with explicit untrusted markers instead of stripping (preserves legitimate messages). Applied at gateway inbound and tool results.

2. **Context Segregation**: System prompt uses XML structural boundaries (`<system_instructions>`, `<user_memory>`, `<tool_output>`) with trust labels to prevent instruction boundary confusion.

3. **Prompt Hardening**: Security policy injected into system prompt instructing the model to treat external data as data, not instructions. Role boundary enforcement.

4. **HITL for Dangerous Ops**: `ToolConfirmation` event for file deletion (apply_patch) and identity modification (IDENTITY.md). Auto-denied in gateway mode. Configurable via `security.hitl_dangerous_ops`.

5. **Rate Limiting** (`crates/core/src/rate_guard.rs`): Per-session action caps (tool calls, shell commands, file/memory writes, web requests) with warn and block thresholds. Configurable via `[security]` config.

6. **Secret Redaction** (existing): Regex-based redaction of API keys, tokens, and credentials in tool outputs.

## Key Source Files

| File | What |
|------|------|
| `crates/cli/src/main.rs` | Entry point, clap commands, init |
| `crates/cli/src/onboarding.rs` | TUI onboarding wizard (inquire-based) |
| `crates/cli/src/plugins.rs` | Integration catalog, `borg add/remove/plugins` commands |
| `crates/cli/src/repl.rs` | Interactive loop + heartbeat rendering |
| `crates/core/src/agent.rs` | Conversation loop + tool dispatch |
| `crates/core/src/provider.rs` | Provider enum, auto-detection, headers |
| `crates/core/src/llm.rs` | Multi-provider streaming SSE client |
| `crates/core/src/config.rs` | Config parsing with defaults |
| `crates/core/src/identity.rs` | IDENTITY.md load/save |
| `crates/core/src/memory.rs` | Memory loading with token budget |
| `crates/core/src/skills.rs` | Skills loading, parsing, progressive token budgeting |
| `crates/core/src/hooks.rs` | Lifecycle hook system (trait, registry, dispatch) |
| `crates/core/src/doctor.rs` | Diagnostic checks and report formatting |
| `crates/core/src/host_audit.rs` | Host security audit checks (firewall, ports, SSH, permissions, encryption, updates, services) |
| `crates/core/src/sanitize.rs` | Prompt injection detection (scoring-based, regex patterns, untrusted content wrapping) |
| `crates/core/src/rate_guard.rs` | Per-session rate limiting for tool calls, shell commands, file/memory writes, web requests |
| `crates/core/src/db.rs` | SQLite database with versioned migrations |
| `crates/core/src/types.rs` | Message (with timestamps), ToolCall, ToolDefinition |
| `crates/heartbeat/src/scheduler.rs` | Interval + quiet hours + dedup |
| `crates/tools/src/manifest.rs` | tool.toml parsing |
| `crates/tools/src/registry.rs` | Scan + register user tools |
| `crates/tools/src/executor.rs` | Runtime resolution + subprocess |
| `crates/sandbox/src/policy.rs` | SandboxPolicy + command wrapping |
| `crates/sandbox/src/seatbelt.rs` | macOS profile generation |
| `crates/sandbox/src/bubblewrap.rs` | Linux bwrap arg building |
| `crates/apply-patch/src/parser.rs` | Patch DSL parser |
| `crates/apply-patch/src/apply.rs` | Filesystem patch applicator |
| `crates/gateway/src/manifest.rs` | channel.toml parsing |
| `crates/gateway/src/registry.rs` | Scan + register user channels |
| `crates/gateway/src/executor.rs` | Channel script subprocess execution |
| `crates/gateway/src/server.rs` | Axum HTTP server (webhook routes, native Telegram + Slack) |
| `crates/gateway/src/slack/` | Native Slack Bot API integration (types, verify, parse, api) |
| `crates/gateway/src/telegram/` | Native Telegram Bot API integration (types, verify, parse, api, dedup) |
| `crates/gateway/src/twilio/` | Native Twilio integration (WhatsApp + SMS) |
| `crates/gateway/src/handler.rs` | Webhook handler: verify -> parse -> agent -> respond |
| `crates/core/src/integrations/` | Native tool integrations (Gmail, Outlook, Calendar, Notion, Linear) |

## Testing

```sh
cargo test                                               # all tests
cargo test -p borg-apply-patch                           # 13 patch tests
cargo test -p borg-core                                  # config + skills tests
cargo test -p borg-gateway                               # channel manifest + registry tests
cargo test -p borg-plugins                               # catalog + installer tests
```
