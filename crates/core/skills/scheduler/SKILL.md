---
name: scheduler
description: "Create and manage scheduled tasks and cron jobs"
requires:
  bins: []
  env: []
---

# Task Scheduler

Two tools for scheduled automation:

- **`manage_tasks`** — Schedule prompts that run through the LLM (AI tasks)
- **`manage_cron`** — Schedule shell commands that execute directly (Linux-style cron jobs)

Both run automatically via the daemon (`borg daemon` or `borg service install`).

## Prerequisites

- Daemon must be running to execute tasks (starts automatically)

## Tool: manage_tasks (AI Tasks)

Schedule prompts that are sent to the LLM on a schedule.

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

### Schedule Types

#### Cron (7-field with seconds)

The `cron` crate uses 7 fields: `sec min hour day_of_month month day_of_week year`

```
0 0 8 * * * *      → Every day at 08:00:00
0 30 9 * * Mon *    → Every Monday at 09:30:00
0 0 */2 * * * *     → Every 2 hours
0 0 9 1 * * *       → First of every month at 09:00
```

#### Interval

Human-readable durations:

```
30m   → every 30 minutes
2h    → every 2 hours
1d    → every day
60s   → every 60 seconds
```

#### Once

One-shot tasks that execute immediately on the next daemon tick and then auto-complete.

### Common Patterns

- **Daily digest**: `create` with cron `0 0 8 * * * *`, prompt "Summarize my calendar and unread emails"
- **Weekly review**: `create` with cron `0 0 10 * * Fri *`, prompt "Review my open PRs and issues"
- **Periodic check**: `create` with interval `2h`, prompt "Check deployment status"
- **One-time reminder**: `create` with `once`, prompt "Remind me to submit the report"

## Tool: manage_cron (Shell Cron Jobs)

Schedule shell commands that execute directly — no LLM involved. Like Linux crontab.

| Action   | Required params          | Description                        |
|----------|--------------------------|------------------------------------|
| create   | schedule, command        | Create a new cron job              |
| list     | (none)                   | List all cron jobs                 |
| get      | job_id                   | Get details for one job            |
| delete   | job_id                   | Delete a cron job                  |
| pause    | job_id                   | Pause (skip runs until resumed)    |
| resume   | job_id                   | Resume a paused job                |
| runs     | job_id                   | Show execution history             |
| run_now  | job_id                   | Trigger immediate execution        |

### Parameters

- **schedule**: 5-field Linux cron expression (e.g. `*/5 * * * *`). Automatically converted to 7-field internally.
- **command**: Shell command to execute (e.g. `python3 /opt/backup.py`)
- **name**: Optional human-readable name (auto-generated from command if omitted)
- **timeout_ms**: Execution timeout in milliseconds (default: 300000 = 5 min)
- **delivery_channel**: Send output to telegram, slack, or discord
- **delivery_target**: Chat/channel ID for delivery

### Cron Schedule Format (5-field Linux)

```
*/5 * * * *        → Every 5 minutes
0 3 * * *          → Daily at 3:00 AM
0 0 * * 0          → Every Sunday at midnight
0 9 1 * *          → First of every month at 9:00 AM
30 */2 * * 1-5     → Every 2 hours at :30, weekdays only
```

### Common Patterns

- **Backup script**: `create` with schedule `0 3 * * *`, command `python3 /opt/backup/run.py`
- **Health check**: `create` with schedule `*/5 * * * *`, command `curl -sf http://localhost:8080/health`
- **Log rotation**: `create` with schedule `0 0 * * 0`, command `/usr/local/bin/rotate-logs.sh`
- **Disk cleanup**: `create` with schedule `0 4 * * *`, command `find /tmp -mtime +7 -delete`
- **Custom script**: `create` with schedule `*/10 * * * *`, command `python3 /opt/monitor/check.py`

### Key Differences from manage_tasks

| Aspect | manage_tasks | manage_cron |
|--------|-------------|-------------|
| Executes | LLM prompt (AI response) | Shell command directly |
| Use for | AI reasoning, summaries, checks | Scripts, backups, system commands |
| Output | LLM-generated text | stdout/stderr from command |
| Retry | Retries on transient LLM errors | No retry on non-zero exit |
