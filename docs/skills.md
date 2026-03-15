# Skills

Skills are instruction bundles that teach the agent how to use external CLI tools. They are distinct from "tools" — skills are text-based instructions injected into the system prompt, while tools are sandboxed executable scripts.

## How skills work

1. At each turn, available skills are loaded and formatted into the system prompt
2. The agent reads the skill instructions and can use `run_shell` to execute the described CLI commands
3. Skills declare their requirements (binaries, env vars) which are checked at load time
4. Unavailable skills (missing requirements) are still listed but flagged as unavailable

## Built-in skills

Thirteen skills are embedded in the binary:

| Skill | Requirements | Description |
|-------|-------------|-------------|
| `slack` | `curl`, `SLACK_BOT_TOKEN` | Send messages to Slack channels |
| `discord` | `curl`, `DISCORD_BOT_TOKEN` | Send messages to Discord channels |
| `github` | `gh` | Interact with GitHub (issues, PRs, repos) |
| `weather` | `curl` | Get weather information |
| `skill-creator` | — | Meta-skill for creating new skills |
| `git` | `git` | Git operations (commit, branch, diff, log) |
| `http` | `curl` | HTTP requests (GET, POST, PUT, DELETE) |
| `search` | `curl` | Web search integration |
| `docker` | `docker` | Docker container management |
| `database` | varies | SQL/database operations |
| `notes` | — | Note-taking and organization |
| `calendar` | varies | Calendar operations |
| `1password` | `op` | 1Password secret management |

## User skills

User-created skills live at `~/.tamagotchi/skills/<name>/SKILL.md`. The agent creates these via the `apply_skill_patch` tool.

User skills with the same name as a built-in skill **override** the built-in version.

## SKILL.md format

```markdown
---
name: my-skill
description: "What it does and when to use it."
requires:
  bins: ["curl", "jq"]
  env: ["API_TOKEN"]
---

# My Skill

Instructions for the agent on how to use this skill.

## Examples

\`\`\`bash
curl -H "Authorization: Bearer $API_TOKEN" https://api.example.com/data | jq '.results'
\`\`\`
```

### Frontmatter fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Skill identifier (should match directory name) |
| `description` | yes | When and why the agent should use this skill |
| `requires.bins` | no | List of CLI binaries that must be on `$PATH` |
| `requires.env` | no | List of environment variables that must be set |

### Body

The markdown body after the frontmatter contains instructions, command examples, and usage patterns. Write it as if you're teaching someone how to use the CLI tools — the agent will follow these instructions literally.

## Requirement checking

At load time, each skill's requirements are validated:

- **Binaries**: checked via `which`
- **Environment variables**: checked via `std::env::var`

If any requirement is missing, the skill is marked as unavailable. It still appears in `list_skills` output but won't be injected into the system prompt context.

## Token budgeting

Skills are injected into the system prompt with a configurable token budget (`skills.max_context_tokens`, default 4000). Available skills are included first, then unavailable ones, until the budget is exhausted.

Configure the budget in `~/.tamagotchi/config.toml`:

```toml
[skills]
enabled = true
max_context_tokens = 4000
```

## Creating a skill

The agent can create skills during a conversation using `apply_skill_patch`:

```
*** Begin Patch
*** Add File: my-api/SKILL.md
---
name: my-api
description: "Query the My API service for data."
requires:
  bins: ["curl"]
  env: ["MY_API_KEY"]
---

# My API

Use curl to query the API:

\`\`\`bash
curl -H "Authorization: Bearer $MY_API_KEY" https://api.example.com/v1/query?q=SEARCH_TERM
\`\`\`
*** End Patch
```

You can also create skill files manually in `~/.tamagotchi/skills/<name>/SKILL.md`.
