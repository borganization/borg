# Development

Guide for building, testing, and working on the Tamagotchi codebase.

## Prerequisites

- Rust stable toolchain (install via [rustup](https://rustup.rs/))
- `cargo fmt` and `cargo clippy` (included with rustup)
- Linux: `bubblewrap` package for sandbox tests

## Building

```sh
cargo build            # debug build
cargo build --release  # optimized build
```

The binary is `target/release/tamagotchi` (or `target/debug/tamagotchi`).

## Testing

```sh
cargo test                            # all tests
cargo test -p tamagotchi-apply-patch  # patch parser + applicator (13 tests)
cargo test -p tamagotchi-core         # config + skills + memory tests
cargo test -p tamagotchi-heartbeat    # heartbeat scheduler tests
cargo test -p tamagotchi-tools        # tool manifest tests
```

## Linting

```sh
cargo fmt --check          # formatting check
cargo clippy -- -D warnings  # lint check (warnings are errors)
```

## Project structure

```
tamagotchi/
├── Cargo.toml              # workspace root
├── CLAUDE.md               # project instructions
├── docs/                   # documentation (you are here)
├── crates/
│   ├── cli/                # binary crate
│   │   └── src/
│   │       ├── main.rs     # entry point, clap commands, init
│   │       ├── repl.rs     # interactive loop + heartbeat rendering
│   │       ├── onboarding.rs # TUI onboarding wizard
│   │       ├── logo.rs     # ASCII art rendering
│   │       ├── service.rs  # daemon loop + launchd/systemd service management
│   │       └── tui/        # ratatui-based TUI
│   │           ├── mod.rs          # TUI core with event loop
│   │           ├── app.rs          # app state and rendering
│   │           ├── history.rs      # scrollable history view
│   │           ├── command_popup.rs # slash command autocomplete
│   │           ├── composer.rs     # input composition UI
│   │           ├── markdown.rs     # markdown rendering
│   │           ├── theme.rs        # color theme
│   │           ├── layout.rs       # layout composition
│   │           └── spinner.rs      # loading spinner
│   ├── core/               # core library
│   │   ├── src/
│   │   │   ├── agent.rs        # conversation loop + tool dispatch
│   │   │   ├── llm.rs          # multi-provider SSE streaming client
│   │   │   ├── provider.rs     # provider enum + auto-detection
│   │   │   ├── config.rs       # config parsing with serde defaults
│   │   │   ├── soul.rs         # SOUL.md load/save
│   │   │   ├── memory.rs       # memory loading with token budget
│   │   │   ├── skills.rs       # skills loading, parsing, budgeting
│   │   │   ├── types.rs        # Message, ToolCall, ToolDefinition
│   │   │   ├── session.rs      # session persistence + auto-titling
│   │   │   ├── db.rs           # SQLite database (sessions, tasks)
│   │   │   ├── conversation.rs # history compaction + normalization
│   │   │   ├── policy.rs       # execution policy (approve/deny)
│   │   │   ├── secrets.rs      # secret detection + redaction
│   │   │   ├── web.rs          # web fetch + search
│   │   │   ├── tasks.rs        # scheduled task definitions
│   │   │   ├── logging.rs      # daily JSONL logging
│   │   │   ├── retry.rs        # exponential backoff retry
│   │   │   ├── tokenizer.rs    # tiktoken-rs token estimation
│   │   │   ├── truncate.rs     # tool output truncation
│   │   │   └── lib.rs
│   │   └── skills/         # built-in skill definitions
│   │       ├── slack/SKILL.md
│   │       ├── discord/SKILL.md
│   │       ├── github/SKILL.md
│   │       ├── weather/SKILL.md
│   │       ├── skill-creator/SKILL.md
│   │       ├── git/SKILL.md
│   │       ├── http/SKILL.md
│   │       ├── search/SKILL.md
│   │       ├── docker/SKILL.md
│   │       ├── database/SKILL.md
│   │       ├── notes/SKILL.md
│   │       ├── calendar/SKILL.md
│   │       └── 1password/SKILL.md
│   ├── heartbeat/          # heartbeat scheduler
│   │   └── src/
│   │       ├── scheduler.rs
│   │       └── lib.rs
│   ├── tools/              # user tool management
│   │   └── src/
│   │       ├── manifest.rs  # tool.toml parsing
│   │       ├── registry.rs  # tool discovery + registration
│   │       ├── executor.rs  # runtime resolution + subprocess
│   │       └── lib.rs
│   ├── sandbox/            # platform sandboxing
│   │   └── src/
│   │       ├── policy.rs    # SandboxPolicy + command wrapping
│   │       ├── seatbelt.rs  # macOS profile generation
│   │       ├── bubblewrap.rs # Linux bwrap args
│   │       └── lib.rs
│   └── apply-patch/        # patch DSL
│       └── src/
│           ├── parser.rs    # DSL parser
│           ├── apply.rs     # filesystem applicator
│           └── lib.rs
└── .env.example            # env var template
```

## Runtime requirements

The binary requires one of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `GEMINI_API_KEY` set at runtime. For development, create a `.env` file from the example:

```sh
cp .env.example .env
# edit .env with your key
```

## Adding a built-in skill

1. Create `crates/core/skills/<name>/SKILL.md` with YAML frontmatter
2. Add `const BUILTIN_<NAME>: &str = include_str!("../skills/<name>/SKILL.md");` in `crates/core/src/skills.rs`
3. Add the constant to the `builtins_raw` array in `load_all_skills()`
4. Add a test case in the `builtins_parse_correctly` test

## Adding a new crate

1. Create the crate directory under `crates/`
2. Add it to the workspace `members` in the root `Cargo.toml`
3. Add workspace lint configuration
4. Wire up dependencies from existing crates as needed
