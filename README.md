# Borg

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Next generation personal AI assistant.**

## What is Borg?

Borg is a personal AI assistant that runs locally on your machine, remembers you across conversations, and gets better over time. Unlike cloud-only chatbots, it develops its own personality, learns your preferences, and is infinitely customizable through plugins.

## Features

- **Remembers you** — maintains memory across every conversation so it never forgets what matters to you
- **Multiple AI providers** — works with OpenRouter, OpenAI, Anthropic, and Gemini so you can use the model you prefer
- **Connects to your apps** — integrates with Slack, Telegram, and your favorite messaging platforms
- **Built-in skills** — comes with skills for weather, calendar, notes, GitHub, search, Docker, and more
- **Proactive check-ins** — can reach out on its own with reminders, ideas, or just to say hello (with quiet hours so it won't bother you at night)
- **Evolving personality** — develops its own voice and style over time based on how you interact
- **Safe and sandboxed** — all tools run in a secure sandbox so nothing can touch your files without permission

## Quick Start

1. **Download Borg** from [Releases](https://github.com/borganization/borg/releases)

2. **Run it:**
    ```sh
    borg
    ```

## Plugins

Connect Borg to the apps you already use. Add a plugin in one command:

```sh
borg add telegram
borg add gmail
borg plugins             # see all available plugins
```

**Available plugins:** Telegram, Slack, Twilio (WhatsApp + SMS), Gmail, Outlook, Google Calendar, Notion, Linear. iMessage works automatically on macOS.

## Commands

| Command              | What it does                                   |
| -------------------- | ---------------------------------------------- |
| `borg`               | Start an interactive conversation              |
| `borg ask "..."`     | Ask a quick question and get a one-shot answer |
| `borg add <name>`    | Set up a plugin (e.g. `borg add telegram`)     |
| `borg remove <name>` | Remove a plugin's credentials                  |
| `borg plugins`       | See all available plugins and their status     |
| `borg doctor`        | Check that everything is configured correctly  |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and PR guidelines.

## License

[MIT](LICENSE)
