//! Evolution system — Pokemon-style agent specialization via sustained usage.
//!
//! Three permanent stages (Base → Evolved → Final) with Lvl.0–99 per stage.
//! Ten archetypes classify usage patterns; LLM generates unique evolution names.
//! State is event-sourced: derived by replaying verified events from baseline.
//! HMAC chain prevents tampering; rate limiting prevents gaming.
//!
//! XP curve is WoW-style: early levels fast, late levels exponentially harder.
//! Stage 1 completes in 2-5 days, Stage 2 in ~30 days, Stage 3 Lvl.99 in 6-12 months.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::fmt;

use crate::db::Database;
use crate::hooks::{Hook, HookAction, HookContext, HookData, HookPoint};

// ── HMAC ──

/// Domain string for HMAC key derivation. Combined with per-installation salt.
pub(crate) const EVOLUTION_HMAC_DOMAIN: &[u8] = b"borg-evolution-chain-v1";

/// Legacy compiled-in secret for installations without per-install salt.
const EVOLUTION_HMAC_LEGACY: &[u8] = b"borg-evolution-chain-v1";

type HmacSha256 = Hmac<Sha256>;

/// Compute HMAC for an evolution event, chaining from the previous event's HMAC.
#[allow(clippy::expect_used)]
pub(crate) fn compute_event_hmac(
    key: &[u8],
    prev_hmac: &str,
    event_type: &str,
    xp_delta: i32,
    archetype: &str,
    source: &str,
    created_at: i64,
) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(prev_hmac.as_bytes());
    mac.update(event_type.as_bytes());
    mac.update(&xp_delta.to_le_bytes());
    mac.update(archetype.as_bytes());
    mac.update(source.as_bytes());
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
fn verify_event_hmac(key: &[u8], event: &EvolutionEvent, expected_prev_hmac: &str) -> bool {
    if event.prev_hmac != expected_prev_hmac {
        return false;
    }
    let expected = compute_event_hmac(
        key,
        &event.prev_hmac,
        &event.event_type,
        event.xp_delta,
        event.archetype.as_deref().unwrap_or(""),
        &event.source,
        event.created_at,
    );
    event.hmac == expected
}

// ── Rate Limiting ──

/// Maximum events per bucket per hour during replay.
pub(crate) fn rate_limit_for(event_type: &str) -> u32 {
    match event_type {
        "xp_gain" => 30,
        "evolution" => 3,
        "classification" => 3,
        "archetype_shift" => 5,
        _ => 10,
    }
}

/// Source-specific rate limit per hour.
const SOURCE_RATE_LIMIT: u32 = 10;

// ── Types ──

/// The 10 archetypes that classify usage patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Archetype {
    Ops,
    Builder,
    Analyst,
    Communicator,
    Guardian,
    Strategist,
    Creator,
    Caretaker,
    Merchant,
    Tinkerer,
}

impl Archetype {
    pub const ALL: [Archetype; 10] = [
        Archetype::Ops,
        Archetype::Builder,
        Archetype::Analyst,
        Archetype::Communicator,
        Archetype::Guardian,
        Archetype::Strategist,
        Archetype::Creator,
        Archetype::Caretaker,
        Archetype::Merchant,
        Archetype::Tinkerer,
    ];

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ops" => Some(Self::Ops),
            "builder" => Some(Self::Builder),
            "analyst" => Some(Self::Analyst),
            "communicator" => Some(Self::Communicator),
            "guardian" => Some(Self::Guardian),
            "strategist" => Some(Self::Strategist),
            "creator" => Some(Self::Creator),
            "caretaker" => Some(Self::Caretaker),
            "merchant" => Some(Self::Merchant),
            "tinkerer" => Some(Self::Tinkerer),
            _ => None,
        }
    }
}

impl fmt::Display for Archetype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ops => write!(f, "ops"),
            Self::Builder => write!(f, "builder"),
            Self::Analyst => write!(f, "analyst"),
            Self::Communicator => write!(f, "communicator"),
            Self::Guardian => write!(f, "guardian"),
            Self::Strategist => write!(f, "strategist"),
            Self::Creator => write!(f, "creator"),
            Self::Caretaker => write!(f, "caretaker"),
            Self::Merchant => write!(f, "merchant"),
            Self::Tinkerer => write!(f, "tinkerer"),
        }
    }
}

/// Three permanent evolution stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Base,
    Evolved,
    Final,
}

impl Stage {
    pub fn number(&self) -> u8 {
        match self {
            Self::Base => 1,
            Self::Evolved => 2,
            Self::Final => 3,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "base" => Some(Self::Base),
            "evolved" => Some(Self::Evolved),
            "final" => Some(Self::Final),
            _ => None,
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Base => write!(f, "base"),
            Self::Evolved => write!(f, "evolved"),
            Self::Final => write!(f, "final"),
        }
    }
}

/// Autonomy tier derived 1:1 from Stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyTier {
    Observe,
    Assist,
    Autonomous,
}

impl AutonomyTier {
    pub fn from_stage(stage: Stage) -> Self {
        match stage {
            Stage::Base => Self::Observe,
            Stage::Evolved => Self::Assist,
            Stage::Final => Self::Autonomous,
        }
    }
}

impl fmt::Display for AutonomyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Observe => write!(f, "Observe"),
            Self::Assist => write!(f, "Assist"),
            Self::Autonomous => write!(f, "Autonomous"),
        }
    }
}

/// A recorded event from the evolution ledger.
#[derive(Debug, Clone)]
pub struct EvolutionEvent {
    pub id: i64,
    pub event_type: String,
    pub xp_delta: i32,
    pub archetype: Option<String>,
    pub source: String,
    pub metadata_json: Option<String>,
    pub created_at: i64,
    pub hmac: String,
    pub prev_hmac: String,
}

/// Computed evolution state (derived from replaying events).
#[derive(Debug, Clone)]
pub struct EvolutionState {
    pub stage: Stage,
    pub level: u8,
    pub total_xp: u32,
    pub xp_to_next_level: u32,
    pub dominant_archetype: Option<Archetype>,
    pub evolution_name: Option<String>,
    pub evolution_description: Option<String>,
    pub archetype_scores: HashMap<Archetype, u32>,
    pub total_events: u32,
    pub chain_valid: bool,
}

// ── XP Curve ──

