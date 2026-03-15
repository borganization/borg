# Tamagotchi

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**A personal AI assistant that lives on your computer.**

## What is Tamagotchi?

Tamagotchi is an AI assistant that runs locally on your machine, remembers you across conversations, and gets better over time. Unlike cloud-only chatbots, it develops its own personality, learns your preferences, and even builds its own tools when it needs new capabilities — no plugins or app store required.

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
   git clone https://github.com/theognis1002/tamagotchi.git
   cd tamagotchi
   cargo build --release
   ```

2. **Set your API key** (pick one):
   ```sh
   export OPENROUTER_API_KEY="sk-or-..."
   # or OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY
   ```

3. **Run the setup wizard:**
   ```sh
   ./target/release/tamagotchi init
   ```

4. **Start chatting:**
   ```sh
   ./target/release/tamagotchi
   ```

## Commands

| Command | What it does |
|---------|-------------|
| `tamagotchi` | Start an interactive conversation |
| `tamagotchi ask "..."` | Ask a quick question and get a one-shot answer |
| `tamagotchi init` | Run the setup wizard (name, personality, provider) |
| `tamagotchi gateway` | Start the webhook server for messaging integrations |
| `tamagotchi doctor` | Check that everything is configured correctly |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and PR guidelines.

## License

[MIT](LICENSE)
