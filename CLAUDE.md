# Tamagotchi

AI personal assistant agent built in Rust. The agent itself is the plugin system — it writes its own tools at runtime rather than relying on a static extension framework.

## Architecture

Cargo workspace with 6 crates:

```
crates/
  cli/          Binary: REPL, clap args, heartbeat display, onboarding TUI
  core/         Library: agent loop, multi-provider LLM client, memory, soul, config
  heartbeat/    Library: proactive scheduler with quiet hours + dedup
  tools/        Library: tool manifest parsing, registry, subprocess executor
  sandbox/      Library: macOS Seatbelt + Linux Bubblewrap policies
  apply-patch/  Library: patch DSL parser + filesystem applicator
```

**Data directory:** `~/.tamagotchi/` — config, personality, memory, user-created tools, logs.

## Build & Test

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy -- -D warnings
```

Binary name is `tamagotchi`. Requires one of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY` at runtime (see `.env.example`).

## CLI Commands

- `tamagotchi` or `tamagotchi chat` — interactive REPL
- `tamagotchi ask "message"` — one-shot query
- `tamagotchi init` — interactive onboarding wizard (name, personality, provider, model selection)

## Agent Loop

`core/agent.rs` — streams LLM response, parses tool calls, executes them, appends results, loops until text-only response.

System prompt assembled each turn: `SOUL.md` + current time + memory context + skills context (all token-budgeted).

## Built-in Tools

| Tool | Purpose |
|------|---------|
| `write_memory` | Write/append to memory files (SOUL.md, MEMORY.md, or topic files) |
| `read_memory` | Read a memory file |
| `list_tools` | List user-created tools |
| `apply_patch` | Create/modify files in `~/.tamagotchi/tools/` via patch DSL |
| `run_shell` | Execute a shell command |
| `list_skills` | List all skills with status and source |
| `apply_skill_patch` | Create/modify files in `~/.tamagotchi/skills/` via patch DSL |

## User Tools

Located at `~/.tamagotchi/tools/<name>/tool.toml` + entrypoint script. The agent creates these via `apply_patch`. Registry auto-reloads after patching.

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

`~/.tamagotchi/config.toml`:

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
```

## Memory System

- `~/.tamagotchi/MEMORY.md` — loaded every turn
- `~/.tamagotchi/memory/*.md` — loaded by recency until token budget exhausted
- Token estimation via `tiktoken-rs` (cl100k_base BPE tokenizer)

## Personality (SOUL.md)

`~/.tamagotchi/SOUL.md` is injected into the system prompt. The agent can update it via `write_memory` targeting `SOUL.md`. Changes persist across sessions.

During `tamagotchi init`, the onboarding wizard generates a personalized SOUL.md based on the user's chosen agent name and personality style (Professional, Casual, Snarky, Nurturing, or Minimal).

## Skills

Skills are instruction bundles (SKILL.md files with YAML frontmatter) that teach the agent how to use external CLI tools via `run_shell`. Distinct from "tools" which are sandboxed executable scripts.

- **Built-in skills**: Embedded via `include_str!` in `crates/core/skills/*/SKILL.md` (slack, discord, github, weather, skill-creator)
- **User skills**: `~/.tamagotchi/skills/<name>/SKILL.md` — created via `apply_skill_patch`
- User skills with the same name override built-in skills
- Requirements (bins/env vars) are checked at load time; unavailable skills are still listed but flagged

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

## Heartbeat

Separate tokio task. Fires at configured interval, skips during quiet hours, suppresses duplicate/empty responses. Renders in cyan in the REPL.

## Sandboxing

User tools run sandboxed:
- **macOS**: `sandbox-exec` with generated Seatbelt profile (deny-all default, explicit allows)
- **Linux**: `bwrap` with namespace isolation (read-only mounts, network unshare)

Policy derived from each tool's `[sandbox]` section in `tool.toml`.

## Key Source Files

| File | What |
|------|------|
| `crates/cli/src/main.rs` | Entry point, clap commands, init |
| `crates/cli/src/onboarding.rs` | TUI onboarding wizard (inquire-based) |
| `crates/cli/src/repl.rs` | Interactive loop + heartbeat rendering |
| `crates/core/src/agent.rs` | Conversation loop + tool dispatch |
| `crates/core/src/provider.rs` | Provider enum, auto-detection, headers |
| `crates/core/src/llm.rs` | Multi-provider streaming SSE client |
| `crates/core/src/config.rs` | Config parsing with defaults |
| `crates/core/src/soul.rs` | SOUL.md load/save |
| `crates/core/src/memory.rs` | Memory loading with token budget |
| `crates/core/src/skills.rs` | Skills loading, parsing, token budgeting |
| `crates/core/src/types.rs` | Message, ToolCall, ToolDefinition |
| `crates/heartbeat/src/scheduler.rs` | Interval + quiet hours + dedup |
| `crates/tools/src/manifest.rs` | tool.toml parsing |
| `crates/tools/src/registry.rs` | Scan + register user tools |
| `crates/tools/src/executor.rs` | Runtime resolution + subprocess |
| `crates/sandbox/src/policy.rs` | SandboxPolicy + command wrapping |
| `crates/sandbox/src/seatbelt.rs` | macOS profile generation |
| `crates/sandbox/src/bubblewrap.rs` | Linux bwrap arg building |
| `crates/apply-patch/src/parser.rs` | Patch DSL parser |
| `crates/apply-patch/src/apply.rs` | Filesystem patch applicator |

## Testing

```sh
cargo test                          # all 20 tests
cargo test -p tamagotchi-apply-patch  # 13 patch tests
cargo test -p tamagotchi-core         # config + skills tests
```
