# Borg Feature Spec 03: Evolution System

## Goal

Add an evolution system so Borg feels like it is becoming uniquely shaped by the user's real behavior.

Evolution should be based on:

- actual usage patterns
- vitals health
- bond/trust development
- connected tools/integrations
- domain specialization signals

This feature depends on:

1. vitals system (spec 01)
2. bond system (spec 02)

Evolution is behavioral specialization, not raw XP.

## Product Requirements

### User outcomes

- Users should feel: "this Borg is mine."
- Evolution archetypes should reflect how the user actually uses the product.
- CLI/TUI should clearly show current stage, level, archetype progress, and gate requirements.

### UX constraints

- Avoid generic gamified jargon.
- Use themed terms: stage, level, archetype, evolution.
- Allow deterministic classification based on tool usage patterns.

## Core Model

### Stages

Three permanent stages with Lvl.0–99 per stage:

```rust
pub enum Stage {
    Base,     // Stage 1 — no specialization
    Evolved,  // Stage 2 — emerging specialization
    Final,    // Stage 3 — mastery
}
```

### Archetypes

Ten archetypes classify usage patterns:

```rust
pub enum Archetype {
    Ops,           // DevOps, SRE, infrastructure, CI/CD
    Builder,       // Tool creation, automation, coding
    Analyst,       // Research, data, metrics, reporting
    Communicator,  // Messaging, email, outreach
    Guardian,      // Security, compliance, monitoring
    Strategist,    // Planning, decision-making
    Creator,       // Content, writing, marketing
    Caretaker,     // Home, wellness, personal management
    Merchant,      // E-commerce, sales, finance
    Tinkerer,      // Hardware, homelab, experimentation
}
```

### Evolution state

```rust
pub struct EvolutionState {
    pub stage: Stage,
    pub level: u8,                    // 0-99
    pub total_xp: u32,
    pub xp_to_next_level: u32,
    pub dominant_archetype: Option<Archetype>,
    pub evolution_name: Option<String>,
    pub evolution_description: Option<String>,
    pub archetype_scores: HashMap<Archetype, u32>,
    pub total_events: u32,
    pub chain_valid: bool,
}
```

### Evolution event

```rust
pub struct EvolutionEvent {
    pub id: i64,
    pub event_type: String,           // xp_gain, evolution, classification, archetype_shift
    pub xp_delta: i32,
    pub archetype: Option<String>,
    pub source: String,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub hmac: String,
    pub prev_hmac: String,
}
```

## Persistence (Event-Sourced)

State is **never stored directly**. The `evolution_events` table is the single source of truth. Current state is computed by replaying all verified events from baseline.

### Anti-tamper: HMAC chain

Each event carries an HMAC-SHA256 signature using the shared `hmac_chain` module. Fields are concatenated directly (no separators, for backward compatibility). Domain: `borg-evolution-chain-v1`. During replay, events with broken chains are skipped.

### Anti-gaming: Rate limiting

During replay, events are capped per hour:
- xp_gain: 30/hr, evolution: 3/hr, classification: 3/hr, archetype_shift: 5/hr
- Per-source: 10/hr (prevents one tool from dominating)

### DB Migration (V24)

#### `evolution_events`
Append-only ledger — the single source of truth.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PRIMARY KEY AUTOINCREMENT | |
| event_type | TEXT NOT NULL | xp_gain, evolution, classification, archetype_shift |
| xp_delta | INTEGER NOT NULL | XP gained (0 for non-XP events) |
| archetype | TEXT | Which archetype relates to event |
| source | TEXT NOT NULL | Tool name, "session_start", etc. |
| metadata_json | TEXT | Classification results, evolution details |
| created_at | INTEGER NOT NULL | Unix timestamp |
| hmac | TEXT NOT NULL | HMAC-SHA256 signature |
| prev_hmac | TEXT NOT NULL DEFAULT '0' | Chain link |

Indexes on `created_at` and `archetype`.

V25 adds append-only triggers (prevent UPDATE/DELETE).

## XP System

### XP awards per action

| Action | Base XP | Archetype Bonus |
|--------|---------|-----------------|
| Tool call (success) | +1 | +2 if aligned |
| Creation event | +3 | +3 if aligned |
| Session interaction | +1 | — |
| Tool failure | 0 | — |

### XP curve (WoW-style)

```
xp_for_level(stage, n) = stage_base + floor(n ^ stage_curve)
```

