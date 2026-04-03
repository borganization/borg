//! Vitals system — passive agent health tracking via lifecycle hooks.
//!
//! Five stats (stability, focus, sync, growth, charge) update automatically
//! from usage events. State is event-sourced: derived by replaying verified
//! events from baseline. HMAC chain prevents tampering; rate limiting caps
//! impact per category per hour to prevent gaming.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use regex::Regex;
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt;
use std::sync::LazyLock;

use crate::db::Database;
use crate::hooks::{Hook, HookAction, HookContext, HookData, HookPoint};

// ── HMAC ──

/// Domain string for HMAC key derivation. Combined with per-installation salt.
pub(crate) const VITALS_HMAC_DOMAIN: &[u8] = b"borg-vitals-chain-v1";

/// Legacy compiled-in secret for installations without per-install salt.
const VITALS_HMAC_LEGACY: &[u8] = b"borg-vitals-chain-v1";

type HmacSha256 = Hmac<Sha256>;

/// Compute HMAC for an event, chaining from the previous event's HMAC.
pub(crate) fn compute_event_hmac(
    key: &[u8],
    prev_hmac: &str,
    category: &str,
    source: &str,
    deltas: StatDeltas,
    created_at: i64,
) -> String {
    let mut mac = match HmacSha256::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return String::from("0"),
    };
    mac.update(prev_hmac.as_bytes());
    mac.update(category.as_bytes());
    mac.update(source.as_bytes());
    mac.update(&[
        deltas.stability as u8,
        deltas.focus as u8,
        deltas.sync as u8,
        deltas.growth as u8,
        deltas.charge as u8,
    ]);
    mac.update(&created_at.to_le_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .fold(String::new(), |mut s, b| {
            use std::fmt::Write;
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Verify an event's HMAC against the expected chain.
fn verify_event_hmac(key: &[u8], event: &VitalsEvent, expected_prev_hmac: &str) -> bool {
    if event.prev_hmac != expected_prev_hmac {
        return false;
    }
    let deltas = StatDeltas {
        stability: event.stability_delta as i8,
        focus: event.focus_delta as i8,
        sync: event.sync_delta as i8,
        growth: event.growth_delta as i8,
        charge: event.charge_delta as i8,
    };
    let expected = compute_event_hmac(
        key,
        &event.prev_hmac,
        &event.category,
        &event.source,
        deltas,
        event.created_at,
    );
    event.hmac == expected
}

// ── Rate Limiting ──

/// Maximum events per category per hour during replay.
pub(crate) fn rate_limit_for(category: &str) -> u32 {
    match category {
        "interaction" => 10,
        "success" => 15,
        "failure" => 10,
        "correction" => 5,
        "creation" => 5,
        _ => 5,
    }
}

// ── Types ──

/// The 5 vitals stats, all 0..=100.
#[derive(Debug, Clone)]
pub struct VitalsState {
    pub stability: u8,
    pub focus: u8,
    pub sync: u8,
    pub growth: u8,
    pub charge: u8,
    pub last_interaction_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub chain_valid: bool,
}

/// Broad impact categories for events.
/// New tools automatically get classified by the hook based on outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    Interaction,
    Success,
    Failure,
    Correction,
    Creation,
}

impl fmt::Display for EventCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Interaction => write!(f, "interaction"),
            Self::Success => write!(f, "success"),
            Self::Failure => write!(f, "failure"),
            Self::Correction => write!(f, "correction"),
            Self::Creation => write!(f, "creation"),
        }
    }
}

impl EventCategory {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "interaction" => Some(Self::Interaction),
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "correction" => Some(Self::Correction),
            "creation" => Some(Self::Creation),
            _ => None,
        }
    }
}

/// Stat deltas to apply from an event.
#[derive(Debug, Clone, Copy, Default)]
pub struct StatDeltas {
    pub stability: i8,
    pub focus: i8,
    pub sync: i8,
    pub growth: i8,
    pub charge: i8,
}

