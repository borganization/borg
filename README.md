# Borg

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**A personal AI assistant that lives on your computer.**

## What is Borg?

Borg is an AI assistant that runs locally on your machine, remembers you across conversations, and gets better over time. Unlike cloud-only chatbots, it develops its own personality, learns your preferences, and even builds its own tools when it needs new capabilities — no plugins or app store required.

## Features

- **Remembers you** — maintains memory across every conversation so it never forgets what matters to you
- **Multiple AI providers** — works with OpenRouter, OpenAI, Anthropic, and Gemini so you can use the model you prefer
- **Proactive check-ins** — can reach out on its own with reminders, ideas, or just to say hello (with quiet hours so it won't bother you at night)
- **Creates its own tools** — when it needs a new capability, it writes and installs the tool on the fly
- **Evolving personality** — develops its own voice and style over time based on how you interact
- **Safe and sandboxed** — all tools run in a secure sandbox so nothing can touch your files without permission
- **Connects to your apps** — integrates with Slack, Discord, and other messaging platforms via webhooks
- **Built-in skills** — comes with skills for weather, calendar, notes, GitHub, search, Docker, and more

## Quick Start

1. **Clone and build:**
   ```sh
   git clone https://github.com/borganization/borg.git
   cd borg
   cargo build --release
   ```

2. **Set your API key** (pick one):
   ```sh
   export OPENROUTER_API_KEY="sk-or-..."
   # or OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY
   ```

3. **Run the setup wizard:**
   ```sh
   ./target/release/borg init
   ```

4. **Start chatting:**
   ```sh
   ./target/release/borg
   ```

## Commands

| Command | What it does |
|---------|-------------|
| `borg` | Start an interactive conversation |
| `borg ask "..."` | Ask a quick question and get a one-shot answer |
| `borg init` | Run the setup wizard (name, personality, provider) |
| `borg add <name>` | Set up an integration (e.g. `borg add telegram`) |
| `borg remove <name>` | Remove an integration's credentials |
| `borg plugins` | See all available integrations and their status |
| `borg gateway` | Start the webhook server for messaging integrations |
| `borg doctor` | Check that everything is configured correctly |

## Integrations

Everything is compiled into one binary. Set up integrations by configuring credentials:

```sh
borg add telegram        # configure Telegram bot token
borg add gmail           # configure Gmail API key
borg plugins             # see all available integrations
```

**Available integrations:** Telegram, Slack, Twilio (WhatsApp + SMS), Gmail, Outlook, Google Calendar, Notion, Linear. iMessage is built-in on macOS (no credentials needed).

Skills and channels can also be added at runtime — Borg creates its own tools and skills on the fly without recompiling.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and PR guidelines.

## License

[MIT](LICENSE)