| Stage | Base | Curve | Target Duration |
|-------|------|-------|-----------------|
| 1 (Base) | 2 | 1.0 (linear) | 2-5 days |
| 2 (Evolved) | 8 | 1.2 | ~30 days |
| 3 (Final) | 20 | 1.5 | 6-12 months |

## Archetype Classification

### Deterministic (tool name)

| Tool | Archetype |
|------|-----------|
| create_tool, apply_patch, apply_skill_patch, create_channel | Builder |
| security_audit | Guardian |
| browser, search, read_pdf, memory_search | Analyst |
| calendar, notion, linear, manage_tasks | Strategist |
| gmail, outlook | Communicator |
| write_memory | Creator |
| Channel names containing telegram/slack/discord/etc. | Communicator |
| Integration names containing docker/git/database | Ops |

### Shell command keyword scanning

`run_shell` commands are classified by keyword matching:
- **Ops**: deploy, kubernetes, docker, terraform, kubectl, etc.
- **Builder**: cargo, npm, pip, gcc, build, compile, etc.
- **Analyst**: query, select, analyze, csv, psql, etc.
- **Guardian**: firewall, ufw, nmap, chmod, audit, etc.
- **Strategist**: plan, prioritize, compare, evaluate, etc.
- **Tinkerer**: homelab, proxmox, pihole, wireguard, raspberry, etc.

## Evolution Gates

### Stage 1→2 (Base → Evolved)

All must pass:
1. Level = 99 at Stage 1
2. Bond score >= 30
3. Dominant archetype >= 1.3x runner-up score
4. Minimum vital (min of all 5 stats) >= 20

### Stage 2→3 (Evolved → Final)

All must pass:
1. Level = 99 at Stage 2
2. Bond score >= 55
3. Correction rate < 20% (last 14 days)
4. Dominant archetype stable for 14+ consecutive days

When gates pass, an evolution event is recorded with `{"gates_verified": true}` in metadata. During replay, evolution events without this flag are rejected.

## CLI / TUI

### `borg status` (includes evolution section)

Shows current stage, level, description, stage progress bar, XP progress.

### `borg status archetypes`

Shows archetype score breakdown with bar chart for all 10 archetypes.

### `borg status history`

Shows evolution history timeline (stage transitions with dates and names).

### TUI session header

Compact one-liner: `[Pipeline Warden Lvl.42 | Ops]`

## System prompt injection

Compact XML context injected at BeforeAgentStart and BeforeLlmCall:

```xml
<evolution_context>
Stage: Evolved | Pipeline Warden Lvl.42
Archetype: Ops (score: 74)
</evolution_context>
```

## Configuration

`evolution.enabled` (boolean, default: true) — can be toggled via `borg settings`.

When disabled, the EvolutionHook is not registered and no XP events are recorded.

## Hook integration

`EvolutionHook` implements the `Hook` trait:
- **SessionStart**: Record interaction XP
- **BeforeAgentStart**: InjectContext (evolution context)
- **BeforeLlmCall**: InjectContext (evolution context)
- **AfterToolCall**: Classify tool, record XP, attempt evolution

Registered AFTER VitalsHook and BondHook in CLI and TUI.

## Architecture

### Core module
- `crates/core/src/evolution.rs` — all types, scoring, classification, gates, HMAC chain, formatting, EvolutionHook
- `crates/core/src/hmac_chain.rs` — shared HMAC builder, chain verification, rate limiting (used by vitals, bond, evolution)

### Extensibility
- Bond system is queried for gate checks
- Vitals system is queried for gate checks (correction rate, min vital)
- Event types are extensible without schema changes
- HMAC chain provides audit trail

## Acceptance Criteria

- Evolution state persists in DB via event-sourced ledger.
- `borg status` shows evolution section with stage, level, archetype.
- `borg status archetypes` shows archetype score breakdown.
- `borg status history` shows evolution timeline.
- Base → Evolved → Final progression works with gate checks.
- Archetype classification is deterministic from tool usage.
- HMAC chain detects tampered events.
- Rate limiting prevents gaming.
- Evolution gates respect bond score and vitals health.
- 43+ unit tests pass.
- No safety regressions — autonomy is informational only.

## Implementation Notes

- Build deterministic first — no LLM classifier needed.
- Keep scoring auditable and debuggable.
- Evolution should be hard to game with shallow usage (rate limiting enforces this).
- Autonomy concept is owned by the bond system, not evolution.
- HMAC chain uses shared `hmac_chain` module (no field separators for backward compatibility).
