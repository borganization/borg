# Getting Started

Borg is an AI personal assistant agent built in Rust. The agent writes its own tools at runtime rather than relying on a static plugin framework.

## Prerequisites

- **Rust toolchain**: Install via [rustup](https://rustup.rs/)
- **LLM API key**: One of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY`
- **Linux**: `bwrap` (bubblewrap) for tool sandboxing — install via your package manager
- **macOS**: `sandbox-exec` is included with the OS

## Installation

Clone the repository and build from source:

```sh
git clone https://github.com/borganization/borg.git
cd borg
cargo build --release
```

The binary is located at `target/release/borg`.

## Setup

1. Set your API key (any one of the supported providers):

```sh
export OPENROUTER_API_KEY="sk-or-..."
# or
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export OPENAI_API_KEY="sk-..."
# or
export GEMINI_API_KEY="..."
```

Or add it to your shell profile / `.env` file. See `.env.example` for the template. The provider is auto-detected based on which key is set.

2. Initialize with the onboarding wizard:

```sh
borg init
```

This launches an interactive TUI that walks you through setup:

- **Name your agent** — give it a custom identity (appears in IDENTITY.md)
- **Pick a personality style** — Professional, Casual, Snarky, Nurturing, or Minimal
- **Choose a provider and model** — select from OpenRouter, OpenAI, Anthropic, or Gemini models

The wizard writes your choices to `config.toml` and generates a personalized `IDENTITY.md`. If you cancel mid-wizard, defaults are used instead.

This creates `~/.borg/` with your customized config, personality, and memory files:

```
~/.borg/
├── config.toml       # Configuration
├── IDENTITY.md       # Personality prompt
├── MEMORY.md         # Memory index
├── memory/           # Topic-specific memories
├── tools/            # User-created tools
├── skills/           # User-created skills
├── sessions/         # Session persistence (JSON files)
├── logs/             # Daily JSONL debug logs
├── cache/
└── borg.db     # SQLite database (sessions, scheduled tasks)
```

## Usage

### Interactive REPL

```sh
borg
# or
borg chat
```

Start a conversation. The agent streams responses, can call tools, and remembers context across turns within a session. Use slash commands like `/compact`, `/undo`, `/memory cleanup`, and `/session list` for session management.

### One-shot query

```sh
borg ask "What's the weather in Tokyo?"
```

Send a single message, get a response, and exit.

Flags:
- `--yes` — auto-approve all tool executions (skip confirmation prompts)
- `--json` — output response as JSON

### Daemon mode

```sh
borg daemon
```

Run the agent in the background for scheduled tasks and heartbeat check-ins without the interactive TUI.

### System service

```sh
borg service install    # install as a system service (launchd/systemd)
borg service uninstall  # remove the service
borg service status     # check service status
```

## What happens when you chat

1. Your message is added to the conversation history
2. A system prompt is assembled from: `IDENTITY.md` + current time + memory context + skills context
3. The message is streamed to the LLM via the configured provider
4. If the LLM responds with tool calls, each tool is executed and results are fed back
5. The loop continues until the LLM responds with text only
6. The session is auto-saved for later resumption

## Next steps

- [Configuration](configuration.md) — customize the provider, model, timeouts, and all settings
- [Tools](tools.md) — learn how the agent creates and uses tools
- [Skills](skills.md) — instruction bundles for teaching the agent CLI workflows
- [Memory](memory.md) — how the agent remembers things across sessions
- [Architecture](architecture.md) — how the codebase is structured
