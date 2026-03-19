# User Tools

The agent can create its own tools at runtime. These are sandboxed scripts that the agent writes via the `create_tool` tool and then invokes in future conversations.

## How it works

1. During a conversation, the agent decides it needs a new capability
2. It uses `create_tool` to create a `tool.toml` manifest and entrypoint script in `~/.borg/tools/<name>/`
3. The tool registry reloads automatically
4. The tool is now available as a callable function in all future turns

## Tool structure

Each tool lives in its own directory under `~/.borg/tools/`:

```
~/.borg/tools/
└── weather/
    ├── tool.toml      # Manifest: name, description, runtime, sandbox policy, parameters
    └── main.py        # Entrypoint script
```

## Manifest format (`tool.toml`)

```toml
name = "weather"
description = "Get current weather for a city"
runtime = "python"          # python | node | deno | bash
entrypoint = "main.py"     # script to execute
timeout_ms = 30000          # max execution time in milliseconds

[sandbox]
network = true              # allow network access
fs_read = ["/etc/ssl"]      # allowed read paths
fs_write = []               # allowed write paths

[parameters]
type = "object"

[parameters.properties.city]
type = "string"
description = "City name"

[parameters.required]
values = ["city"]

[credentials]
# key-value pairs mapping to config [credentials] entries
# api_key = "weather_api"
```

### Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `name` | yes | — | Tool identifier (must match directory name) |
| `description` | yes | — | What the tool does (shown to the LLM) |
| `runtime` | no | `"python"` | Script runtime: `python`, `node`, `deno`, or `bash` |
| `entrypoint` | no | `"main.py"` | Script filename to execute |
| `timeout_ms` | no | `30000` | Maximum execution time |

### Sandbox section

| Field | Default | Description |
|-------|---------|-------------|
| `network` | `false` | Whether the tool can make network requests |
| `fs_read` | `[]` | Filesystem paths the tool can read (beyond its own directory) |
| `fs_write` | `[]` | Filesystem paths the tool can write to |

### Parameters section

Parameters are defined in TOML and converted to JSON Schema for the LLM. Each property needs a `type` and `description`. Required parameters are listed in `[parameters.required].values`.

### Credentials section

The `[credentials]` section maps tool-local names to keys in the global `[credentials]` config. This lets the tool access API keys stored as environment variable references in `config.toml`.

## Tool execution

1. The registry resolves the runtime binary (`python3`, `node`, `deno`, `bash`)
2. The sandbox policy wraps the command (see [Sandboxing](sandboxing.md))
3. The tool receives its arguments as a JSON object on **stdin**
4. The tool writes its result to **stdout**
5. Exit code, stdout, and stderr are captured and returned to the agent

### Example entrypoint (`main.py`)

```python
import json
import sys
import urllib.request

args = json.loads(sys.stdin.read())
city = args["city"]

url = f"https://wttr.in/{city}?format=j1"
response = urllib.request.urlopen(url)
data = json.loads(response.read())

current = data["current_condition"][0]
print(f"{city}: {current['temp_C']}°C, {current['weatherDesc'][0]['value']}")
```

## Built-in tools

These are always available and don't require tool.toml manifests:

| Tool | Description |
|------|-------------|
| `write_memory` | Write or append to memory files (IDENTITY.md, MEMORY.md, or topic files) |
| `read_memory` | Read a memory file |
| `list_tools` | List all user-created tools |
| `apply_patch` | Create/update/delete files in the current working directory via [Patch DSL](patch-dsl.md) |
| `create_tool` | Create/modify files in `~/.borg/tools/` via [Patch DSL](patch-dsl.md) |
| `run_shell` | Execute a shell command (subject to [execution policy](configuration.md#policy)) |
| `list_skills` | List all skills with status and source |
| `apply_skill_patch` | Create/modify files in `~/.borg/skills/` via [Patch DSL](patch-dsl.md) |

### Conditional tools

These tools are available when their corresponding config section is enabled:

| Tool | Config | Description |
|------|--------|-------------|
| `web_fetch` | `[web] enabled = true` | Fetch a URL and convert HTML to text |
| `web_search` | `[web] enabled = true` | Search the web (DuckDuckGo or Brave) |
| `schedule_task` | `[tasks] enabled = true` | Create a scheduled task (cron, interval, or one-time) |
| `list_scheduled_tasks` | `[tasks] enabled = true` | List all scheduled tasks with status |
| `pause_task` | `[tasks] enabled = true` | Pause a running scheduled task |
| `resume_task` | `[tasks] enabled = true` | Resume a paused scheduled task |
| `cancel_task` | `[tasks] enabled = true` | Cancel a scheduled task |