/// A recorded event from the ledger.
#[derive(Debug, Clone)]
pub struct VitalsEvent {
    pub id: i64,
    pub category: String,
    pub source: String,
    pub stability_delta: i32,
    pub focus_delta: i32,
    pub sync_delta: i32,
    pub growth_delta: i32,
    pub charge_delta: i32,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub hmac: String,
    pub prev_hmac: String,
}

/// Drift issues to surface to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftFlag {
    InactiveTooLong,
    LowStability,
    LowSync,
    LowCharge,
    RepeatedFailures,
}

impl DriftFlag {
    pub fn description(&self) -> &'static str {
        match self {
            Self::InactiveTooLong => "Agent inactive for over 48 hours",
            Self::LowStability => "Stability is critically low",
            Self::LowSync => "Sync is low — regular interaction helps",
            Self::LowCharge => "Charge is critically low",
            Self::RepeatedFailures => "Multiple recent tool failures detected",
        }
    }
}

/// Recommended action with human-readable text.
#[derive(Debug)]
pub struct Recommendation {
    pub reason: &'static str,
    pub tip: &'static str,
}

// ── Scoring ──

/// Baseline vitals for a fresh agent.
pub fn baseline() -> VitalsState {
    VitalsState {
        stability: 80,
        focus: 60,
        sync: 55,
        growth: 70,
        charge: 65,
        last_interaction_at: Utc::now(),
        updated_at: Utc::now(),
        chain_valid: true,
    }
}

/// Deterministic stat deltas for each event category.
pub fn deltas_for(category: EventCategory) -> StatDeltas {
    match category {
        EventCategory::Interaction => StatDeltas {
            stability: 0,
            focus: 1,
            sync: 2,
            growth: 0,
            charge: 1,
        },
        EventCategory::Success => StatDeltas {
            stability: 1,
            focus: 1,
            sync: 0,
            growth: 0,
            charge: 1,
        },
        EventCategory::Failure => StatDeltas {
            stability: -2,
            focus: -1,
            sync: 0,
            growth: -1,
            charge: 0,
        },
        EventCategory::Correction => StatDeltas {
            stability: -3,
            focus: -2,
            sync: -1,
            growth: 0,
            charge: -1,
        },
        EventCategory::Creation => StatDeltas {
            stability: 2,
            focus: 1,
            sync: 1,
            growth: 3,
            charge: 3,
        },
    }
}

/// Apply deltas to a mutable state, clamping each stat to 0..=100.
pub fn apply_deltas(state: &mut VitalsState, deltas: &StatDeltas) {
    state.stability = clamp_add(state.stability, deltas.stability);
    state.focus = clamp_add(state.focus, deltas.focus);
    state.sync = clamp_add(state.sync, deltas.sync);
    state.growth = clamp_add(state.growth, deltas.growth);
    state.charge = clamp_add(state.charge, deltas.charge);
}

fn clamp_add(val: u8, delta: i8) -> u8 {
    let result = val as i16 + delta as i16;
    result.clamp(0, 100) as u8
}

// ── Event Replay (Event Sourcing) ──

/// Replay verified events from baseline to compute current state.
/// Verifies HMAC chain and applies per-category-per-hour rate limits.
pub fn replay_events(events: &[VitalsEvent]) -> VitalsState {
    replay_events_with_key(VITALS_HMAC_LEGACY, events)
}

