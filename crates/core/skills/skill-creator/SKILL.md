---
name: skill-creator
description: "Create, edit, or improve Tamagotchi skills. Use when: creating a new skill from scratch, improving an existing skill, or reviewing/auditing a SKILL.md file. Triggers on phrases like 'create a skill', 'make a skill', 'new skill'."
requires: {}
---

# Skill Creator

Guide for creating Tamagotchi skills. Skills are instruction bundles (SKILL.md files) that teach the agent how to use external CLI tools via `run_shell`.

## Skill Structure

```
skill-name/
└── SKILL.md    # YAML frontmatter + markdown instructions
```

Skills live at `~/.tamagotchi/skills/<skill-name>/SKILL.md`.

## SKILL.md Format

```markdown
---
name: my-skill
description: "What it does and when to use it. Be specific about triggers."
requires:
  bins: ["tool-name"]      # CLI tools checked via `which`
  env: ["API_KEY_VAR"]     # Environment variables checked via env
---

# Skill Title

Instructions for using the skill with run_shell.
Include concrete command examples the agent can execute.
```

## Creating a Skill

Use `apply_skill_patch` to create skills:

```
*** Begin Patch
*** Add File: my-skill/SKILL.md
---
name: my-skill
description: "Description of what it does and when to trigger it."
requires:
  bins: ["curl"]
---

# My Skill

Instructions and examples here.
*** End Patch
```

## Best Practices

- **Be concise**: only include information the agent doesn't already know
- **Include examples**: concrete `run_shell` command examples are more useful than explanations
- **Specific triggers**: the `description` field determines when the skill activates
- **List requirements**: specify `bins` and `env` so the agent knows what's needed
- **One skill per concern**: keep skills focused on a single tool or domain

## Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Lowercase, hyphen-separated skill name |
| `description` | Yes | What it does + when to use it (triggers skill activation) |
| `requires.bins` | No | CLI binaries that must be installed |
| `requires.env` | No | Environment variables that must be set |
