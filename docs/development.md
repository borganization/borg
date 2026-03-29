# Development

Guide for building, testing, and working on the Borg codebase.

## Prerequisites

- Rust stable toolchain (install via [rustup](https://rustup.rs/))
- `cargo fmt` and `cargo clippy` (included with rustup)
- Linux: `bubblewrap` package for sandbox tests

## Building

```sh
cargo build            # debug build
cargo build --release  # optimized build
```

The binary is `target/release/borg` (or `target/debug/borg`).

## Testing

```sh
cargo test                            # all tests
cargo test -p borg-apply-patch        # patch parser + applicator
cargo test -p borg-core               # config + skills + memory + settings tests
cargo test -p borg-heartbeat          # heartbeat scheduler tests
cargo test -p borg-tools              # tool manifest tests
cargo test -p borg-gateway            # channel manifest + registry tests
cargo test -p borg-plugins            # catalog + installer tests
```

## Linting

```sh
cargo fmt --check          # formatting check
cargo clippy -- -D warnings  # lint check (warnings are errors)
```

## Project structure

```
borg/
├── Cargo.toml              # workspace root (8 crates)
├── CLAUDE.md               # project instructions
├── docs/                   # documentation (you are here)
├── crates/
│   ├── cli/                # binary crate
│   │   └── src/
│   │       ├── main.rs         # entry point, clap commands
│   │       ├── repl.rs         # interactive loop + heartbeat rendering
│   │       ├── onboarding.rs   # TUI onboarding wizard (inquire-based)
│   │       ├── plugins.rs      # integration catalog, borg add/remove/plugins
│   │       ├── logo.rs         # ASCII art rendering
│   │       ├── service.rs      # daemon loop + launchd/systemd service management
│   │       └── tui/            # ratatui-based TUI
│   │           ├── mod.rs              # TUI core with event loop
│   │           ├── app.rs              # app state and rendering
│   │           ├── history.rs          # scrollable history view
│   │           ├── command_popup.rs    # slash command autocomplete
│   │           ├── settings_popup.rs   # interactive settings editing
│   │           ├── composer.rs         # input composition UI
│   │           ├── markdown.rs         # markdown rendering
│   │           ├── theme.rs            # color theme
│   │           ├── layout.rs           # layout composition
│   │           └── spinner.rs          # loading spinner
│   ├── core/               # core library
│   │   ├── src/
│   │   │   ├── agent.rs            # conversation loop + tool dispatch
│   │   │   ├── llm.rs              # multi-provider SSE streaming client
│   │   │   ├── provider.rs         # provider enum + auto-detection
│   │   │   ├── config.rs           # config parsing with serde defaults
│   │   │   ├── identity.rs         # IDENTITY.md load/save
│   │   │   ├── memory.rs           # memory loading with token budget + semantic search
│   │   │   ├── embeddings.rs       # embedding API client, cosine similarity
│   │   │   ├── skills.rs           # skills loading, parsing, progressive budgeting
│   │   │   ├── types.rs            # Message (with timestamps), ToolCall, ToolDefinition
│   │   │   ├── tool_handlers.rs    # built-in tool implementations
│   │   │   ├── tool_policy.rs      # composable tool filtering (profiles, allow/deny)
│   │   │   ├── tool_catalog.rs     # tool group definitions and profiles
│   │   │   ├── session.rs          # session persistence + auto-titling
│   │   │   ├── db.rs               # SQLite database with versioned migrations (V15)
│   │   │   ├── settings.rs         # three-tier settings resolver (DB -> TOML -> defaults)
│   │   │   ├── conversation.rs     # history compaction + normalization
│   │   │   ├── policy.rs           # execution policy (approve/deny)
│   │   │   ├── secrets.rs          # secret detection + redaction
│   │   │   ├── sanitize.rs         # prompt injection detection (scoring-based)
│   │   │   ├── rate_guard.rs       # per-session rate limiting
│   │   │   ├── hooks.rs            # lifecycle hook system
│   │   │   ├── doctor.rs           # diagnostic checks and report formatting
│   │   │   ├── browser.rs          # Chrome detection, CDP session management
│   │   │   ├── host_audit.rs       # host security audit checks
│   │   │   ├── pairing.rs          # sender pairing/access control
│   │   │   ├── web.rs              # web fetch + search
│   │   │   ├── tasks.rs            # scheduled task definitions
│   │   │   ├── image_gen.rs        # image generation (OpenAI/fal)
│   │   │   ├── tts.rs              # text-to-speech synthesis
│   │   │   ├── telemetry.rs        # OpenTelemetry tracing/metrics
│   │   │   ├── media.rs            # image compression + media processing
│   │   │   ├── logging.rs          # daily JSONL logging
│   │   │   ├── retry.rs            # exponential backoff retry
│   │   │   ├── tokenizer.rs        # tiktoken-rs token estimation
│   │   │   ├── truncate.rs         # tool output truncation
│   │   │   ├── multi_agent/        # multi-agent orchestration
│   │   │   ├── integrations/       # native tool integrations (Gmail, Outlook, Calendar, Notion, Linear)
│   │   │   └── lib.rs
│   │   └── skills/             # built-in skill definitions (15 skills)
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
│   │       ├── 1password/SKILL.md
│   │       ├── browser/SKILL.md
│   │       └── scheduler/SKILL.md
│   ├── heartbeat/          # heartbeat scheduler
│   │   └── src/
│   │       ├── scheduler.rs    # interval/cron scheduling, quiet hours, wake signal
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
│   ├── apply-patch/        # patch DSL
│   │   └── src/
│   │       ├── parser.rs    # DSL parser (Add, Update, Delete, Move)
│   │       ├── apply.rs     # filesystem applicator
│   │       └── lib.rs
│   ├── gateway/            # webhook gateway
│   │   └── src/
│   │       ├── server.rs    # Axum HTTP server, webhook routes
│   │       ├── handler.rs   # message processing, pairing, agent invocation
│   │       ├── manifest.rs  # channel.toml parsing
│   │       ├── registry.rs  # channel discovery + registration
│   │       ├── executor.rs  # channel script subprocess execution
│   │       ├── telegram/    # native Telegram integration
│   │       ├── slack/       # native Slack integration
│   │       ├── discord/     # native Discord integration
│   │       ├── teams/       # native Teams integration
│   │       ├── google_chat/ # native Google Chat integration
│   │       ├── signal/      # native Signal integration (SSE daemon)
│   │       ├── twilio/      # native Twilio integration (SMS/WhatsApp)
│   │       ├── imessage/    # native iMessage integration (macOS)
│   │       └── lib.rs
│   └── plugins/            # plugin marketplace
│       └── src/
│           ├── catalog.rs   # plugin registry (messaging, email, productivity)
│           ├── installer.rs # template installer
│           ├── verifier.rs  # file integrity checking
│           ├── keychain.rs  # credential storage helpers
│           └── lib.rs
└── .env.example            # env var template
```

## Runtime requirements

The binary requires one of `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `DEEPSEEK_API_KEY`, `GROQ_API_KEY`, or a running Ollama instance at runtime. For development, create a `.env` file from the example:

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

## Database

SQLite at `~/.borg/borg.db` with versioned migrations (currently V15). Key migrations:

- **V1**: sessions, scheduled_tasks, task_runs, meta, token_usage
- **V2**: messages table (crash recovery)
- **V3**: channel_sessions + channel_messages (gateway)
- **V13**: pairing_requests + approved_senders
- **V14**: retry, timeout, and delivery columns on scheduled_tasks

Schema version is tracked in the `meta` table; migrations run automatically on `Database::open()`.
