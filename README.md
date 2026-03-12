# Tamagotchi

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

An AI personal assistant that lives on your machine. It talks to you, remembers things across sessions, checks in proactively, and creates its own tools when it needs new capabilities.

## Features

- **Streaming conversations** via OpenRouter (any model -- Claude, GPT, Llama, etc.)
- **Self-extending tool system** -- the agent writes Python/Node/Bash scripts at runtime and immediately uses them
- **Persistent memory** -- file-based memory system loaded into every conversation
- **Evolving personality** -- SOUL.md defines who the agent is; the agent can update it over time
- **Proactive heartbeat** -- periodic check-ins with quiet hours and duplicate suppression
- **Sandboxed tool execution** -- macOS Seatbelt and Linux Bubblewrap isolation for user-created tools
- **Patch DSL** -- structured file creation/modification system for reliable tool generation

## Prerequisites

- [Rust 1.75+](https://rustup.rs/)
- An [OpenRouter](https://openrouter.ai) API key
- macOS or Linux (sandboxing is platform-specific; other platforms work without sandbox)

## Quick Start

1. Clone and build:
   ```sh
   git clone https://github.com/theognis1002/tamagotchi.git
   cd tamagotchi
   cargo build --release
   ```

2. Initialize the config directory:
   ```sh
   ./target/release/tamagotchi init
   ```

3. Set your API key:
   ```sh
   export OPENROUTER_API_KEY="sk-or-..."
   ```

4. Start chatting:
   ```sh
   ./target/release/tamagotchi
   ```

The `init` command creates `~/.tamagotchi/` with:
- `config.toml` -- LLM settings, heartbeat config, sandbox mode
- `SOUL.md` -- agent personality
- `MEMORY.md` -- memory index
- `tools/` -- where agent-created tools live

## Usage

```sh
tamagotchi              # Interactive REPL (default)
tamagotchi chat         # Same as above
tamagotchi ask "..."    # Send a single message and exit
tamagotchi init         # Scaffold ~/.tamagotchi/
```

### REPL Commands

| Command | Action |
|---------|--------|
| `help` | Show available commands |
| `/tools` | List installed user tools |
| `/memory` | Show loaded memory context |
| `quit` | Exit |

## How It Works

The agent runs a conversation loop: send messages to the LLM, parse streaming responses, execute any tool calls, feed results back, and repeat until the model responds with text only.

### Built-in Tools

| Tool | What it does |
|------|-------------|
| `write_memory` | Save information to memory files |
| `read_memory` | Retrieve saved information |
| `apply_patch` | Create or modify tool scripts |
| `list_tools` | Show available user tools |
| `run_shell` | Execute shell commands |

### Self-Extending Tools

When you ask the agent for a capability it doesn't have (e.g., "check the weather"), it:

1. Uses `apply_patch` to create a new tool in `~/.tamagotchi/tools/`
2. Writes a `tool.toml` manifest and implementation script
3. The tool registry reloads automatically
4. The agent uses the new tool immediately

Tools are plain scripts (Python, Node, Deno, or Bash) that receive JSON on stdin and return results on stdout.

### Memory

- `~/.tamagotchi/MEMORY.md` -- loaded into every conversation
- `~/.tamagotchi/memory/*.md` -- topic files, loaded by recency within a token budget

The agent updates its own memory using the `write_memory` tool.

### Heartbeat

When enabled, a background task periodically prompts the agent to check in. It respects quiet hours and suppresses duplicate or empty messages.

Enable in `~/.tamagotchi/config.toml`:
```toml
[heartbeat]
enabled = true
interval = "30m"
quiet_hours_start = "23:00"
quiet_hours_end = "07:00"
```

## Architecture

```
crates/
  cli/          REPL, argument parsing, heartbeat display
  core/         Agent loop, LLM client, memory, personality, config
  heartbeat/    Proactive check-in scheduler
  tools/        Tool manifest parsing, registry, subprocess execution
  sandbox/      macOS Seatbelt + Linux Bubblewrap sandboxing
  apply-patch/  Patch DSL parser + filesystem applicator
```

## Configuration

Edit `~/.tamagotchi/config.toml`. See [`.env.example`](.env.example) for required environment variables.

```toml
[llm]
api_key_env = "OPENROUTER_API_KEY"    # env var holding your key
model = "anthropic/claude-sonnet-4"  # any OpenRouter model
temperature = 0.7
max_tokens = 4096

[heartbeat]
enabled = false
interval = "30m"

[sandbox]
enabled = true
mode = "strict"

[memory]
max_context_tokens = 8000
```

## Development

```sh
cargo build                # Debug build
cargo test                 # Run all tests
cargo fmt                  # Format code
cargo clippy               # Lint
RUST_LOG=debug cargo run   # Run with debug logging
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and PR guidelines.

## License

[MIT](LICENSE)