/// Replay events with a specific HMAC key (for per-installation derived keys).
pub fn replay_events_with_key(key: &[u8], events: &[VitalsEvent]) -> VitalsState {
    let mut state = baseline();
    let mut expected_prev_hmac = "0".to_string();
    let mut chain_valid = true;
    // Rate limit: (hour_bucket, category) -> count
    let mut hourly_counts: HashMap<(i64, &str), u32> = HashMap::new();
    let mut last_interaction_at: Option<i64> = None;

    for event in events {
        // Verify HMAC chain — skip tampered/injected events
        if !verify_event_hmac(key, event, &expected_prev_hmac) {
            tracing::warn!("vitals: skipping event {} with broken HMAC chain", event.id);
            chain_valid = false;
            continue;
        }
        expected_prev_hmac = event.hmac.clone();

        // Rate limit per category per hour
        let hour_bucket = event.created_at / 3600;
        let key = (hour_bucket, event.category.as_str());
        let count = hourly_counts.entry(key).or_insert(0);
        let cap = rate_limit_for(&event.category);
        if *count >= cap {
            continue; // Skip — hit rate limit ceiling
        }
        *count += 1;

        // Apply deltas
        let deltas = StatDeltas {
            stability: event.stability_delta as i8,
            focus: event.focus_delta as i8,
            sync: event.sync_delta as i8,
            growth: event.growth_delta as i8,
            charge: event.charge_delta as i8,
        };
        apply_deltas(&mut state, &deltas);

        // Track last interaction time (only for non-failure/correction events)
        if event.category != "failure" && event.category != "correction" {
            last_interaction_at = Some(event.created_at);
        }
    }

    // Set timestamps from replay
    if let Some(ts) = last_interaction_at {
        if let Some(dt) = DateTime::<Utc>::from_timestamp(ts, 0) {
            state.last_interaction_at = dt;
        }
    }
    if let Some(last_event) = events.last() {
        if let Some(dt) = DateTime::<Utc>::from_timestamp(last_event.created_at, 0) {
            state.updated_at = dt;
        }
    }

    state.chain_valid = chain_valid;
    state
}

/// Apply time-based decay based on inactivity duration.
pub fn apply_decay(state: &VitalsState, now: DateTime<Utc>) -> VitalsState {
    let mut result = state.clone();
    let hours = (now - state.last_interaction_at).num_hours();

    if hours >= 24 {
        result.sync = clamp_add(result.sync, -6);
        result.charge = clamp_add(result.charge, -4);
    }
    if hours >= 72 {
        result.stability = clamp_add(result.stability, -8);
        result.focus = clamp_add(result.focus, -8);
    }
    if hours >= 168 {
        result.growth = clamp_add(result.growth, -5);
        result.stability = clamp_add(result.stability, -5);
    }

    result
}

// ── Drift Detection ──

/// Detect drift flags from current state.
pub fn detect_drift(state: &VitalsState, now: DateTime<Utc>) -> Vec<DriftFlag> {
    let mut flags = Vec::new();
    let hours = (now - state.last_interaction_at).num_hours();

    if hours > 48 {
        flags.push(DriftFlag::InactiveTooLong);
    }
    if state.stability < 30 {
        flags.push(DriftFlag::LowStability);
    }
    if state.sync < 40 {
        flags.push(DriftFlag::LowSync);
    }
    if state.charge < 30 {
        flags.push(DriftFlag::LowCharge);
    }

    flags
}

/// Detect repeated failures from recent events.
pub fn detect_failure_drift(events: &[VitalsEvent]) -> bool {
    let failure_count = events.iter().filter(|e| e.category == "failure").count();
    failure_count >= 3
}

// ── Recommendations ──

/// Deterministic recommendation based on current vitals.
pub fn recommend(state: &VitalsState) -> Recommendation {
    if state.sync < 40 {
        return Recommendation {
            reason: "Sync is low",
            tip: "Have a conversation or start a task to restore sync",
        };
    }
    if state.charge < 30 {
        return Recommendation {
            reason: "Charge is low",
            tip: "Use borg for a meaningful task to restore charge",
        };
    }
    if state.stability < 30 {
        return Recommendation {
            reason: "Stability is low",
            tip: "Complete some successful tool operations to rebuild stability",
        };
    }
    if state.focus < 40 {
        return Recommendation {
            reason: "Focus is drifting",
            tip: "Clear, directed tasks help the agent stay focused",
        };
    }
    if state.growth < 40 {
        return Recommendation {
            reason: "Growth has stalled",
            tip: "Create tools, write memories, or install skills to grow capabilities",
        };
    }
    Recommendation {
        reason: "Vitals are healthy",
        tip: "Keep using borg regularly to maintain vitals",
    }
}