/// XP required for a specific level at a given stage.
/// WoW-style: Stage 1 is fast (linear), Stage 2 moderate, Stage 3 exponential.
pub fn xp_for_level(stage: &Stage, level: u8) -> u32 {
    let n = level as f64;
    match stage {
        Stage::Base => 2 + level as u32, // base=2, curve=1.0 (linear)
        Stage::Evolved => 8 + (n.powf(1.2)) as u32, // base=8, curve=1.2
        Stage::Final => 20 + (n.powf(1.5)) as u32, // base=20, curve=1.5
    }
}

/// Total XP required to reach a given level from Lvl.0.
pub fn total_xp_for_level(stage: &Stage, target_level: u8) -> u32 {
    (0..target_level).map(|n| xp_for_level(stage, n)).sum()
}

/// Given accumulated XP in current stage, compute (level, xp_remaining_to_next).
pub fn level_from_xp(stage: &Stage, xp: u32) -> (u8, u32) {
    let mut remaining = xp;
    for lvl in 0..99u8 {
        let cost = xp_for_level(stage, lvl);
        if remaining < cost {
            return (lvl, cost - remaining);
        }
        remaining -= cost;
    }
    // Lvl.99 — show 0 remaining
    (99, 0)
}

// ── Archetype Classification ──

/// Deterministic tool-name → archetype mapping.
pub fn classify_tool_archetype(tool_name: &str, metadata: Option<&str>) -> Option<Archetype> {
    // Direct tool name mapping
    let archetype = match tool_name {
        "create_tool" | "apply_patch" | "apply_skill_patch" | "create_channel" => {
            Some(Archetype::Builder)
        }
        "security_audit" => Some(Archetype::Guardian),
        "browser" | "search" | "read_pdf" | "memory_search" => Some(Archetype::Analyst),
        "calendar" | "notion" | "linear" | "manage_tasks" => Some(Archetype::Strategist),
        "gmail" | "outlook" => Some(Archetype::Communicator),
        "write_memory" => Some(Archetype::Creator),
        "run_shell" => classify_shell_command(metadata),
        _ => None,
    };

    if archetype.is_some() {
        return archetype;
    }

    // Check if tool name matches a known channel/integration
    let name_lower = tool_name.to_lowercase();
    if name_lower.contains("telegram")
        || name_lower.contains("slack")
        || name_lower.contains("discord")
        || name_lower.contains("whatsapp")
        || name_lower.contains("sms")
    {
        return Some(Archetype::Communicator);
    }

    if name_lower.contains("docker")
        || name_lower.contains("git")
        || name_lower.contains("database")
    {
        return Some(Archetype::Ops);
    }

    None
}

// Keyword sets for shell command classification
const OPS_KEYWORDS: &[&str] = &[
    "deploy",
    "kubernetes",
    "k8s",
    "docker",
    "terraform",
    "ansible",
    "nginx",
    "systemctl",
    "journalctl",
    "helm",
    "pipeline",
    "prometheus",
    "grafana",
    "kubectl",
    "podman",
];

const BUILDER_KEYWORDS: &[&str] = &[
    "cargo", "npm", "pip", "gcc", "make", "build", "compile", "lint", "rustc", "webpack", "vite",
    "esbuild",
];

const ANALYST_KEYWORDS: &[&str] = &[
    "query",
    "select",
    "aggregate",
    "report",
    "analyze",
    "csv",
    "data",
    "metric",
    "psql",
    "sqlite3",
    "mysql",
];

const GUARDIAN_KEYWORDS: &[&str] = &[
    "firewall",
    "ufw",
    "iptables",
    "nmap",
    "chmod",
    "chown",
    "audit",
    "vulnerability",
    "cve",
    "openssl",
];

const STRATEGIST_KEYWORDS: &[&str] = &[
    "plan",
    "prioritize",
    "compare",
    "evaluate",
    "decision",
    "roadmap",
    "okr",
];

const TINKERER_KEYWORDS: &[&str] = &[
    "homelab",
    "proxmox",
    "pve",
    "esxi",
    "truenas",
    "pihole",
    "wireguard",
    "tailscale",
    "raspberry",
    "arduino",
    "serial",
    "gpio",
    "mqtt",
    "zigbee",
    "zwave",
];

/// Classify a shell command by scanning its content for archetype keywords.
fn classify_shell_command(metadata: Option<&str>) -> Option<Archetype> {
    let text = metadata?.to_lowercase();

    let keyword_sets: &[(&[&str], Archetype)] = &[
        (OPS_KEYWORDS, Archetype::Ops),
        (BUILDER_KEYWORDS, Archetype::Builder),
        (ANALYST_KEYWORDS, Archetype::Analyst),
        (GUARDIAN_KEYWORDS, Archetype::Guardian),
        (STRATEGIST_KEYWORDS, Archetype::Strategist),
        (TINKERER_KEYWORDS, Archetype::Tinkerer),
    ];

    for (keywords, archetype) in keyword_sets {
        if keywords.iter().any(|kw| text.contains(kw)) {
            return Some(*archetype);
        }
    }

    None
}

// ── Scoring ──

/// Base XP awarded for a successful tool call.
const BASE_XP_TOOL_SUCCESS: i32 = 1;
/// Bonus XP for archetype-aligned tool success.
const BONUS_XP_ALIGNED: i32 = 2;
/// Base XP for creation events.
const BASE_XP_CREATION: i32 = 3;
/// Bonus XP for archetype-aligned creation.
const BONUS_XP_CREATION_ALIGNED: i32 = 3;
/// Base XP for session interaction.
const BASE_XP_INTERACTION: i32 = 1;

// ── Event Replay (Event Sourcing) ──

/// Replay verified events from baseline to compute current evolution state.
/// Verifies HMAC chain and applies rate limits per event type per hour.
pub fn replay_events(events: &[EvolutionEvent]) -> EvolutionState {
    replay_events_with_key(EVOLUTION_HMAC_LEGACY, events)
}

