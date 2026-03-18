# Feature Parity: Borg vs OpenClaw

Target: 85% core feature parity.

## Already Implemented (No Work Needed)

| Feature                     | Location                                                                          |
| --------------------------- | --------------------------------------------------------------------------------- |
| Session compaction          | `crates/core/src/conversation.rs` — `plan_compaction` + `compact_history`         |
| Web search/fetch tools      | `crates/core/src/web.rs` + `tool_handlers.rs` (DuckDuckGo + Brave)                |
| Multi-agent support         | `crates/core/src/multi_agent/` — spawn, roles, tools                              |
| Sub-agent spawning          | `crates/core/src/multi_agent/mod.rs` — `spawn_agent` tool                         |
| Model key failover/rotation | `crates/core/src/llm.rs` — `fallback_keys`, `try_rotate_key`                      |
| Message chunking            | `crates/gateway/src/chunker.rs` — paragraph/line/sentence/hard split              |
| Telegram typing indicators  | `crates/gateway/src/telegram/api.rs` — `send_typing`                              |
| Execution policy (shell)    | `crates/core/src/policy.rs` — auto_approve/deny glob patterns + hardcoded denials |
| HITL dangerous ops          | `crates/core/src/agent.rs` — file deletion, IDENTITY.md changes                   |

---

## P0 — Must Have for 85% Parity

### 1. Semantic Memory Search (Embedding-based)

- [x] Add embedding generation on `write_memory`
- [x] Add vector similarity query to `load_memory_context`
- [x] Add `[memory.embeddings]` config (provider, model, dimension)
- [x] DB migration for embeddings table (SQLite BLOB storage)
- [x] Fallback to recency when embeddings unavailable

**Why:** Recency-only loading is broken for large memory stores. A conversation about "budget" should retrieve budget-related memory from weeks ago, not yesterday's random notes.

**Size:** L
**Files:** `memory.rs`, `config.rs`, `db.rs`, `tool_handlers.rs`

### 2. Browser Automation Tool (CDP)

- [x] Extend `browser.rs` from detection-only to CDP client (`chromiumoxide` crate)
- [x] Register `browser` built-in tool with actions: navigate, click, type, screenshot, get_text, evaluate_js
- [x] Persistent browser session per agent session
- [x] Screenshots returned as base64 `ImageBase64` content parts

**Why:** Web interaction is a core agent capability. Chrome detection and config exist but no tool is wired up.

**Size:** L
**Files:** `browser.rs`, `tool_handlers.rs`, `agent.rs`

### 3. Token Usage Tracking per Provider/Model

- [ ] DB migration adding `provider`, `model`, `cost_usd` columns to `token_usage`
- [ ] Pass provider/model to `log_token_usage` in agent loop
- [ ] Extract model/provider from LLM response metadata
- [ ] New query methods for per-provider/model usage reports

**Why:** Users on OpenRouter with multiple models need per-model cost visibility.

**Size:** S
**Files:** `db.rs`, `agent.rs`, `llm.rs`

### 4. In-Channel Commands

- [ ] Intercept `/` prefixed messages in gateway before agent dispatch
- [ ] Implement: `/status`, `/new`, `/reset`, `/compact`, `/usage`
- [ ] Wire command responses for native Telegram/Slack/Discord handlers
- [ ] Expose session info queries in DB for `/status` and `/usage`

**Why:** Users interacting via Telegram/Slack need session management without CLI access.

**Size:** M
**Files:** `gateway/handler.rs`, `gateway/server.rs`, `db.rs`
**Depends on:** #3 (for `/usage`)

### 5. Tool-Level Execution Approval

- [ ] Add `tool_auto_approve` and `tool_deny` lists to `ExecutionPolicy`
- [ ] Check tool-level policy before every tool dispatch (not just shell/patch)
- [ ] Add `[policy]` tool-level config section
- [ ] Default: auto-approve read-only tools (read_memory, list_tools, list_skills, list_channels)

**Why:** Current HITL only covers 2 operations. Users need granular per-tool control.

**Size:** M
**Files:** `policy.rs`, `agent.rs`, `config.rs`

---

## P1 — Important, Not Blocking 85%

### 6. Image Compression + Vision Pipeline

- [ ] Image compression/resize before LLM (use `image` crate)
- [ ] Auto-detect image parts and ensure model supports vision
- [ ] Process gateway `InboundAttachment` through compression

**Size:** M
**Files:** `media.rs`, `llm.rs`, `gateway/handler.rs`

<!--
### 7. Generalized Channel Sender Authentication

- [ ] Gateway-level allowlist/blocklist per channel
- [ ] Pairing code flow for unknown senders
- [ ] DB table for paired senders
- [ ] `[gateway.auth]` config section

**Size:** M
**Files:** `gateway/handler.rs`, `gateway/server.rs`, `config.rs`, `db.rs`
**Depends on:** #4 (for `/pair <code>` flow) -->

### 8. Config Hot Reload

- [ ] `Arc<RwLock<Config>>` pattern for shared config
- [ ] File watcher (`notify` crate) on `config.toml`
- [ ] Check for updates between agent turns
- [ ] Apply changes without restart

**Size:** M
**Files:** `config.rs`, `agent.rs`, `cli/main.rs`

### 9. Slack Typing Indicators

- [ ] Add typing indicator API call to Slack module
- [ ] Send typing before Slack agent processing (match Telegram behavior)

**Size:** S
**Files:** `gateway/slack/api.rs`, `gateway/server.rs`

---

## P2 — Nice to Have

### 10. Audio Transcription

- [ ] Transcribe voice messages (Telegram/WhatsApp) via Whisper or provider API
- [ ] Inject transcript as text before agent processing

**Size:** M. **Depends on:** #6

### 11. Provider-Level Failover

- [ ] Fall back to entirely different provider when one is down (not just key rotation)
- [ ] Configurable failover chain in `[llm]`

**Size:** L

### 12. Multi-Agent Gateway Routing

- [ ] Route different channels to different agent configs
- [ ] Separate memory, personality, model per channel binding

**Size:** L. **Depends on:** existing multi-agent system + #8

---

## Implementation Sequence

```
Phase 1 (parallel):  #3 Token Usage (S) + #5 Tool Approval (M) + #9 Slack Typing (S)
Phase 2 (parallel):  #1 Semantic Memory (L) + #2 Browser (L) + #4 Channel Commands (M)
Phase 3 (P1):        #6 Vision Pipeline (M) + #7 Channel Auth (M) + #8 Hot Reload (M)
Phase 4 (P2):        #10 Audio + #11 Provider Failover + #12 Multi-Agent Routing
```

## Out of Scope (Bells & Whistles)

Not pursuing parity on: Canvas/A2UI, voice wake, mobile/desktop companion apps, ACP protocol, Tailscale integration, VNC browser viewing, WebChat control UI, video processing, Bonjour device discovery.