// ── Classification ──

/// Classify a tool call into an event category based on tool name and outcome.
pub fn classify_tool(tool_name: &str, is_error: bool) -> EventCategory {
    if is_error {
        return EventCategory::Failure;
    }
    match tool_name {
        "write_memory" | "create_tool" | "apply_skill_patch" | "create_channel" | "apply_patch" => {
            EventCategory::Creation
        }
        _ => EventCategory::Success,
    }
}

#[allow(clippy::expect_used)]
static CORRECTION_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(that'?s wrong|that'?s not right|not what i (meant|asked)|you misunderstood|incorrect|try again|don'?t do that|redo (this|that|it)|wtf|wth|what the (fuck|hell)|fuck(ing)? (broken|useless|terrible|awful|horrible)|this (sucks|is broken|is wrong)|so frustrating|damn it|are you (stupid|dumb|deaf))\b"
    ).expect("compile-time literal regex")
});

/// Regex-based heuristic for detecting user corrections and frustration.
/// Deliberately conservative — prefers false negatives over false positives.
pub fn looks_like_correction(msg: &str) -> bool {
    CORRECTION_PATTERN.is_match(msg)
}

// ── Formatting ──

fn bar(val: u8, width: usize) -> String {
    let filled = (val as usize * width + 50) / 100;
    let empty = width - filled;
    format!("{}{}", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty))
}

/// Compact one-liner for TUI session header.
pub fn format_compact(state: &VitalsState) -> String {
    format!(
        "[stability:{} focus:{} sync:{} growth:{} charge:{}]",
        state.stability, state.focus, state.sync, state.growth, state.charge
    )
}

/// Full status output for `borg status` / `/status`.
pub fn format_status(state: &VitalsState, events: &[VitalsEvent], drift: &[DriftFlag]) -> String {
    let mut out =
        String::from("Borg Vitals\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");

    out.push_str(&format!(
        "  stability    {}  {}\n",
        bar(state.stability, 10),
        state.stability
    ));
    out.push_str(&format!(
        "  focus        {}  {}\n",
        bar(state.focus, 10),
        state.focus
    ));
    out.push_str(&format!(
        "  sync         {}  {}\n",
        bar(state.sync, 10),
        state.sync
    ));
    out.push_str(&format!(
        "  growth       {}  {}\n",
        bar(state.growth, 10),
        state.growth
    ));
    out.push_str(&format!(
        "  charge       {}  {}\n",
        bar(state.charge, 10),
        state.charge
    ));

    if !events.is_empty() {
        let mut interactions = 0u32;
        let mut successes = 0u32;
        let mut failures = 0u32;
        let mut creations = 0u32;
        let mut corrections = 0u32;

        for e in events {
            match e.category.as_str() {
                "interaction" => interactions += 1,
                "success" => successes += 1,
                "failure" => failures += 1,
                "creation" => creations += 1,
                "correction" => corrections += 1,
                _ => {}
            }
        }

        out.push_str("\nRecent Activity (7d):\n");
        let mut parts = Vec::new();
        if interactions > 0 {
            parts.push(format!("{interactions} interactions"));
        }
        if successes > 0 {
            parts.push(format!("{successes} successes"));
        }
        if failures > 0 {
            parts.push(format!("{failures} failures"));
        }
        if creations > 0 {
            parts.push(format!("{creations} creations"));
        }
        if corrections > 0 {
            parts.push(format!("{corrections} corrections"));
        }
        out.push_str(&format!("  {}\n", parts.join(", ")));
    }

    if !drift.is_empty() {
        out.push_str("\nDrift:\n");
        for flag in drift {
            out.push_str(&format!("  \u{26a0} {}\n", flag.description()));
        }
    }

    let rec = recommend(state);
    out.push_str(&format!("\nTip: {}\n", rec.tip));

    out
}

/// One-line drift notice for TUI session start. Returns None if no drift.
pub fn format_drift_notice(drift: &[DriftFlag]) -> Option<String> {
    let first = drift.first()?;
    Some(format!("Drift: {}", first.description()))
}

