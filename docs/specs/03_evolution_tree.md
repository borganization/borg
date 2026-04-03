# Borg Feature Spec 03: Evolution Tree

## Goal

Add an evolution system so Borg feels like it is becoming uniquely shaped by the user’s real behavior.

Evolution should be based on:

- actual usage patterns
- care loop health
- bond/trust development
- connected tools/integrations
- routines and workflows the user uses
- domain specialization signals

This feature depends on:

1. care loop
2. bond system

Evolution is not raw XP. It is behavioral specialization.

## Product Requirements

### User outcomes

- Users should feel: “this Borg is mine.”
- Evolution paths should reflect how the user actually uses the product.
- Unlocks should feel meaningful: routines, modules, shells/themes, prompts, specialist behavior.
- CLI/TUI should clearly show current form, path progress, and unlock requirements.

### UX constraints

- Avoid generic gamified jargon.
- Use themed terms: form, path, specialization, shell, module, evolution.
- Do not force the user to pick too early.
- Allow deterministic paths for obvious cases and LLM-assisted classification for ambiguous/custom workflows.

## Core Model

### Forms

```rust
pub enum EvolutionForm {
    SeedUnit,
    LinkedUnit,
    SpecializedUnit(SpecializationPath),
    EliteForm(SpecializationPath),
}
```

### Paths

```rust
pub enum SpecializationPath {
    Ops,
    Family,
    Builder,
    Strategist,
    Wellness,
    Scout,
}
```

### Evolution state

```rust
pub struct EvolutionState {
    pub current_form: EvolutionForm,
    pub active_path: Option<SpecializationPath>,
    pub path_scores: std::collections::HashMap<SpecializationPath, u32>,
    pub unlocked_modules: Vec<String>,
    pub unlocked_shells: Vec<String>,
    pub unlocked_routines: Vec<String>,
    pub unlocked_traits: Vec<String>,
    pub evolution_history: Vec<EvolutionEvent>,
    pub last_evaluated_at: DateTime<Utc>,
}
```

### Evolution event

```rust
pub struct EvolutionEvent {
    pub timestamp: DateTime<Utc>,
    pub from_form: String,
    pub to_form: String,
    pub reason: String,
}
```

## Persistence

Add DB migration.

### New tables

#### `evolution_state`

Singleton row.

Columns:

- `id INTEGER PRIMARY KEY CHECK (id = 1)`
- `current_form TEXT NOT NULL`
- `active_path TEXT NULL`
- `last_evaluated_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`

#### `evolution_path_scores`

Columns:

- `path TEXT PRIMARY KEY`
- `score INTEGER NOT NULL`
- `updated_at TEXT NOT NULL`

#### `evolution_unlocks`

Columns:

- `id INTEGER PRIMARY KEY`
- `kind TEXT NOT NULL` // module | shell | routine | trait
- `key TEXT NOT NULL`
- `unlocked_at TEXT NOT NULL`
- `metadata_json TEXT NULL`

#### `evolution_events`

Columns:

- `id INTEGER PRIMARY KEY`
- `timestamp TEXT NOT NULL`
- `from_form TEXT NOT NULL`
- `to_form TEXT NOT NULL`
- `reason TEXT NOT NULL`
- `metadata_json TEXT NULL`

#### `workflow_classifications`

For non-obvious custom workflows.

Columns:

- `id INTEGER PRIMARY KEY`
- `workflow_key TEXT NOT NULL`
- `workflow_name TEXT NOT NULL`
- `source TEXT NOT NULL` // tool | routine | skill | inferred
- `classification_json TEXT NOT NULL`
- `classifier_version TEXT NOT NULL`
- `confidence REAL NOT NULL`
- `classified_at TEXT NOT NULL`

## Evolution Forms and Gates

### Seed Unit

Default.

Capabilities:

- answers questions
- stores a few preferences
- simple reminders
- beginner missions

### Linked Unit

Unlock when all are true:

- at least 3 tools/integrations/channels/skills connected in meaningful use
- first 5 missions completed (or equivalent MVP completion events)
- at least 10 preferences/memory teachings

Unlocks:

- daily briefing
- cross-app suggestions
- saved routines

### Specialized Unit

Eligible when:

- current form is `LinkedUnit`
- one path score clearly leads
- bond score >= 55
- sync average over last 14 days >= threshold
- at least 1 active routine or workflow in the dominant path

Paths:

- Ops
- Family
- Builder
- Strategist
- Wellness
- Scout

Unlocks:

- path-specific shell
- stronger modules
- path-specific mission suggestions
- more proactive domain recommendations

### Elite Form

Eligible when:

- current form is `SpecializedUnit`
- 30-day sync consistency passes threshold
- at least 3 active automations/routines
- bond score >= 75
- domain mastery criteria met for active path

Unlocks:

- rare workflows
- advanced automation chains
- prestige shell
- specialist sub-agent scaffolding hooks for future work

## Deterministic Path Signals

Compute path scores from real usage.

### Ops

Signals:

- calendar usage
- meeting prep
- reminders
- recurring checklists
- scheduling routines
- daily/weekly briefings

### Family

Signals:

- shopping workflows
- family reminders
- shared calendar usage
- meal planning
- household routines

### Builder

Signals:

- custom tools created
- apply_patch usage
- skill creation
- multi-step automations
- workflow chaining
- code/project-local memory usage

### Strategist

Signals:

- planning sessions
- prioritization notes
- summaries/comparisons
- decision-support queries
- weekly calibrations

### Wellness

Signals:

- health routines
- habit check-ins
- sleep/exercise/meal planning workflows
- reminder consistency around wellness domains

### Scout

Signals:

- research workflows
- comparisons
- discovery queries
- travel planning
- investigation-heavy sessions

## Workflow Classification for Ambiguous Cases

Some user-created workflows will not map deterministically:

- grocery buy flow
- Amazon cart optimizer
- YouTube review comment workflow
- custom shopping/research/admin automations

For these, add an optional LLM-assisted classifier.

### Requirements

- Use deterministic scoring first.
- Only invoke LLM classifier for unknown/ambiguous workflows.
- Cache results in `workflow_classifications`.
- Include confidence score.
- Keep cost bounded.

### Classifier input

Provide:

- workflow/tool/skill name
- description
- parameter schema if available
- recent usage context summary
- examples of when it runs

### Classifier output

Structured JSON like:

```json
{
    "primary_path": "Family",
    "secondary_paths": ["Ops"],
    "tags": ["shopping", "household", "routine"],
    "confidence": 0.82,
    "reason": "This workflow supports household logistics and recurring family shopping."
}
```

### Safety/robustness

- Never let one low-confidence classification force evolution by itself.
- Use classifications as weighted path score inputs, not sole authority.
- Reclassify only when workflow definition changes materially.

## Path Score Model

Use weighted additive scoring.

Example:

- successful calendar routine => Ops +4
- create custom tool => Builder +5
- weekly planning calibration => Strategist +4
- shopping routine success => Family +4
- research session => Scout +3

Use rolling windows:

- 30-day primary score
- lifetime score
- recent momentum bonus

Suggested formula:
`effective_score = lifetime * 0.35 + last_30d * 0.65`

This lets current behavior steer the form.

## CLI / TUI Requirements

Add commands:

- `borg evolve`
- `borg evolve status`
- `borg evolve paths`
- `borg evolve review`
- `borg evolve classify <workflow>`

### `borg evolve`

Show:

- current form
- active path
- next unlock requirements
- path score leaderboard
- recently unlocked modules/routines/shells

Example:

```text
Evolution Status
Current Form     SpecializedUnit(Builder)
Active Path      Builder

Path Scores
Builder          74
Strategist       41
Ops              33
Scout            27
Family           12
Wellness         8

Next Evolution
Elite Builder Form
Requirements:
- Bond score 75+      [68]
- 30-day sync         [24/30 good days]
- Active automations  [2/3]
- Builder mastery     [in progress]

Recent Unlocks
- Module: Workflow Forge
- Routine: Patch Review Chain
- Shell: Builder Frame I
```

### `borg evolve review`

Show why the current path was chosen:

- top signal sources
- recent classified workflows
- bond/care gating blockers

### `borg evolve classify <workflow>`

Manual maintenance command:

- inspect cached classification
- optionally re-run classifier if requested
- useful for debugging ambiguous paths

## Unlock System

Define unlock metadata in code or embedded data.

### Unlock kinds

- modules
- shells
- routines
- traits

Examples:

- `daily-briefing`
- `cross-app-suggestions`
- `saved-routines`
- `builder-frame-i`
- `family-ops-pack`
- `rare-weekly-planning-engine`

Keep unlock application deterministic and idempotent.

## Integration Points

### Care loop dependencies

Evolution should read:

- average sync
- cleanliness
- stability
- mission completions if added there

### Bond dependencies

Evolution should read:

- bond score
- bond level
- routine success rates
- preference learning counts

### Customizations / marketplace

Evolution should be able to consider:

- installed templates from `customizations`
- connected channels/integrations
- active tools/skills

### Agent prompt context

Inject a tiny evolution summary:

- current form
- active path
- unlocked traits/modules relevant to behavior

Do not bloat tokens.

## Evaluation Cadence

Recompute evolution:

- on startup
- after meaningful sessions
- after workflow/routine creation
- after tool/integration install
- after bond milestone changes
- after care milestone changes

Avoid recomputing every turn if expensive.

## Acceptance Criteria

- Evolution state persists in DB.
- `borg evolve` and related status commands work.
- Seed -> Linked progression works deterministically.
- Specialized path selection works from real usage signals.
- Elite gating respects care + bond + routine criteria.
- Ambiguous workflows can be classified with optional LLM help and cached.
- Unlocks are visible in CLI/TUI output.
- The system is explainable: user can inspect why a path/form was chosen.

## Implementation Notes

- Build deterministic first, classifier second.
- Keep scoring auditable and debuggable.
- Cache all expensive classifications.
- Do not force user-facing path choice unless tie-breaking is needed.
- Expose enough reasoning in CLI so the system does not feel arbitrary.
- Structure code so future shell/avatar/cosmetic UI can consume the same unlock model.
