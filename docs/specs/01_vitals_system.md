# Borg Feature Spec 01: Vitals System

## Goal

Add a passive vitals system that tracks agent health through normal usage patterns. Five stats reflect the agent's operational state and update automatically via lifecycle hooks. No explicit maintenance commands required — the system rewards natural, productive usage.

This should feel like:
- A system status layer, not a game
- Visible in CLI + TUI
- Informational, not spammy
- Persistent across sessions
- Useful even without a GUI

The vitals system is the foundation for later bond and evolution systems.

## Product Requirements

### User outcomes
- Users see at a glance whether their agent is healthy and active.
- Stats reward productive behaviors naturally — no busywork.
- The system surfaces actionable drift warnings when something needs attention.

### UX constraints
- No childish hunger/sleep metaphors.
- No fake urgency or spammy notifications.
- Status text themed as "vitals", "sync", "drift", "stability" — like an operating system, not a toy.
- Everything works in REPL, one-shot CLI, and TUI.

## The 5 Vitals

All stats are integers from `0..=100`.

| Vital | What it measures | Goes up when | Goes down when |
|-------|-----------------|--------------|----------------|
| **stability** | Agent reliability and consistency | Tools succeed, sessions complete smoothly | Tools fail, corrections detected, errors |
| **focus** | Alignment with user intent | Successful interactions, creation events | Corrections, repeated failures |
| **sync** | How in-touch agent and user are | Regular interaction, sessions | Inactivity (strongest time decay) |
| **growth** | Capability expansion and learning | Memory writes, tool/skill creation, new integrations | Inactivity, stagnation |
| **charge** | Momentum and energy | Any meaningful usage, creation events | Inactivity |

```rust
pub struct VitalsState {
    pub stability: u8,
    pub focus: u8,
    pub sync: u8,
    pub growth: u8,
    pub charge: u8,
    pub last_interaction_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### Baseline initialization
On first run:
- stability = 80
- focus = 60
- sync = 55
- growth = 70
- charge = 65

## Event System

Events are classified into broad **categories** rather than hardcoded action types. This means new tools, skills, and integrations automatically get tracked without code changes.

### Event categories

```rust
pub enum EventCategory {
    Interaction,   // any user engagement
    Success,       // any tool/action completed without error
    Failure,       // any tool/action errored
    Correction,    // user corrected the agent (heuristic)
    Creation,      // user created something (tool, memory, skill, channel)
}
```

### Stat deltas per category

| Category | stability | focus | sync | growth | charge |
|----------|-----------|-------|------|--------|--------|
| Interaction | 0 | +1 | +2 | 0 | +1 |
| Success | +1 | +1 | 0 | 0 | +1 |
| Failure | -2 | -1 | 0 | -1 | 0 |
| Correction | -3 | -2 | -1 | 0 | -1 |
| Creation | +2 | +1 | +1 | +3 | +3 |

### Event classification

Events are classified at the hook level:

- **SessionStart** hook → `Interaction`
- **BeforeAgentStart** hook → `Interaction` (or `Correction` if message matches heuristic)
- **AfterToolCall** hook → classified by tool name and outcome:
  - `write_memory`, `create_tool`, `apply_skill_patch`, `create_channel`, `apply_patch` → `Creation` (if successful)
  - Any tool with `is_error: false` → `Success`
  - Any tool with `is_error: true` → `Failure`

### Correction detection heuristic

Regex-based pattern matching on user messages. Detects both polite corrections ("that's wrong", "try again") and frustration signals ("wtf", "this sucks", "so frustrating"). Word-boundary anchored to avoid false positives. Deliberately conservative — prefers false negatives.

## Decay Rules

Time-based decay applied when vitals are read (lazy evaluation):

| Inactivity threshold | Stats affected |
|---------------------|---------------|
| 24h+ | sync -6, charge -4 |
| 72h+ | additionally stability -8, focus -8 |
| 168h+ (7 days) | additionally growth -5, stability -5 |

All stats clamped `0..=100`.

## Drift Detection

Drift flags are derived from current state:

```rust
pub enum DriftFlag {
    InactiveTooLong,       // >48h no interaction
    LowStability,          // stability < 30
    LowSync,               // sync < 40
    LowCharge,             // charge < 30
    RepeatedFailures,      // 3+ failures in recent events
}
```

## Persistence (Event-Sourced)

State is **never stored directly**. The `vitals_events` table is the single source of truth. Current vitals are computed by replaying all verified events from baseline.

### Anti-tamper: HMAC chain

Each event carries an HMAC-SHA256 signature using the shared `hmac_chain` module. Fields are concatenated directly: `prev_hmac || category || source || deltas || timestamp`. Key is derived per-installation via `db.derive_hmac_key()` with domain `borg-vitals-chain-v1`. During replay, events with broken chains are skipped.

### Anti-gaming: Rate limiting

During replay, each event category is capped per hour:
- interaction: 10/hr, success: 15/hr, failure: 10/hr, correction: 5/hr, creation: 5/hr

Events beyond the cap are ignored, so bulk-inserting events just hits the ceiling.

### DB V22 migration

#### `vitals_events`
Append-only ledger — the single source of truth.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PRIMARY KEY AUTOINCREMENT | |
| category | TEXT NOT NULL | interaction, success, failure, correction, creation |
| source | TEXT NOT NULL | Tool name, "session_start", "user_message", etc. |
| stability_delta | INTEGER NOT NULL DEFAULT 0 | |
| focus_delta | INTEGER NOT NULL DEFAULT 0 | |
| sync_delta | INTEGER NOT NULL DEFAULT 0 | |
| growth_delta | INTEGER NOT NULL DEFAULT 0 | |
| charge_delta | INTEGER NOT NULL DEFAULT 0 | |
| metadata_json | TEXT | Optional context |
| created_at | INTEGER NOT NULL | Unix timestamp |
| hmac | TEXT NOT NULL | HMAC-SHA256 of event data chained from previous |
| prev_hmac | TEXT NOT NULL DEFAULT '0' | HMAC of previous event (chain link) |

Index on `created_at` for trend queries.

## CLI / TUI

### `borg status` (CLI) and `/status` (TUI)

Single command surface. Output:

```text
Borg Vitals
───────────
  stability    ████████░░  80
  focus        ██████░░░░  60
  sync         █████░░░░░  55
  growth       ███████░░░  70
  charge       ██████░░░░  65