/// Replay events with a specific HMAC key (for per-installation derived keys).
pub fn replay_events_with_key(key: &[u8], events: &[EvolutionEvent]) -> EvolutionState {
    let mut stage = Stage::Base;
    let mut total_xp: u32 = 0;
    let mut archetype_scores: HashMap<Archetype, u32> = HashMap::new();
    let mut evolution_name: Option<String> = None;
    let mut evolution_description: Option<String> = None;
    let mut chain_valid = true;
    let mut expected_prev_hmac = "0".to_string();
    let mut accepted_events: u32 = 0;

    // Rate limiting: (hour_bucket, event_type) -> count
    let mut hourly_type_counts: HashMap<(i64, &str), u32> = HashMap::new();
    // Rate limiting: (hour_bucket, source) -> count
    let mut hourly_source_counts: HashMap<(i64, String), u32> = HashMap::new();

    for event in events {
        // Verify HMAC chain
        if !verify_event_hmac(key, event, &expected_prev_hmac) {
            tracing::warn!(
                "evolution: broken HMAC chain at event {}, skipping",
                event.id
            );
            chain_valid = false;
            continue;
        }
        expected_prev_hmac = event.hmac.clone();

        // Rate limiting
        let hour_bucket = event.created_at / 3600;
        let type_key = (hour_bucket, event.event_type.as_str());
        let type_count = hourly_type_counts.entry(type_key).or_insert(0);
        if *type_count >= rate_limit_for(&event.event_type) {
            continue;
        }
        *type_count += 1;

        let source_key = (hour_bucket, event.source.clone());
        let source_count = hourly_source_counts.entry(source_key).or_insert(0);
        if *source_count >= SOURCE_RATE_LIMIT {
            continue;
        }
        *source_count += 1;

        accepted_events += 1;

        match event.event_type.as_str() {
            "xp_gain" => {
                total_xp = total_xp.saturating_add(event.xp_delta.max(0) as u32);
                // Update archetype score
                if let Some(ref arch_str) = event.archetype {
                    if let Some(arch) = Archetype::parse(arch_str) {
                        let score = archetype_scores.entry(arch).or_insert(0);
                        *score = score.saturating_add(event.xp_delta.max(0) as u32);
                    }
                }
            }
            "evolution" => {
                // Warn if this evolution event wasn't gate-verified
                let gates_verified = event
                    .metadata_json
                    .as_deref()
                    .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
                    .and_then(|v| v.get("gates_verified").and_then(serde_json::Value::as_bool))
                    .unwrap_or(false);
                if !gates_verified {
                    tracing::warn!(
                        "evolution: rejecting event {} without gates_verified",
                        event.id
                    );
                    continue;
                }
                // Stage transition: reset XP, advance stage
                stage = match stage {
                    Stage::Base => Stage::Evolved,
                    Stage::Evolved => Stage::Final,
                    Stage::Final => Stage::Final, // already max
                };
                total_xp = 0;
                // Extract name and description from metadata
                if let Some(ref meta) = event.metadata_json {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(meta) {
                        if let Some(name) = parsed.get("name").and_then(|v| v.as_str()) {
                            evolution_name = Some(name.to_string());
                        }
                        if let Some(desc) = parsed.get("description").and_then(|v| v.as_str()) {
                            evolution_description = Some(desc.to_string());
                        }
                    }
                }
            }
            "classification" | "archetype_shift" => {
                // Informational — metadata may update dominant archetype tracking
            }
            _ => {}
        }
    }

    let (level, xp_to_next) = level_from_xp(&stage, total_xp);

    EvolutionState {
        stage,
        level,
        total_xp,
        xp_to_next_level: xp_to_next,
        dominant_archetype: dominant_archetype(&archetype_scores),
        evolution_name,
        evolution_description,
        archetype_scores,
        total_events: accepted_events,
        chain_valid,
    }
}

/// Find the dominant archetype (highest score).
fn dominant_archetype(scores: &HashMap<Archetype, u32>) -> Option<Archetype> {
    scores
        .iter()
        .max_by_key(|(_, &score)| score)
        .filter(|(_, &score)| score > 0)
        .map(|(&arch, _)| arch)
}

// ── Evolution Gates ──

/// Check if Stage 1→2 evolution gates are met.
pub fn check_stage1_gates(state: &EvolutionState, bond_score: u8, min_vital: u8) -> bool {
    if state.stage != Stage::Base {
        return false;
    }
    if state.level < 99 {
        return false;
    }
    if bond_score < 30 {
        return false;
    }
    if min_vital < 20 {
        return false;
    }

    // Dominant archetype must be ≥ 1.3x runner-up
    let mut scores: Vec<u32> = state.archetype_scores.values().copied().collect();
    scores.sort_unstable_by(|a, b| b.cmp(a));
    if scores.is_empty() || scores[0] == 0 {
        return false;
    }
    let runner_up = if scores.len() > 1 { scores[1] } else { 0 };
    // Allow evolution if runner_up is 0 (only one archetype used)
    if runner_up > 0 && (scores[0] as f64) < (runner_up as f64 * 1.3) {
        return false;
    }

    true
}

/// Check if Stage 2→3 evolution gates are met.
pub fn check_stage2_gates(
    state: &EvolutionState,
    bond_score: u8,
    correction_rate: f64,
    archetype_stable_days: u32,
) -> bool {
    if state.stage != Stage::Evolved {
        return false;
    }
    if state.level < 99 {
        return false;
    }
    if bond_score < 55 {
        return false;
    }
    if correction_rate >= 0.20 {
        return false;
    }
    if archetype_stable_days < 14 {
        return false;
    }

    true
}

/// Compute how many consecutive days the dominant archetype has been stable.
pub fn compute_archetype_stable_days(db: &Database) -> u32 {
    let events = db.load_all_evolution_events().unwrap_or_default();
    if events.is_empty() {
        return 0;
    }

    let mut scores: HashMap<Archetype, u32> = HashMap::new();
    let mut last_dominant: Option<Archetype> = None;
    let mut stable_since: i64 = events.first().map(|e| e.created_at).unwrap_or(0);

    for event in &events {
        if event.event_type == "xp_gain" {
            if let Some(ref arch_str) = event.archetype {
                if let Some(arch) = Archetype::parse(arch_str) {
                    let score = scores.entry(arch).or_insert(0);
                    *score = score.saturating_add(event.xp_delta.max(0) as u32);
                }
            }
        }
        let current_dominant = scores.iter().max_by_key(|(_, &v)| v).map(|(&k, _)| k);
        if current_dominant != last_dominant && current_dominant.is_some() {
            stable_since = event.created_at;
            last_dominant = current_dominant;
        }
    }

    let now = chrono::Utc::now().timestamp();
    let seconds = (now - stable_since).max(0) as u64;
    (seconds / 86400) as u32
}

