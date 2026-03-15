---
name: scheduler
description: "Create and manage scheduled tasks using the manage_tasks tool. Covers cron, interval, and one-shot schedules."
requires:
  bins: []
  env: []
---

# Task Scheduler

Use the `manage_tasks` tool to create and manage scheduled tasks. Tasks run automatically via the daemon (`tamagotchi daemon` or `tamagotchi service install`).

## Prerequisites

- `tasks.enabled = true` in config (default: true)
- Daemon must be running to execute tasks

## Tool: manage_tasks

Single tool with an `action` parameter:

| Action   | Required params                              | Description                     |
|----------|----------------------------------------------|---------------------------------|
| create   | name, prompt, schedule_type, schedule_expr   | Create a new task               |
| list     | (none)                                       | List all tasks                  |
| get      | task_id                                      | Get details for one task        |
| update   | task_id + any of: name, prompt, schedule_type, schedule_expr, timezone | Update a task |
| pause    | task_id                                      | Pause (skip runs until resumed) |
| resume   | task_id                                      | Resume a paused task            |
| cancel   | task_id                                      | Cancel permanently              |
| delete   | task_id                                      | Delete task and its run history |

## Schedule Types

### Cron (7-field with seconds)

The `cron` crate uses 7 fields: `sec min hour day_of_month month day_of_week year`

```
0 0 8 * * * *      → Every day at 08:00:00
0 30 9 * * Mon *    → Every Monday at 09:30:00
0 0 */2 * * * *     → Every 2 hours
0 0 9 1 * * *       → First of every month at 09:00
```

### Interval

Human-readable durations:

```
30m   → every 30 minutes
2h    → every 2 hours
1d    → every day
60s   → every 60 seconds
```

### Once

One-shot tasks that execute immediately on the next daemon tick and then auto-complete.

## Common Patterns

- **Daily digest**: `create` with cron `0 0 8 * * * *`, prompt "Summarize my calendar and unread emails"
- **Weekly review**: `create` with cron `0 0 10 * * Fri *`, prompt "Review my open PRs and issues"
- **Periodic check**: `create` with interval `2h`, prompt "Check deployment status"
- **One-time reminder**: `create` with `once`, prompt "Remind me to submit the report"
