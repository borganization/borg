# Tamagotchi — TODO

## Phase 1: Security Hardening & Test Coverage

- [x] **Path traversal prevention in memory read/write** — `write_memory` and `read_memory` accept arbitrary filenames without sanitization, allowing writes/reads outside `~/.tamagotchi/` (e.g., `../../.bashrc`)
- [x] **Path traversal prevention in apply_patch** — patch file paths are not validated against the base directory, allowing file operations outside the intended scope
- [x] **Heartbeat tests** — `parse_interval`, `is_quiet_hours`, and `hash_string` have zero test coverage
- [x] **Sandbox tests** — seatbelt profile generation and bubblewrap argument building are untested
- [x] **Tools manifest tests** — `ToolManifest` parsing, `parameters_json_schema()`, and `sandbox_policy()` have no tests

## Phase 2: Robustness & Code Quality

- [ ] **Agent loop iteration limit** — `run_agent_loop` has no max iterations; a misbehaving LLM could cause infinite tool-call loops
- [ ] **LlmClient reuse** — a new `LlmClient` is created on every agent loop iteration (`agent.rs:107`); should reuse the existing one
- [ ] **Deduplicate ToolDefinition types** — `tamagotchi_core::types::ToolDefinition` and `tamagotchi_tools::registry::tamagotchi_core_types::ToolDefinition` are identical but duplicated
- [ ] **LLM retry logic** — no retries on transient network failures for LLM API calls
- [ ] **Conversation history persistence** — history is lost when the process exits

## Phase 3: UX & Operational Improvements

- [ ] **Readline support** — replace raw stdin reading with `rustyline` for history, editing, and completion
- [ ] **Structured file logging** — log to `~/.tamagotchi/logs/` in addition to stderr
- [ ] **Tool output size limits** — user tool stdout is unbounded; large outputs could overwhelm context
- [ ] **Graceful shutdown** — no signal handling for clean exit (ctrl+c drops mid-stream)
- [ ] **run_shell safety** — consider confirmation prompt or allowlist for shell commands executed by the agent