Recent Activity (7d):
  12 interactions, 8 successes, 1 failure, 2 creations

Drift:
  ⚠ Daily sync overdue

Tip: Use borg regularly to keep vitals healthy
```

### TUI session header

On session start, show compact one-liner:
```text
[stability:80 focus:60 sync:55 growth:70 charge:65]
```

Plus optional drift notice (max one per session).

## Architecture

### Core modules
- `crates/core/src/vitals.rs` — all types, scoring, decay, drift, formatting, VitalsHook
- `crates/core/src/hmac_chain.rs` — shared HMAC builder, chain verification, rate limiting (used by vitals, bond, evolution)

### Hook integration
Uses the existing `Hook` trait. `VitalsHook` opens its own DB connection (hooks are `Send + Sync`).

```rust
impl Hook for VitalsHook {
    fn name(&self) -> &str { "vitals" }
    fn points(&self) -> &[HookPoint] {
        &[HookPoint::SessionStart, HookPoint::BeforeAgentStart, HookPoint::AfterToolCall]
    }
    fn execute(&self, ctx: &HookContext) -> HookAction {
        // Classify event → compute deltas → record in DB
        // Always returns HookAction::Continue (purely observational)
    }
}
```

Registered in:
- `crates/cli/src/tui/mod.rs` — after `Agent::new()`, before `Arc::new(Mutex::new(agent))`
- `crates/cli/src/repl.rs` — after `Agent::new()` in `one_shot()`

### Extensibility for future systems
- Bond system can subscribe to the same `vitals_events` ledger
- Evolution system can replay events and query computed state
- Event categories are broad enough to absorb new tool types without changes

## Acceptance Criteria

- DB V22 migration creates `vitals_events` table with HMAC columns.
- Vitals update passively during normal usage via hooks.
- State is event-sourced — computed by replaying verified events from baseline.
- Tampered events (broken HMAC chain) are skipped during replay.
- Rate limiting caps per-category-per-hour impact during replay.
- `borg status` and `/status` display current vitals with bars and drift warnings.
- TUI shows compact vitals header on session start.
- Decay applies correctly based on inactivity duration.
- 35+ unit + integration tests pass.
- No performance impact on agent loop (hook is fire-and-forget, logs errors).

## Implementation Notes

- Prefer deterministic scoring over LLM scoring.
- Keep all text themed as "vitals", "sync", "drift", "stability", "growth".
- Avoid cute pet language.
- Hook never returns `InjectContext` or `Skip` — purely observational.
- Decay is lazy — applied when vitals are read, not on a timer.
- No mutable state table — event ledger is the single source of truth.
