# Borg Feature Spec 02: Bond System

## Goal

Add a functional trust/bond system on top of the vitals system.

Bond is not friendship fluff. It represents operational trust between the user and Borg:

- how often Borg gets things right
- how often the user accepts or rejects suggestions
- whether routines complete reliably
- whether Borg learns user preferences successfully over time

Bond should:

- be persistent and tamper-resistant (HMAC chain verification)
- rise and fall based on real usage outcomes
- unlock more proactive and autonomous behavior later
- feed the evolution system
- be event-sourced — no singleton state table, score replayed from verified events

This feature depends on the vitals system (spec 01) and consumes the `vitals_events` ledger for signals.

## Product Requirements

### User outcomes

- Users should feel Borg is becoming more aligned with them.
- Corrections should feel like training, not just friction.
- Bond should reflect real trust, not vanity metrics.
- High bond should visibly change how Borg behaves.

### UX constraints

- Avoid sentimental language.
- Keep language framed around trust, alignment, reliability, and calibration.
- Surface actionable ways to improve bond.
- Do not punish users too harshly for non-use.

## Core Model

Bond is a score derived by replaying all verified events from a baseline.

```rust
pub struct BondState {
    pub score: u8,                   // 0..=100, replayed from events
    pub level: BondLevel,            // derived from score
    pub autonomy_tier: AutonomyTier, // derived from level
    pub total_events: u32,
    pub chain_valid: bool,           // HMAC chain verified during replay
}
```

### Bond levels

```rust
pub enum BondLevel {
    Fragile,      // 0-24
    Emerging,     // 25-44
    Stable,       // 45-64
    Trusted,      // 65-84
    Synced,       // 85-100
}
```

### Autonomy tiers

```rust
pub enum AutonomyTier {
    ObserveOnly,      // suggest only
    Recommend,        // stronger recommendation prompts
    DraftAssist,      // draft before approval
    GuidedAction,     // can chain low-risk routines with approval
    HighTrust,        // reserved for future gated autonomy
}
```

Maps 1:1 from BondLevel (Fragile → ObserveOnly, Emerging → Recommend, etc.).

High autonomy must still respect existing safety/HITL settings.

## Persistence

### Event-sourced design

No `bond_state` table. Score is always computed by replaying `bond_events` from a baseline of 40.

### DB Migration (V23)

#### `bond_events`

Append-only ledger with HMAC chain for tamper detection.

Columns:

- `id INTEGER PRIMARY KEY AUTOINCREMENT`
- `event_type TEXT NOT NULL` — tool_success, tool_failure, creation, correction, suggestion_accepted, suggestion_rejected
- `score_delta INTEGER NOT NULL` — signed delta
- `reason TEXT NOT NULL DEFAULT ''` — human-readable (tool name, etc.)
- `hmac TEXT NOT NULL` — HMAC-SHA256 of this event's content
- `prev_hmac TEXT NOT NULL DEFAULT ''` — HMAC of previous event (chain)
- `created_at INTEGER NOT NULL` — unix timestamp

Indexes on `created_at` and `event_type`.

### HMAC chain

Uses the shared `hmac_chain` module (`crates/core/src/hmac_chain.rs`). Fields are concatenated directly: `prev_hmac || event_type || score_delta || reason || created_at`. Key is derived per-installation via `db.derive_hmac_key()` with domain `borg-bond-chain-v1`.

Each event links to the previous via `prev_hmac`. On replay, the entire chain is verified. Tampered events are detected and flagged in `chain_valid`.

### Rate limiting

Prevents gaming by capping bond events per time window:

| Limit | Value |
|-------|-------|
| Total events per hour | 30 |
| Positive-delta events per hour | 15 |
| Per event_type per hour | 5-15 (type-specific) |

Events exceeding limits are silently dropped.

## Event Inputs

Bond consumes vitals events and adds bond-specific signals:

### From vitals (via `vitals::classify_tool()`)
- Tool success → `tool_success` (+1)
- Tool failure → `tool_failure` (-1)
- Creation event → `creation` (+2)

### From user message heuristic
- Suggestion accepted (heuristic) → `suggestion_accepted` (+1)
- Suggestion rejected (heuristic) → `suggestion_rejected` (-1)
- Correction detected → `correction` (-2) — reuses vitals correction detection

### From scheduled tasks (via `task_runs` table)
- Routine success rate computed on demand from `task_runs` (30d window)
- Not stored as bond events — queried live for display

### Suggestion heuristic

Heuristic-only (no explicit `/accept`/`/reject` commands):

**Acceptance patterns** (regex, case-insensitive):
`\b(do it|go ahead|sounds good|approved|let's do it|yes, do|yes please|absolutely)\b`

Note: Bare "yes" is intentionally excluded to reduce false positives from conversational filler. Multi-word phrases are preferred.

**Rejection patterns** (regex, case-insensitive):
`\b(no don't|don't do|cancel that|never mind|skip that|wrong suggestion|stop suggesting|not helpful)\b`

Distinct from the vitals `looks_like_correction` regex. Corrections = "you did something wrong." Rejections = "I don't want your suggestion."