// ── Formatting ──

/// Compact one-liner for TUI session header.
pub fn format_compact(state: &EvolutionState) -> String {
    match (&state.evolution_name, &state.dominant_archetype) {
        (Some(name), Some(arch)) => {
            let arch_display = format!("{arch}");
            let capitalized = capitalize_first(&arch_display);
            format!("[{name} Lvl.{} | {capitalized}]", state.level)
        }
        (Some(name), None) => format!("[{name} Lvl.{}]", state.level),
        (None, Some(arch)) => {
            let arch_display = format!("{arch}");
            let capitalized = capitalize_first(&arch_display);
            format!("[Base Form Lvl.{} | {capitalized}]", state.level)
        }
        (None, None) => format!("[Base Form Lvl.{}]", state.level),
    }
}

/// Full status section for `borg status` output.
pub fn format_status_section(state: &EvolutionState) -> String {
    let mut out = String::new();

    // Header: name + level
    match &state.evolution_name {
        Some(name) => out.push_str(&format!("  {name} Lvl.{}\n", state.level)),
        None => out.push_str(&format!("  Base Form Lvl.{}\n", state.level)),
    }

    // Description
    match &state.evolution_description {
        Some(desc) => out.push_str(&format!("  \"{desc}\"\n")),
        None => out.push_str("  Discovering your patterns...\n"),
    }

    out.push('\n');

    // Stage progress bar
    let stage_label = match state.stage {
        Stage::Base => "Base (1/3)",
        Stage::Evolved => "Evolved (2/3)",
        Stage::Final => "Final (3/3)",
    };
    let stage_fill = match state.stage {
        Stage::Base => 10,
        Stage::Evolved => 20,
        Stage::Final => 30,
    };
    let stage_bar = format!(
        "{}{}",
        "\u{2588}".repeat(stage_fill),
        "\u{2591}".repeat(30 - stage_fill)
    );
    out.push_str(&format!("  Stage        {stage_bar}  {stage_label}\n"));

    // XP progress
    if state.level < 99 {
        let xp_needed = xp_for_level(&state.stage, state.level);
        let xp_into_level = xp_needed.saturating_sub(state.xp_to_next_level);
        out.push_str(&format!(
            "  XP           {xp_into_level} / {xp_needed} to Lvl.{}\n",
            state.level + 1
        ));
    } else {
        out.push_str("  XP           MAX LEVEL\n");
    }

    out
}

/// Format archetype scores for `borg status archetypes`.
pub fn format_archetype_scores(state: &EvolutionState) -> String {
    let mut out = String::from("Archetype Scores\n");

    let mut sorted: Vec<(Archetype, u32)> = Archetype::ALL
        .iter()
        .map(|a| (*a, *state.archetype_scores.get(a).unwrap_or(&0)))
        .collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));

    let max_score = sorted.first().map(|(_, s)| *s).unwrap_or(1).max(1);

    for (arch, score) in &sorted {
        let arch_display = format!("{arch}");
        let capitalized = capitalize_first(&arch_display);
        let bar_len = (*score as usize * 10) / max_score as usize;
        let bar = format!(
            "{}{}",
            "\u{2588}".repeat(bar_len),
            "\u{2591}".repeat(10 - bar_len)
        );
        let marker = if Some(*arch) == state.dominant_archetype {
            " *"
        } else {
            ""
        };
        out.push_str(&format!("  {capitalized:<15} {score:>5}  {bar}{marker}\n"));
    }

    out
}

/// Format evolution history timeline.
pub fn format_history(events: &[EvolutionEvent]) -> String {
    let evolution_events: Vec<&EvolutionEvent> = events
        .iter()
        .filter(|e| e.event_type == "evolution")
        .collect();

    if evolution_events.is_empty() {
        return "Evolution History\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n  No evolutions yet. Keep using Borg!\n".to_string();
    }

    let mut out = String::from("Evolution History\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");

    for event in &evolution_events {
        let ts = chrono::DateTime::from_timestamp(event.created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let mut name = String::new();
        let mut desc = String::new();
        if let Some(ref meta) = event.metadata_json {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(meta) {
                if let Some(n) = parsed.get("name").and_then(|v| v.as_str()) {
                    name = n.to_string();
                }
                if let Some(d) = parsed.get("description").and_then(|v| v.as_str()) {
                    desc = d.to_string();
                }
            }
        }

        let stage_label = if name.is_empty() {
            "Evolved".to_string()
        } else {
            name.clone()
        };

        out.push_str(&format!("  {ts}  → {stage_label}\n"));
        if !desc.is_empty() {
            out.push_str(&format!("           \"{desc}\"\n"));
        }
    }

    out
}

/// XML evolution context for system prompt injection.
pub fn format_evolution_context(state: &EvolutionState) -> String {
    let name = state.evolution_name.as_deref().unwrap_or("Base Form");
    let stage = match state.stage {
        Stage::Base => "Base",
        Stage::Evolved => "Evolved",
        Stage::Final => "Final",
    };
    let autonomy = AutonomyTier::from_stage(state.stage);
    let arch = state
        .dominant_archetype
        .map(|a| {
            let s = format!("{a}");
            let score = state.archetype_scores.get(&a).unwrap_or(&0);
            format!("\nArchetype: {} (score: {score})", capitalize_first(&s))
        })
        .unwrap_or_default();

    format!(
        "<evolution_context>\nStage: {stage} | {name} Lvl.{}{arch}\nAutonomy: {autonomy}\n</evolution_context>",
        state.level
    )
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

// ── EvolutionHook ──

/// Lifecycle hook that passively records evolution XP events and injects context.
pub struct EvolutionHook {
    db: std::sync::Mutex<Database>,
}

impl EvolutionHook {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            db: std::sync::Mutex::new(Database::open()?),
        })
    }

    fn record_xp(&self, source: &str, archetype: Option<Archetype>, xp: i32) {
        let Ok(db) = self.db.lock() else {
            tracing::warn!("evolution: mutex poisoned, skipping event");
            return;
        };
        let arch_str = archetype.map(|a| a.to_string());
        if let Err(e) = db.record_evolution_event("xp_gain", xp, arch_str.as_deref(), source, None)
        {
            tracing::warn!("evolution: failed to record XP event: {e}");
            return;
        }
        // Check if we should attempt an evolution after this XP gain
        self.attempt_evolution(&db);
    }

    /// Check evolution gates and record an evolution event if all prerequisites are met.
    fn attempt_evolution(&self, db: &Database) {
        let evo_state = match db.get_evolution_state() {
            Ok(s) => s,
            Err(_) => return,
        };

        // Only trigger at level 99 (stage cap)
        if evo_state.level < 99 {
            return;
        }

        // Already at final stage
        if evo_state.stage == Stage::Final {
            return;
        }

        // Get bond state for gate checks (use derived key for HMAC verification)
        let bond_events = db.get_all_bond_events().unwrap_or_default();
        let bond_key = db.derive_hmac_key(crate::bond::BOND_HMAC_DOMAIN);
        let bond_state = crate::bond::replay_events_with_key(&bond_key, &bond_events);

        // Get vitals state for gate checks
        let vitals_state = match db.get_vitals_state() {
            Ok(s) => s,
            Err(_) => return,
        };
        let min_vital = vitals_state
            .stability
            .min(vitals_state.focus)
            .min(vitals_state.sync)
            .min(vitals_state.growth)
            .min(vitals_state.charge);

        let gates_passed = match evo_state.stage {
            Stage::Base => check_stage1_gates(&evo_state, bond_state.score, min_vital),
            Stage::Evolved => {
                // Compute correction rate from vitals events (last 14 days)
                let fourteen_days_ago = chrono::Utc::now().timestamp() - 14 * 86400;
                let (corrections, total) = db
                    .count_vitals_events_by_category_since(fourteen_days_ago, "correction")
                    .unwrap_or((0, 1));
                let correction_rate = if total > 0 {
                    corrections as f64 / total as f64
                } else {
                    0.0
                };
                // Approximate archetype stable days from first event with current dominant archetype
                let archetype_stable_days = compute_archetype_stable_days(db);
                check_stage2_gates(
                    &evo_state,
                    bond_state.score,
                    correction_rate,
                    archetype_stable_days,
                )
            }
            Stage::Final => false,
        };

        if !gates_passed {
            return;
        }

        let metadata = serde_json::json!({ "gates_verified": true }).to_string();
        if let Err(e) =
            db.record_evolution_event("evolution", 0, None, "gate_check", Some(&metadata))
        {
            tracing::warn!("evolution: failed to record evolution event: {e}");
        } else {
            tracing::info!("evolution: stage transition triggered — gates verified");
        }
    }

    fn evolution_context(&self) -> String {
        let Ok(db) = self.db.lock() else {
            return String::new();
        };
        match db.get_evolution_state() {
            Ok(state) => format_evolution_context(&state),
            Err(_) => String::new(),
        }
    }
}