// ── VitalsHook ──

/// Lifecycle hook that passively records vitals events.
/// Wraps Database in a Mutex because Hook requires Send + Sync
/// but rusqlite::Connection is !Sync.
pub struct VitalsHook {
    db: std::sync::Mutex<Database>,
}

impl VitalsHook {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            db: std::sync::Mutex::new(Database::open()?),
        })
    }

    fn record(&self, category: EventCategory, source: &str) {
        let deltas = deltas_for(category);
        let Ok(db) = self.db.lock() else {
            tracing::warn!("vitals: mutex poisoned, skipping event");
            return;
        };
        if let Err(e) = db.record_vitals_event(&category.to_string(), source, &deltas, None) {
            tracing::warn!("vitals: failed to record event: {e}");
        }
    }
}

impl Hook for VitalsHook {
    fn name(&self) -> &str {
        "vitals"
    }

    fn points(&self) -> &[HookPoint] {
        &[
            HookPoint::SessionStart,
            HookPoint::BeforeAgentStart,
            HookPoint::AfterToolCall,
        ]
    }

    fn execute(&self, ctx: &HookContext) -> HookAction {
        match &ctx.data {
            HookData::SessionStart { .. } => {
                self.record(EventCategory::Interaction, "session_start");
            }
            HookData::AgentStart { user_message } => {
                if looks_like_correction(user_message) {
                    self.record(EventCategory::Correction, "user_message");
                } else {
                    self.record(EventCategory::Interaction, "user_message");
                }
            }
            HookData::ToolResult { name, is_error, .. } => {
                let category = classify_tool(name, *is_error);
                self.record(category, name);
            }
            _ => {}
        }
        HookAction::Continue
    }
}