## Scoring Rules

### Initial score

Baseline: `40` (`Emerging`). This is a compile-time constant, not stored.

### Positive deltas

| Event | Delta |
|-------|-------|
| Tool success | +1 |
| Creation event | +2 |
| Preference taught (creation via write_memory) | +2 |
| Suggestion accepted (heuristic) | +1 |

### Negative deltas

| Event | Delta |
|-------|-------|
| Tool failure | -1 |
| Correction detected | -2 |
| Suggestion rejected (heuristic) | -1 |

Score clamped to `0..=100` during replay.

## Rolling Metrics

Computed on demand (not stored), used for display and context injection:

- `routine_success_rate_30d` — from `task_runs` table
- `correction_rate_30d` — from `vitals_events` where category='correction'
- `preference_learning_count_30d` — from `vitals_events` where category='creation'

## Behavior Changes by Bond

Bond level is injected into the system prompt via the hook's `InjectContext`. The agent naturally adjusts behavior based on the context.

### System prompt injection

Compact XML block (~60 tokens):
```xml
<bond_context>
Bond: Trusted (68/100) | Autonomy: DraftAssist
Correction rate: 9% | Routine success: 83%
</bond_context>
```

Injected at `BeforeAgentStart` and `BeforeLlmCall` hook points.

### Low bond

- Fewer proactive suggestions
- Conservative memory reuse
- More explicit phrasing when unsure

### Medium bond

- Normal suggestion frequency
- Moderate memory reuse
- Suggested routines and drafts

### High bond

- Deeper reuse of memory context
- Stronger proactive recommendations
- Unlock more "draft this for me" style prompts
- Enable future evolution gates

Does not bypass safety/HITL.

## CLI / TUI

### `borg bond`

Show:

- Score (with bar)
- Level
- Autonomy tier
- Chain integrity status
- Rolling metrics (correction rate, routine success rate, preference count)
- Recent bond events (last 5)
- Recommended action

Example:

```text
Bond Status
───────────
  score        ██████░░░░  68
  level        Trusted
  autonomy     DraftAssist
  integrity    Chain valid (142 events)

30d Signals
  Routine Success Rate   83%
  Correction Rate        9%
  Preferences Learned    7

Recent Events
  tool_success    +1  run_shell           2m ago
  creation        +2  write_memory        15m ago
  correction      -2  user_message        1h ago
  tool_success    +1  read_file           1h ago
  suggestion_accepted +1  user_message    2h ago

Tip: Keep completing tasks successfully to strengthen trust
```

### `borg bond history`

Show last N bond events in tabular format.

## Architecture

### Core modules
- `crates/core/src/bond.rs` — all types, scoring, replay, heuristics, formatting, BondHook
- `crates/core/src/hmac_chain.rs` — shared HMAC builder, chain verification, rate limiting (used by vitals, bond, evolution)

### Hook integration
Uses the existing `Hook` trait. `BondHook` wraps Database in `Mutex<Database>` (same pattern as `VitalsHook`).

```rust
impl Hook for BondHook {
    fn name(&self) -> &str { "bond" }
    fn points(&self) -> &[HookPoint] {
        &[HookPoint::SessionStart, HookPoint::BeforeAgentStart,
          HookPoint::BeforeLlmCall, HookPoint::AfterToolCall]
    }
    fn execute(&self, ctx: &HookContext) -> HookAction {
        // SessionStart: no-op checkpoint
        // BeforeAgentStart: suggestion heuristic + InjectContext
        // BeforeLlmCall: InjectContext
        // AfterToolCall: score tool results
    }
}
```

Registered AFTER VitalsHook in:
- `crates/cli/src/tui/mod.rs`
- `crates/cli/src/repl.rs`

### Extensibility for future systems
- Evolution system can query `bond_events` for threshold checks
- Event types are extensible without schema changes
- HMAC chain provides audit trail

## Acceptance Criteria

- DB V23 migration creates `bond_events` table.
- Bond score computed by replaying verified HMAC chain from baseline.
- Rate limiting prevents >30 events/hour, >15 positive/hour, >10 same-type/hour.
- HMAC chain detects tampered events and flags in `chain_valid`.
- `borg bond` shows score, level, autonomy tier, chain integrity, rolling metrics.
- `borg bond history` shows recent events.
- Bond context injected into system prompt at BeforeAgentStart and BeforeLlmCall.
- Bond updates passively during normal usage via hooks.
- Unit + integration tests cover scoring, HMAC chain, rate limiting, heuristics.
- No safety regressions — autonomy tiers are informational, don't bypass HITL.

## Implementation Notes

- Keep scoring deterministic.
- Favor explainability over complexity.
- Bond should be hard to game with shallow usage (rate limiting enforces this).
- Use the shared `hmac_chain` module for HMAC computation and chain verification.
- Reuse `vitals::classify_tool()` for tool event categorization.
- Rolling metrics are live queries, not stored — prevents stale data.
- Hook always returns `Continue` except at BeforeAgentStart/BeforeLlmCall where it returns `InjectContext`.