impl Hook for EvolutionHook {
    fn name(&self) -> &str {
        "evolution"
    }

    fn points(&self) -> &[HookPoint] {
        &[
            HookPoint::SessionStart,
            HookPoint::BeforeAgentStart,
            HookPoint::BeforeLlmCall,
            HookPoint::AfterToolCall,
        ]
    }

    fn execute(&self, ctx: &HookContext) -> HookAction {
        match &ctx.data {
            HookData::SessionStart { .. } => {
                // Record interaction XP
                self.record_xp("session_start", None, BASE_XP_INTERACTION);
            }
            HookData::AgentStart { .. } => {
                let context = self.evolution_context();
                if !context.is_empty() {
                    return HookAction::InjectContext(context);
                }
            }
            HookData::LlmCall { .. } => {
                let context = self.evolution_context();
                if !context.is_empty() {
                    return HookAction::InjectContext(context);
                }
            }
            HookData::ToolResult {
                name,
                is_error,
                result,
                ..
            } => {
                if !*is_error {
                    let archetype = classify_tool_archetype(name, Some(result.as_str()));
                    let is_creation = matches!(
                        name.as_str(),
                        "create_tool"
                            | "apply_patch"
                            | "apply_skill_patch"
                            | "create_channel"
                            | "write_memory"
                    );
                    let xp = if is_creation {
                        let bonus = if archetype.is_some() {
                            BONUS_XP_CREATION_ALIGNED
                        } else {
                            0
                        };
                        BASE_XP_CREATION + bonus
                    } else {
                        let bonus = if archetype.is_some() {
                            BONUS_XP_ALIGNED
                        } else {
                            0
                        };
                        BASE_XP_TOOL_SUCCESS + bonus
                    };
                    self.record_xp(name, archetype, xp);
                }
            }
            _ => {}
        }
        HookAction::Continue
    }
}