impl std::fmt::Debug for VitalsHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VitalsHook").finish()
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // ── Scoring ──

    #[test]
    fn test_baseline_values() {
        let state = baseline();
        assert_eq!(state.stability, 80);
        assert_eq!(state.focus, 60);
        assert_eq!(state.sync, 55);
        assert_eq!(state.growth, 70);
        assert_eq!(state.charge, 65);
    }

    #[test]
    fn test_deltas_for_all_categories() {
        let d = deltas_for(EventCategory::Interaction);
        assert_eq!(
            (d.stability, d.focus, d.sync, d.growth, d.charge),
            (0, 1, 2, 0, 1)
        );

        let d = deltas_for(EventCategory::Success);
        assert_eq!(
            (d.stability, d.focus, d.sync, d.growth, d.charge),
            (1, 1, 0, 0, 1)
        );

        let d = deltas_for(EventCategory::Failure);
        assert_eq!(
            (d.stability, d.focus, d.sync, d.growth, d.charge),
            (-2, -1, 0, -1, 0)
        );

        let d = deltas_for(EventCategory::Correction);
        assert_eq!(
            (d.stability, d.focus, d.sync, d.growth, d.charge),
            (-3, -2, -1, 0, -1)
        );

        let d = deltas_for(EventCategory::Creation);
        assert_eq!(
            (d.stability, d.focus, d.sync, d.growth, d.charge),
            (2, 1, 1, 3, 3)
        );
    }

    #[test]
    fn test_apply_deltas_normal() {
        let mut state = baseline();
        let deltas = deltas_for(EventCategory::Interaction);
        apply_deltas(&mut state, &deltas);
        assert_eq!(state.stability, 80);
        assert_eq!(state.focus, 61);
        assert_eq!(state.sync, 57);
        assert_eq!(state.growth, 70);
        assert_eq!(state.charge, 66);
    }

    #[test]
    fn test_apply_deltas_clamp_upper() {
        let mut state = baseline();
        state.stability = 99;
        state.growth = 98;
        let deltas = deltas_for(EventCategory::Creation);
        apply_deltas(&mut state, &deltas);
        assert_eq!(state.stability, 100);
        assert_eq!(state.growth, 100);
    }

    #[test]
    fn test_apply_deltas_clamp_lower() {
        let mut state = baseline();
        state.stability = 1;
        state.focus = 0;
        let deltas = deltas_for(EventCategory::Correction);
        apply_deltas(&mut state, &deltas);
        assert_eq!(state.stability, 0);
        assert_eq!(state.focus, 0);
    }

    #[test]
    fn test_classify_tool() {
        assert_eq!(
            classify_tool("write_memory", false),
            EventCategory::Creation
        );
        assert_eq!(classify_tool("create_tool", false), EventCategory::Creation);
        assert_eq!(
            classify_tool("apply_skill_patch", false),
            EventCategory::Creation
        );
        assert_eq!(
            classify_tool("create_channel", false),
            EventCategory::Creation
        );
        assert_eq!(classify_tool("apply_patch", false), EventCategory::Creation);
        assert_eq!(classify_tool("run_shell", false), EventCategory::Success);
        assert_eq!(classify_tool("read_file", false), EventCategory::Success);
        assert_eq!(classify_tool("write_memory", true), EventCategory::Failure);
        assert_eq!(classify_tool("run_shell", true), EventCategory::Failure);
    }

    // ── Decay ──

    #[test]
    fn test_no_decay_within_24h() {
        let state = baseline();
        let now = state.last_interaction_at + Duration::hours(23);
        let decayed = apply_decay(&state, now);
        assert_eq!(decayed.sync, state.sync);
        assert_eq!(decayed.charge, state.charge);
        assert_eq!(decayed.stability, state.stability);
        assert_eq!(decayed.focus, state.focus);
        assert_eq!(decayed.growth, state.growth);
    }

    #[test]
    fn test_decay_24h() {
        let state = baseline();
        let now = state.last_interaction_at + Duration::hours(25);
        let decayed = apply_decay(&state, now);
        assert_eq!(decayed.sync, 55 - 6);
        assert_eq!(decayed.charge, 65 - 4);
        assert_eq!(decayed.stability, 80);
        assert_eq!(decayed.focus, 60);
    }

    #[test]
    fn test_decay_72h() {
        let state = baseline();
        let now = state.last_interaction_at + Duration::hours(73);
        let decayed = apply_decay(&state, now);
        assert_eq!(decayed.sync, 55 - 6);
        assert_eq!(decayed.charge, 65 - 4);
        assert_eq!(decayed.stability, 80 - 8);
        assert_eq!(decayed.focus, 60 - 8);
    }

    #[test]
    fn test_decay_7_days() {
        let state = baseline();
        let now = state.last_interaction_at + Duration::hours(169);
        let decayed = apply_decay(&state, now);
        assert_eq!(decayed.sync, 55 - 6);
        assert_eq!(decayed.charge, 65 - 4);
        assert_eq!(decayed.stability, 80 - 8 - 5);
        assert_eq!(decayed.focus, 60 - 8);
        assert_eq!(decayed.growth, 70 - 5);
    }

    // ── Drift Detection ──

    #[test]
    fn test_drift_inactive() {
        let mut state = baseline();
        state.last_interaction_at = Utc::now() - Duration::hours(49);
        let drift = detect_drift(&state, Utc::now());
        assert!(drift.contains(&DriftFlag::InactiveTooLong));
    }

    #[test]
    fn test_drift_low_stats() {
        let mut state = baseline();
        state.stability = 25;
        state.sync = 35;
        state.charge = 20;
        let drift = detect_drift(&state, Utc::now());
        assert!(drift.contains(&DriftFlag::LowStability));
        assert!(drift.contains(&DriftFlag::LowSync));
        assert!(drift.contains(&DriftFlag::LowCharge));
    }

    #[test]
    fn test_no_drift_healthy() {
        let state = baseline();
        let drift = detect_drift(&state, Utc::now());
        assert!(drift.is_empty());
    }

    // ── Recommendations ──

    #[test]
    fn test_recommend_low_sync() {
        let mut state = baseline();
        state.sync = 35;
        let rec = recommend(&state);
        assert!(rec.reason.contains("Sync"));
    }

    #[test]
    fn test_recommend_low_charge() {
        let mut state = baseline();
        state.charge = 25;
        let rec = recommend(&state);
        assert!(rec.reason.contains("Charge"));
    }

    #[test]
    fn test_recommend_healthy() {
        let state = baseline();
        let rec = recommend(&state);
        assert!(rec.reason.contains("healthy"));
    }

    // ── Correction Heuristic ──

    #[test]
    fn test_correction_positive() {
        assert!(looks_like_correction("that's wrong"));
        assert!(looks_like_correction("That's not right at all"));
        assert!(looks_like_correction("Not what I meant, please fix"));
        assert!(looks_like_correction("incorrect, use the other file"));
        assert!(looks_like_correction("wtf is this output"));
        assert!(looks_like_correction("this sucks"));
        assert!(looks_like_correction("so frustrating"));
        assert!(looks_like_correction("try again please"));
    }

    #[test]
    fn test_correction_negative() {
        assert!(!looks_like_correction("sounds good, let's do that"));
        assert!(!looks_like_correction("yes, perfect"));
        assert!(!looks_like_correction("can you help me with this?"));
        assert!(!looks_like_correction("great work"));
        assert!(!looks_like_correction("I have no idea"));
        assert!(!looks_like_correction("there is no way to know"));
    }

    // ── Formatting ──

    #[test]
    fn test_format_compact() {
        let state = baseline();
        let compact = format_compact(&state);
        assert!(compact.contains("stability:80"));
        assert!(compact.contains("focus:60"));
        assert!(compact.contains("sync:55"));
        assert!(compact.contains("growth:70"));
        assert!(compact.contains("charge:65"));
    }

    #[test]
    fn test_format_status_structure() {
        let state = baseline();
        let drift = vec![DriftFlag::LowSync];
        let output = format_status(&state, &[], &drift);
        assert!(output.contains("Borg Vitals"));
        assert!(output.contains("stability"));
        assert!(output.contains("focus"));
        assert!(output.contains("sync"));
        assert!(output.contains("growth"));
        assert!(output.contains("charge"));
        assert!(output.contains("Drift:"));
        assert!(output.contains("Tip:"));
    }

    // ── Failure Drift ──

    #[test]
    fn test_detect_failure_drift() {
        let events = vec![
            make_event("failure"),
            make_event("failure"),
            make_event("failure"),
        ];
        assert!(detect_failure_drift(&events));
    }

    #[test]
    fn test_no_failure_drift() {
        let events = vec![
            make_event("success"),
            make_event("failure"),
            make_event("success"),
        ];
        assert!(!detect_failure_drift(&events));
    }

    fn make_event(category: &str) -> VitalsEvent {
        VitalsEvent {
            id: 0,
            category: category.to_string(),
            source: "test".to_string(),
            stability_delta: 0,
            focus_delta: 0,
            sync_delta: 0,
            growth_delta: 0,
            charge_delta: 0,
            metadata_json: None,
            created_at: Utc::now().timestamp(),
            hmac: String::new(),
            prev_hmac: String::new(),
        }
    }

    // ── Bar Rendering ──

    #[test]
    fn test_bar_full() {
        assert_eq!(
            bar(100, 10),
            "\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}"
        );
    }

    #[test]
    fn test_bar_empty() {
        assert_eq!(
            bar(0, 10),
            "\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}\u{2591}"
        );
    }

    #[test]
    fn test_bar_half() {
        let b = bar(50, 10);
        assert_eq!(b.chars().count(), 10);
    }

    // ── HMAC Chain ──

    #[test]
    fn test_hmac_deterministic() {
        let deltas = deltas_for(EventCategory::Interaction);
        let h1 = compute_event_hmac(VITALS_HMAC_LEGACY, "0", "interaction", "test", deltas, 1000);
        let h2 = compute_event_hmac(VITALS_HMAC_LEGACY, "0", "interaction", "test", deltas, 1000);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hmac_changes_with_input() {
        let deltas = deltas_for(EventCategory::Interaction);
        let h1 = compute_event_hmac(VITALS_HMAC_LEGACY, "0", "interaction", "test", deltas, 1000);
        let h2 = compute_event_hmac(VITALS_HMAC_LEGACY, "0", "interaction", "test", deltas, 1001);
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hmac_chain_verification() {
        let deltas = deltas_for(EventCategory::Success);
        let hmac1 =
            compute_event_hmac(VITALS_HMAC_LEGACY, "0", "success", "run_shell", deltas, 100);
        let event = VitalsEvent {
            id: 1,
            category: "success".to_string(),
            source: "run_shell".to_string(),
            stability_delta: deltas.stability as i32,
            focus_delta: deltas.focus as i32,
            sync_delta: deltas.sync as i32,
            growth_delta: deltas.growth as i32,
            charge_delta: deltas.charge as i32,
            metadata_json: None,
            created_at: 100,
            hmac: hmac1.clone(),
            prev_hmac: "0".to_string(),
        };
        assert!(verify_event_hmac(VITALS_HMAC_LEGACY, &event, "0"));
        assert!(!verify_event_hmac(VITALS_HMAC_LEGACY, &event, "wrong_prev"));
    }

    #[test]
    fn test_tampered_event_skipped_in_replay() {
        let deltas = deltas_for(EventCategory::Creation);
        let ts = 1000;
        let hmac = compute_event_hmac(
            VITALS_HMAC_LEGACY,
            "0",
            "creation",
            "create_tool",
            deltas,
            ts,
        );

        let legit = VitalsEvent {
            id: 1,
            category: "creation".to_string(),
            source: "create_tool".to_string(),
            stability_delta: deltas.stability as i32,
            focus_delta: deltas.focus as i32,
            sync_delta: deltas.sync as i32,
            growth_delta: deltas.growth as i32,
            charge_delta: deltas.charge as i32,
            metadata_json: None,
            created_at: ts,
            hmac: hmac.clone(),
            prev_hmac: "0".to_string(),
        };

        // Tampered event with wrong HMAC
        let tampered = VitalsEvent {
            id: 2,
            category: "creation".to_string(),
            source: "fake".to_string(),
            stability_delta: 100,
            focus_delta: 100,
            sync_delta: 100,
            growth_delta: 100,
            charge_delta: 100,
            metadata_json: None,
            created_at: ts + 1,
            hmac: "fake_hmac".to_string(),
            prev_hmac: hmac,
        };

        let state = replay_events(&[legit, tampered]);
        // Only the legit event should be applied (creation: +2 stab)
        assert_eq!(state.stability, 82); // 80 + 2
        assert_eq!(state.growth, 73); // 70 + 3
    }

    // ── Rate Limiting ──

    #[test]
    fn test_rate_limiting_caps_events() {
        let deltas = deltas_for(EventCategory::Creation);
        let hour_base = 1000 * 3600; // some hour

        let mut events = Vec::new();
        let mut prev = "0".to_string();
        // Create 20 events in the same hour (cap is 5 for creation)
        for i in 0..20 {
            let ts = hour_base + i;
            let hmac =
                compute_event_hmac(VITALS_HMAC_LEGACY, &prev, "creation", "test", deltas, ts);
            events.push(VitalsEvent {
                id: i + 1,
                category: "creation".to_string(),
                source: "test".to_string(),
                stability_delta: deltas.stability as i32,
                focus_delta: deltas.focus as i32,
                sync_delta: deltas.sync as i32,
                growth_delta: deltas.growth as i32,
                charge_delta: deltas.charge as i32,
                metadata_json: None,
                created_at: ts,
                hmac: hmac.clone(),
                prev_hmac: prev,
            });
            prev = hmac;
        }

        let state = replay_events(&events);
        // Only 5 creation events should apply (cap), each +2 stability
        assert_eq!(state.stability, 80 + 5 * 2); // 90, not 80 + 20*2
        assert_eq!(state.growth, 70 + 5 * 3); // 85, not 70 + 20*3
    }
}
