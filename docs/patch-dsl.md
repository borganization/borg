# Patch DSL

Borg uses a custom patch DSL for creating, modifying, and deleting files. The agent uses this via the `apply_patch` and `apply_skill_patch` built-in tools.

## Format

A patch is wrapped in `*** Begin Patch` / `*** End Patch` markers and contains one or more file operations:

```
*** Begin Patch
*** Add File: path/to/file.txt
file contents here
*** End Patch
```

## Operations

### Add File

Creates a new file with the given contents:

```
*** Begin Patch
*** Add File: my-tool/tool.toml
name = "my-tool"
description = "Does something"
runtime = "python"
entrypoint = "main.py"
*** Add File: my-tool/main.py
import json, sys
args = json.loads(sys.stdin.read())
print(f"Hello, {args['name']}!")
*** End Patch
```

Multiple files can be added in a single patch.

### Update File

Modifies an existing file using unified diff hunks:

```
*** Begin Patch
*** Update File: my-tool/main.py
@@ -1,3 +1,4 @@
 import json, sys
+import os
 args = json.loads(sys.stdin.read())
-print(f"Hello, {args['name']}!")
+name = args.get('name', os.getenv('USER', 'world'))
+print(f"Hello, {name}!")
*** End Patch
```

Hunk format follows unified diff conventions:
- `@@ -start,count +start,count @@` — hunk header
- Lines starting with ` ` (space) are context (must match existing content)
- Lines starting with `-` are removed
- Lines starting with `+` are added

Multiple hunks can be applied to the same file in a single update.

### Delete File

Removes a file:

```
*** Begin Patch
*** Delete File: my-tool/old-script.py
*** End Patch
```

## Mixed operations

A single patch can combine add, update, and delete operations:

```
*** Begin Patch
*** Add File: new-tool/tool.toml
name = "new-tool"
description = "A new tool"
*** Update File: existing-tool/main.py
@@ -1,2 +1,2 @@
 import json, sys
-print("old version")
+print("new version")
*** Delete File: deprecated-tool/main.py
*** End Patch
```

## Base directories

- `apply_patch` operates on `~/.borg/tools/` — file paths are relative to this directory
- `apply_skill_patch` operates on `~/.borg/skills/` — file paths are relative to this directory

## Error handling

The parser validates:
- Patch markers are present and properly formatted
- Hunk headers have valid line numbers
- Context lines match the existing file content
- Files to update exist; files to add don't already exist (unless overwriting)

If any operation fails, the error is returned to the agent with details about what went wrong.
