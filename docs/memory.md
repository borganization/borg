# Memory

Borg has a persistent memory system that carries context across sessions. The agent can read and write memory files, and relevant memories are automatically loaded into the system prompt each turn.

## Memory files

All memory lives in `~/.borg/`:

| File | Purpose | Loaded |
|------|---------|--------|
| `IDENTITY.md` | Personality and behavioral instructions | Always (as system prompt prefix) |
| `MEMORY.md` | Memory index — high-level notes | Always (first in memory context) |
| `memory/*.md` | Topic-specific memories | By recency, within token budget |

## How memory loading works

Each turn, the agent builds a memory context:

1. **MEMORY.md** is loaded first (always included if it fits the budget)
2. **memory/*.md** files are sorted by modification time (most recent first)
3. Files are included one by one until the token budget is exhausted
4. Files that would exceed the budget are skipped

Token estimation uses tiktoken-rs (cl100k_base BPE tokenizer).

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

- `filename`: target file (`IDENTITY.md`, `MEMORY.md`, or any name for a topic file)
- `content`: text to write
- `append`: if `true`, appends to existing content instead of overwriting

Special filenames:
- `IDENTITY.md` — writes to `~/.borg/IDENTITY.md` (personality)
- `MEMORY.md` — writes to `~/.borg/MEMORY.md` (index)
- Anything else — writes to `~/.borg/memory/<filename>`

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

Secrets in tool output are automatically redacted when `security.secret_detection` is enabled (default: true).

## IDENTITY.md

The personality file is special — it's loaded as the first part of the system prompt (before memory context). The agent can modify its own personality by writing to `IDENTITY.md`. Changes persist across sessions.

The default `IDENTITY.md` is created by `borg init`.

## Session persistence

Conversations are automatically saved as session files in `~/.borg/sessions/`. Sessions track:

- Full message history
- Token usage
- Model used
- Auto-generated title

Sessions are stored in the SQLite database (`borg.db`) with metadata for listing and retrieval. The most recent session can be automatically loaded on startup.

## Conversation compaction

When conversation history exceeds the token budget (`conversation.max_history_tokens`, default 32000), the agent can compact history by summarizing older messages while preserving recent context. This can also be triggered manually via the `/compact` slash command.

## Atomic rollback

The `/undo` command rolls back the last agent turn (both the assistant response and the preceding user message), allowing you to retry or rephrase.

## Memory cleanup

The `/memory cleanup` command helps manage memory files by identifying and removing stale or redundant entries.

## Tips

- Keep `MEMORY.md` as a concise index of what the agent knows
- Use topic-specific files (`memory/project-x.md`, `memory/meeting-notes.md`) for detailed context
- The agent will naturally learn to use memory as you interact with it
- Recently modified files are prioritized, so active topics stay in context
