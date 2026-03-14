# Memory

Tamagotchi has a persistent memory system that carries context across sessions. The agent can read and write memory files, and relevant memories are automatically loaded into the system prompt each turn.

## Memory files

All memory lives in `~/.tamagotchi/`:

| File | Purpose | Loaded |
|------|---------|--------|
| `SOUL.md` | Personality and behavioral instructions | Always (as system prompt prefix) |
| `MEMORY.md` | Memory index — high-level notes | Always (first in memory context) |
| `memory/*.md` | Topic-specific memories | By recency, within token budget |

## How memory loading works

Each turn, the agent builds a memory context:

1. **MEMORY.md** is loaded first (always included if it fits the budget)
2. **memory/*.md** files are sorted by modification time (most recent first)
3. Files are included one by one until the token budget is exhausted
4. Files that would exceed the budget are skipped

Token estimation uses a simple heuristic: `length_in_chars / 4`.

### Configuration

```toml
[memory]
max_context_tokens = 8000    # token budget for memory in system prompt
```

## Writing memory

The agent uses the `write_memory` tool:

```json
{
  "filename": "user_preferences.md",
  "content": "# User Preferences\n\n- Prefers concise answers\n- Timezone: PST",
  "append": false
}
```

- `filename`: target file (`SOUL.md`, `MEMORY.md`, or any name for a topic file)
- `content`: text to write
- `append`: if `true`, appends to existing content instead of overwriting

Special filenames:
- `SOUL.md` — writes to `~/.tamagotchi/SOUL.md` (personality)
- `MEMORY.md` — writes to `~/.tamagotchi/MEMORY.md` (index)
- Anything else — writes to `~/.tamagotchi/memory/<filename>`

## Reading memory

The agent uses the `read_memory` tool:

```json
{
  "filename": "user_preferences.md"
}
```

Returns the file contents, or a "not found" message if the file doesn't exist.

## Security

Memory filenames are validated to prevent path traversal:
- No `..` sequences allowed
- No `/` or `\` separators allowed
- Empty filenames are rejected

## SOUL.md

The personality file is special — it's loaded as the first part of the system prompt (before memory context). The agent can modify its own personality by writing to `SOUL.md`. Changes persist across sessions.

The default `SOUL.md` is created by `tamagotchi init`.

## Tips

- Keep `MEMORY.md` as a concise index of what the agent knows
- Use topic-specific files (`memory/project-x.md`, `memory/meeting-notes.md`) for detailed context
- The agent will naturally learn to use memory as you interact with it
- Recently modified files are prioritized, so active topics stay in context