impl std::fmt::Debug for EvolutionHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EvolutionHook").finish()
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    // ── XP Curve ──

    #[test]
    fn xp_for_level_stage1_boundaries() {
        assert_eq!(xp_for_level(&Stage::Base, 0), 2);
        assert_eq!(xp_for_level(&Stage::Base, 50), 52);
        assert_eq!(xp_for_level(&Stage::Base, 99), 101);
    }

    #[test]
    fn xp_for_level_stage2_grows_faster() {
        let s1_total = total_xp_for_level(&Stage::Base, 99);
        let s2_total = total_xp_for_level(&Stage::Evolved, 99);
        assert!(
            s2_total > s1_total * 2,
            "Stage 2 total {s2_total} should be much larger than Stage 1 {s1_total}"
        );
    }

    #[test]
    fn xp_for_level_stage3_exponential() {
        let s3_early = xp_for_level(&Stage::Final, 10);
        let s3_late = xp_for_level(&Stage::Final, 90);
        assert!(
            s3_late > s3_early * 5,
            "Stage 3 late levels should be much harder: early={s3_early} late={s3_late}"
        );
    }

    #[test]
    fn level_from_xp_round_trip() {
        for stage in &[Stage::Base, Stage::Evolved, Stage::Final] {
            let target = 50u8;
            let needed = total_xp_for_level(stage, target);
            let (level, _remaining) = level_from_xp(stage, needed);
            assert_eq!(
                level, target,
                "stage={stage}: xp={needed} should give level {target}"
            );
        }
    }

    #[test]
    fn level_from_xp_max_at_99() {
        let (level, remaining) = level_from_xp(&Stage::Base, 999_999);
        assert_eq!(level, 99);
        assert_eq!(remaining, 0);
    }

    // ── Archetype Classification ──

    #[test]
    fn classify_builder_tools() {
        assert_eq!(
            classify_tool_archetype("create_tool", None),
            Some(Archetype::Builder)
        );
        assert_eq!(
            classify_tool_archetype("apply_patch", None),
            Some(Archetype::Builder)
        );
        assert_eq!(
            classify_tool_archetype("apply_skill_patch", None),
            Some(Archetype::Builder)
        );
    }

    #[test]
    fn classify_guardian_tools() {
        assert_eq!(
            classify_tool_archetype("security_audit", None),
            Some(Archetype::Guardian)
        );
    }

    #[test]
    fn classify_analyst_tools() {
        assert_eq!(
            classify_tool_archetype("browser", None),
            Some(Archetype::Analyst)
        );
        assert_eq!(
            classify_tool_archetype("search", None),
            Some(Archetype::Analyst)
        );
    }

    #[test]
    fn classify_shell_ops_keywords() {
        assert_eq!(
            classify_tool_archetype("run_shell", Some("docker compose up -d")),
            Some(Archetype::Ops)
        );
        assert_eq!(
            classify_tool_archetype("run_shell", Some("kubectl apply -f deploy.yaml")),
            Some(Archetype::Ops)
        );
    }

    #[test]
    fn classify_shell_builder_keywords() {
        assert_eq!(
            classify_tool_archetype("run_shell", Some("cargo build --release")),
            Some(Archetype::Builder)
        );
        assert_eq!(
            classify_tool_archetype("run_shell", Some("npm install express")),
            Some(Archetype::Builder)
        );
    }

    #[test]
    fn classify_shell_tinkerer_keywords() {
        assert_eq!(
            classify_tool_archetype("run_shell", Some("ssh proxmox 'qm list'")),
            Some(Archetype::Tinkerer)
        );
        assert_eq!(
            classify_tool_archetype("run_shell", Some("tailscale status")),
            Some(Archetype::Tinkerer)
        );
    }

    #[test]
    fn classify_unknown_tool_returns_none() {
        assert_eq!(classify_tool_archetype("list_dir", None), None);
        assert_eq!(classify_tool_archetype("read_file", None), None);
    }

    #[test]
    fn classify_channel_names() {
        assert_eq!(
            classify_tool_archetype("telegram_send", None),
            Some(Archetype::Communicator)
        );
        assert_eq!(
            classify_tool_archetype("slack_post", None),
            Some(Archetype::Communicator)
        );
    }

    #[test]
    fn all_archetypes_have_at_least_one_signal() {
        // Verify each archetype can be reached
        let cases: &[(&str, Option<&str>, Archetype)] = &[
            ("run_shell", Some("docker ps"), Archetype::Ops),
            ("create_tool", None, Archetype::Builder),
            ("browser", None, Archetype::Analyst),
            ("telegram_send", None, Archetype::Communicator),
            ("security_audit", None, Archetype::Guardian),
            ("calendar", None, Archetype::Strategist),
            ("write_memory", None, Archetype::Creator),
            // Caretaker, Merchant are primarily classified by user-created tools
            ("run_shell", Some("homelab proxmox"), Archetype::Tinkerer),
        ];
        for (tool, meta, expected) in cases {
            assert_eq!(
                classify_tool_archetype(tool, *meta),
                Some(*expected),
                "Expected {expected} for tool={tool} meta={meta:?}"
            );
        }
    }

    // ── HMAC Chain ──

    #[test]
    fn hmac_deterministic() {
        let h1 = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            "0",
            "xp_gain",
            3,
            "builder",
            "create_tool",
            1000,
        );
        let h2 = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            "0",
            "xp_gain",
            3,
            "builder",
            "create_tool",
            1000,
        );
        assert_eq!(h1, h2);
    }

    #[test]
    fn hmac_verify_valid() {
        let hmac = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            "0",
            "xp_gain",
            1,
            "ops",
            "run_shell",
            1000,
        );
        let event = EvolutionEvent {
            id: 1,
            event_type: "xp_gain".to_string(),
            xp_delta: 1,
            archetype: Some("ops".to_string()),
            source: "run_shell".to_string(),
            metadata_json: None,
            created_at: 1000,
            hmac: hmac.clone(),
            prev_hmac: "0".to_string(),
        };
        assert!(verify_event_hmac(EVOLUTION_HMAC_LEGACY, &event, "0"));
    }

    #[test]
    fn hmac_tamper_detection() {
        let hmac = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            "0",
            "xp_gain",
            1,
            "ops",
            "run_shell",
            1000,
        );
        let mut event = EvolutionEvent {
            id: 1,
            event_type: "xp_gain".to_string(),
            xp_delta: 1,
            archetype: Some("ops".to_string()),
            source: "run_shell".to_string(),
            metadata_json: None,
            created_at: 1000,
            hmac,
            prev_hmac: "0".to_string(),
        };
        // Tamper with XP
        event.xp_delta = 999;
        assert!(!verify_event_hmac(EVOLUTION_HMAC_LEGACY, &event, "0"));
    }

    #[test]
    fn hmac_chain_linking() {
        let h1 = compute_event_hmac(EVOLUTION_HMAC_LEGACY, "0", "xp_gain", 1, "", "a", 1000);
        let h2 = compute_event_hmac(EVOLUTION_HMAC_LEGACY, &h1, "xp_gain", 1, "", "b", 2000);
        assert_ne!(h1, h2);

        let e2 = EvolutionEvent {
            id: 2,
            event_type: "xp_gain".to_string(),
            xp_delta: 1,
            archetype: None,
            source: "b".to_string(),
            metadata_json: None,
            created_at: 2000,
            hmac: h2,
            prev_hmac: h1.clone(),
        };
        assert!(verify_event_hmac(EVOLUTION_HMAC_LEGACY, &e2, &h1));
        // Wrong prev_hmac
        assert!(!verify_event_hmac(EVOLUTION_HMAC_LEGACY, &e2, "0"));
    }

    // ── Replay ──

    fn make_event(
        id: i64,
        event_type: &str,
        xp_delta: i32,
        archetype: Option<&str>,
        source: &str,
        created_at: i64,
        prev_hmac: &str,
    ) -> EvolutionEvent {
        let hmac = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            prev_hmac,
            event_type,
            xp_delta,
            archetype.unwrap_or(""),
            source,
            created_at,
        );
        EvolutionEvent {
            id,
            event_type: event_type.to_string(),
            xp_delta,
            archetype: archetype.map(|s| s.to_string()),
            source: source.to_string(),
            metadata_json: None,
            created_at,
            hmac,
            prev_hmac: prev_hmac.to_string(),
        }
    }

    #[test]
    fn replay_empty_events_gives_baseline() {
        let state = replay_events(&[]);
        assert_eq!(state.stage, Stage::Base);
        assert_eq!(state.level, 0);
        assert_eq!(state.total_xp, 0);
        assert!(state.chain_valid);
        assert!(state.dominant_archetype.is_none());
    }

    #[test]
    fn replay_xp_accumulates() {
        let e1 = make_event(1, "xp_gain", 3, Some("ops"), "run_shell", 1000, "0");
        let e2 = make_event(2, "xp_gain", 3, Some("ops"), "run_shell", 2000, &e1.hmac);
        let state = replay_events(&[e1, e2]);
        assert_eq!(state.total_xp, 6);
        assert_eq!(state.dominant_archetype, Some(Archetype::Ops));
        assert_eq!(*state.archetype_scores.get(&Archetype::Ops).unwrap(), 6);
    }

    #[test]
    fn replay_level_up() {
        // Stage 1: level 0 costs 2 XP, level 1 costs 3 XP
        let e1 = make_event(1, "xp_gain", 5, None, "test", 1000, "0");
        let state = replay_events(&[e1]);
        assert_eq!(state.total_xp, 5);
        // 5 XP: level 0 costs 2, level 1 costs 3 → at level 2
        assert_eq!(state.level, 2);
    }

    #[test]
    fn replay_stage_transition() {
        let e1 = make_event(1, "xp_gain", 999_999, None, "test", 1000, "0");
        let meta = r#"{"gates_verified":true,"name":"Pipeline Warden","description":"A vigilant guardian"}"#;
        let mut e2 = make_event(2, "evolution", 0, None, "system", 2000, &e1.hmac);
        e2.metadata_json = Some(meta.to_string());

        let state = replay_events(&[e1, e2]);
        assert_eq!(state.stage, Stage::Evolved);
        assert_eq!(state.total_xp, 0); // Reset after evolution
        assert_eq!(state.level, 0);
        assert_eq!(state.evolution_name.as_deref(), Some("Pipeline Warden"));
        assert_eq!(
            state.evolution_description.as_deref(),
            Some("A vigilant guardian")
        );
    }

    #[test]
    fn replay_broken_hmac_skips_event() {
        let e1 = make_event(1, "xp_gain", 3, None, "test", 1000, "0");
        let mut e2 = make_event(2, "xp_gain", 100, None, "test", 2000, &e1.hmac);
        e2.hmac = "tampered".to_string(); // Break the chain
        let e3 = make_event(3, "xp_gain", 1, None, "test", 3000, &e2.hmac);

        let state = replay_events(&[e1, e2, e3]);
        // e2 and e3 should be skipped (broken chain)
        assert_eq!(state.total_xp, 3);
        assert!(!state.chain_valid);
    }

    #[test]
    fn replay_rate_limiting() {
        // Create 35 events in the same hour (limit is 30 for xp_gain)
        let mut events = Vec::new();
        let mut prev = "0".to_string();
        for i in 0..35 {
            let e = make_event(
                i + 1,
                "xp_gain",
                1,
                None,
                "test",
                1000 + i as i64, // all in same hour
                &prev,
            );
            prev = e.hmac.clone();
            events.push(e);
        }
        let state = replay_events(&events);
        // Only 30 should count (rate limited), but source limit is 10
        // Since all have same source "test", only 10 should count
        assert_eq!(state.total_xp, 10);
    }

    #[test]
    fn replay_multiple_stages() {
        let e1 = make_event(1, "xp_gain", 999_999, Some("ops"), "run_shell", 1000, "0");
        let mut e2 = make_event(2, "evolution", 0, None, "system", 2000, &e1.hmac);
        e2.metadata_json = Some(r#"{"gates_verified":true,"name":"Sentinel"}"#.to_string());
        let e3 = make_event(
            3,
            "xp_gain",
            999_999,
            Some("ops"),
            "run_shell",
            3000,
            &e2.hmac,
        );
        let mut e4 = make_event(4, "evolution", 0, None, "system", 4000, &e3.hmac);
        e4.metadata_json = Some(
            r#"{"gates_verified":true,"name":"Overseer","description":"Supreme commander"}"#
                .to_string(),
        );
        let e5 = make_event(5, "xp_gain", 50, Some("ops"), "run_shell", 5000, &e4.hmac);

        let state = replay_events(&[e1, e2, e3, e4, e5]);
        assert_eq!(state.stage, Stage::Final);
        assert_eq!(state.total_xp, 50);
        assert_eq!(state.evolution_name.as_deref(), Some("Overseer"));
        assert_eq!(
            state.evolution_description.as_deref(),
            Some("Supreme commander")
        );
    }

    // ── Evolution Gates ──

    fn make_state_at_level(
        stage: Stage,
        level: u8,
        archetype_scores: HashMap<Archetype, u32>,
    ) -> EvolutionState {
        let xp = total_xp_for_level(&stage, level);
        let (_, xp_to_next) = level_from_xp(&stage, xp);
        EvolutionState {
            stage,
            level,
            total_xp: xp,
            xp_to_next_level: xp_to_next,
            dominant_archetype: dominant_archetype(&archetype_scores),
            evolution_name: None,
            evolution_description: None,
            archetype_scores,
            total_events: 100,
            chain_valid: true,
        }
    }

    #[test]
    fn stage1_gates_pass() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 100);
        scores.insert(Archetype::Builder, 50);
        let state = make_state_at_level(Stage::Base, 99, scores);
        assert!(check_stage1_gates(&state, 30, 20));
    }

    #[test]
    fn stage1_gates_fail_level() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 100);
        let state = make_state_at_level(Stage::Base, 50, scores);
        assert!(!check_stage1_gates(&state, 30, 20));
    }

    #[test]
    fn stage1_gates_fail_bond() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 100);
        let state = make_state_at_level(Stage::Base, 99, scores);
        assert!(!check_stage1_gates(&state, 20, 20)); // bond < 30
    }

    #[test]
    fn stage1_gates_fail_close_archetypes() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 100);
        scores.insert(Archetype::Builder, 90); // too close (100/90 < 1.3)
        let state = make_state_at_level(Stage::Base, 99, scores);
        assert!(!check_stage1_gates(&state, 30, 20));
    }

    #[test]
    fn stage2_gates_pass() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 200);
        let state = make_state_at_level(Stage::Evolved, 99, scores);
        assert!(check_stage2_gates(&state, 55, 0.10, 30));
    }

    #[test]
    fn stage2_gates_fail_correction_rate() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 200);
        let state = make_state_at_level(Stage::Evolved, 99, scores);
        assert!(!check_stage2_gates(&state, 55, 0.25, 30)); // correction too high
    }

    #[test]
    fn stage2_gates_fail_archetype_stability() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 200);
        let state = make_state_at_level(Stage::Evolved, 99, scores);
        assert!(!check_stage2_gates(&state, 55, 0.10, 10)); // only 10 days stable
    }

    // ── Formatting ──

    #[test]
    fn compact_format_base() {
        let state = replay_events(&[]);
        let compact = format_compact(&state);
        assert!(compact.contains("Base Form"));
        assert!(compact.contains("Lvl.0"));
    }

    #[test]
    fn compact_format_evolved() {
        let state = EvolutionState {
            stage: Stage::Evolved,
            level: 42,
            total_xp: 1000,
            xp_to_next_level: 50,
            dominant_archetype: Some(Archetype::Ops),
            evolution_name: Some("Pipeline Warden".to_string()),
            evolution_description: Some("A vigilant guardian".to_string()),
            archetype_scores: HashMap::new(),
            total_events: 100,
            chain_valid: true,
        };
        let compact = format_compact(&state);
        assert!(compact.contains("Pipeline Warden"));
        assert!(compact.contains("Lvl.42"));
        assert!(compact.contains("Ops"));
    }

    #[test]
    fn status_section_shows_description() {
        let state = EvolutionState {
            stage: Stage::Evolved,
            level: 42,
            total_xp: 1000,
            xp_to_next_level: 50,
            dominant_archetype: Some(Archetype::Ops),
            evolution_name: Some("Pipeline Warden".to_string()),
            evolution_description: Some("A vigilant guardian".to_string()),
            archetype_scores: HashMap::new(),
            total_events: 100,
            chain_valid: true,
        };
        let section = format_status_section(&state);
        assert!(section.contains("Pipeline Warden"));
        assert!(section.contains("A vigilant guardian"));
        assert!(section.contains("Evolved (2/3)"));
    }

    #[test]
    fn evolution_context_xml() {
        let state = EvolutionState {
            stage: Stage::Evolved,
            level: 42,
            total_xp: 1000,
            xp_to_next_level: 50,
            dominant_archetype: Some(Archetype::Ops),
            evolution_name: Some("Pipeline Warden".to_string()),
            evolution_description: None,
            archetype_scores: {
                let mut m = HashMap::new();
                m.insert(Archetype::Ops, 74);
                m
            },
            total_events: 100,
            chain_valid: true,
        };
        let ctx = format_evolution_context(&state);
        assert!(ctx.contains("<evolution_context>"));
        assert!(ctx.contains("Pipeline Warden Lvl.42"));
        assert!(ctx.contains("Ops"));
        assert!(ctx.contains("Assist"));
    }

    // ── Enum roundtrips ──

    #[test]
    fn archetype_display_parse_roundtrip() {
        for arch in &Archetype::ALL {
            let s = arch.to_string();
            assert_eq!(
                Archetype::parse(&s),
                Some(*arch),
                "roundtrip failed for {arch:?}"
            );
        }
    }

    #[test]
    fn stage_display_parse_roundtrip() {
        for stage in &[Stage::Base, Stage::Evolved, Stage::Final] {
            let s = stage.to_string();
            assert_eq!(
                Stage::parse(&s),
                Some(*stage),
                "roundtrip failed for {stage:?}"
            );
        }
    }

    // ── Gate Enforcement Tests ──

    #[test]
    fn replay_rejects_unverified_evolution_event() {
        // An evolution event without gates_verified metadata should be rejected
        let e1 = make_event(1, "xp_gain", 5, Some("ops"), "test", 1000, "0");
        let e2 = make_event(2, "evolution", 0, None, "system", 2000, &e1.hmac);
        let state = replay_events(&[e1, e2]);
        assert_eq!(
            state.stage,
            Stage::Base,
            "evolution event without gates_verified should be rejected"
        );
        assert!(state.chain_valid);
    }

    #[test]
    fn replay_accepts_gates_verified_evolution_event() {
        let e1 = make_event(1, "xp_gain", 5, Some("ops"), "test", 1000, "0");
        let mut e2 = make_event(2, "evolution", 0, None, "gate_check", 2000, &e1.hmac);
        e2.metadata_json = Some(r#"{"gates_verified": true}"#.to_string());
        // Recompute HMAC since metadata isn't included in HMAC (only type, xp_delta, archetype, source, created_at)
        let state = replay_events(&[e1, e2]);
        assert_eq!(state.stage, Stage::Evolved);
    }

    #[test]
    fn stage1_gates_require_level_99() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 100);
        let state_50 = make_state_at_level(Stage::Base, 50, scores.clone());
        assert!(
            !check_stage1_gates(&state_50, 50, 30),
            "level 50 should not pass gate"
        );

        let state_99 = make_state_at_level(Stage::Base, 99, scores);
        assert!(
            check_stage1_gates(&state_99, 50, 30),
            "level 99 should pass gate"
        );
    }

    #[test]
    fn stage2_gates_require_low_correction_rate() {
        let mut scores = HashMap::new();
        scores.insert(Archetype::Ops, 200);
        let state = make_state_at_level(Stage::Evolved, 99, scores);
        assert!(
            check_stage2_gates(&state, 60, 0.10, 30),
            "low correction rate should pass"
        );
        assert!(
            !check_stage2_gates(&state, 60, 0.25, 30),
            "high correction rate should fail"
        );
    }

    #[test]
    fn derived_key_produces_different_hmacs_than_legacy() {
        let legacy = compute_event_hmac(
            EVOLUTION_HMAC_LEGACY,
            "0",
            "xp_gain",
            5,
            "ops",
            "test",
            1000,
        );
        let derived_key = b"some-derived-key-that-differs-!!";
        let derived = compute_event_hmac(derived_key, "0", "xp_gain", 5, "ops", "test", 1000);
        assert_ne!(
            legacy, derived,
            "derived key should produce different HMAC than legacy"
        );
    }
}
