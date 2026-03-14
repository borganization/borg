# Getting Started

Tamagotchi is an AI personal assistant agent built in Rust. The agent writes its own tools at runtime rather than relying on a static plugin framework.

## Prerequisites

- **Rust toolchain**: Install via [rustup](https://rustup.rs/)
- **OpenRouter API key**: Sign up at [openrouter.ai](https://openrouter.ai/) and get an API key
- **Linux**: `bwrap` (bubblewrap) for tool sandboxing — install via your package manager
- **macOS**: `sandbox-exec` is included with the OS

## Installation

Clone the repository and build from source:

```sh
git clone https://github.com/theognis1002/tamagotchi.git
cd tamagotchi
cargo build --release
```

The binary is located at `target/release/tamagotchi`.

## Setup

1. Set your API key:

```sh
export OPENROUTER_API_KEY="sk-or-..."
```

Or add it to your shell profile / `.env` file. See `.env.example` for the template.

2. Initialize the data directory:

```sh
tamagotchi init
```

This creates `~/.tamagotchi/` with default config, personality, and memory files:

```
~/.tamagotchi/
├── config.toml       # Configuration
├── SOUL.md           # Personality prompt
├── MEMORY.md         # Memory index
├── memory/           # Topic-specific memories
├── tools/            # User-created tools
├── skills/           # User-created skills
├── logs/
└── cache/
```

## Usage

### Interactive REPL

```sh
tamagotchi
# or
tamagotchi chat
```

Start a conversation. The agent streams responses, can call tools, and remembers context across turns within a session.

### One-shot query

```sh
tamagotchi ask "What's the weather in Tokyo?"
```

Send a single message, get a response, and exit.

## What happens when you chat

1. Your message is added to the conversation history
2. A system prompt is assembled from: `SOUL.md` + current time + memory context + skills context
3. The message is streamed to the LLM via OpenRouter
4. If the LLM responds with tool calls, each tool is executed and results are fed back
5. The loop continues until the LLM responds with text only

## Next steps

- [Configuration](configuration.md) — customize the model, timeouts, and sandbox settings
- [Tools](tools.md) — learn how the agent creates and uses tools
- [Skills](skills.md) — instruction bundles for teaching the agent CLI workflows
- [Memory](memory.md) — how the agent remembers things across sessions
- [Architecture](architecture.md) — how the codebase is structured
