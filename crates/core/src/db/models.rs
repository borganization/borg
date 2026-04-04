/// Session metadata row from SQLite.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub total_tokens: i64,
    pub model: String,
    pub title: String,
}

/// Scheduled task row from SQLite.
#[derive(Debug, Clone)]
pub struct ScheduledTaskRow {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule_type: String,
    pub schedule_expr: String,
    pub timezone: String,
    pub status: String,
    pub next_run: Option<i64>,
    pub created_at: i64,
    pub max_retries: i32,
    pub retry_count: i32,
    pub retry_after: Option<i64>,
    pub last_error: Option<String>,
    pub timeout_ms: i64,
    pub delivery_channel: Option<String>,
    pub delivery_target: Option<String>,
    /// Comma-separated list of allowed tools for this task. None = all tools allowed.
    pub allowed_tools: Option<String>,
    /// Task type: "prompt" (LLM task) or "command" (shell cron job).
    pub task_type: String,
}

/// Task run log row from SQLite.
#[derive(Debug, Clone)]
pub struct TaskRunRow {
    pub id: i64,
    pub task_id: String,
    pub started_at: i64,
    pub duration_ms: i64,
    pub result: Option<String>,
    pub error: Option<String>,
    pub status: String,
}

/// A task that has been atomically claimed for execution, with its associated run ID.
#[derive(Debug, Clone)]
pub struct ClaimedTask {
    pub task: ScheduledTaskRow,
    pub run_id: i64,
}

/// Persisted message row from SQLite.
#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub session_id: String,
    pub role: String,
    pub content: Option<String>,
    pub content_parts_json: Option<String>,
    pub tool_calls_json: Option<String>,
    pub tool_call_id: Option<String>,
    pub timestamp: Option<String>,
    pub created_at: i64,
}

/// Delivery queue row from SQLite.
#[derive(Debug, Clone)]
pub struct DeliveryRow {
    pub id: String,
    pub channel_name: String,
    pub sender_id: String,
    pub channel_id: Option<String>,
    pub session_id: Option<String>,
    pub payload_json: String,
    pub status: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub next_retry_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub error: Option<String>,
}

/// Plugin row from SQLite.
#[derive(Debug, Clone)]
pub struct PluginRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub category: String,
    pub status: String,
    pub version: String,
    pub installed_at: i64,
    pub verified_at: Option<i64>,
}

/// Memory embedding row from SQLite.
#[derive(Debug, Clone)]
pub struct EmbeddingRow {
    pub id: i64,
    pub scope: String,
    pub filename: String,
    pub content_hash: String,
    pub embedding: Vec<u8>,
    pub dimension: usize,
    pub model: String,
    pub created_at: i64,
}

/// Chunk row from SQLite (for chunked/FTS memory search).
#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub id: i64,
    pub scope: String,
    pub filename: String,
    pub chunk_index: i64,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub content: String,
    pub content_hash: String,
    pub embedding: Option<Vec<u8>>,
    pub created_at: i64,
}

/// Input data for upserting a chunk.
#[derive(Debug, Clone)]
pub struct ChunkData {
    pub chunk_index: i64,
    pub content: String,
    pub content_hash: String,
    pub embedding: Option<Vec<u8>>,
    pub dimension: Option<usize>,
    pub model: Option<String>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
}

/// Pairing request row from SQLite.
#[derive(Debug, Clone)]
pub struct PairingRequestRow {
    pub id: String,
    pub channel_name: String,
    pub sender_id: String,
    pub code: String,
    pub status: String,
    pub display_name: Option<String>,
    pub created_at: i64,
    pub expires_at: i64,
    pub approved_at: Option<i64>,
}

/// Approved sender row from SQLite.
#[derive(Debug, Clone)]
pub struct ApprovedSenderRow {
    pub id: i64,
    pub channel_name: String,
    pub sender_id: String,
    pub display_name: Option<String>,
    pub approved_at: i64,
}

/// Agent role row from SQLite.
#[derive(Debug, Clone)]
pub struct AgentRoleRow {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub temperature: Option<f32>,
    pub system_instructions: Option<String>,
    pub tools_allowed: Option<String>,
    pub max_iterations: Option<i64>,
    pub is_builtin: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Sub-agent run row from SQLite.
#[derive(Debug, Clone)]
pub struct SubAgentRunRow {
    pub id: String,
    pub nickname: String,
    pub role: String,
    pub parent_session_id: String,
    pub session_id: String,
    pub depth: u32,
    pub status: String,
    pub result_text: Option<String>,
    pub error_text: Option<String>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug)]
pub struct ModelUsageRow {
    pub provider: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub total_cost_usd: Option<f64>,
}

/// Parameters for creating a new scheduled task.
pub struct NewTask<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub prompt: &'a str,
    pub schedule_type: &'a str,
    pub schedule_expr: &'a str,
    pub timezone: &'a str,
    pub next_run: Option<i64>,
    pub max_retries: Option<i32>,
    pub timeout_ms: Option<i64>,
    pub delivery_channel: Option<&'a str>,
    pub delivery_target: Option<&'a str>,
    /// Comma-separated tool allowlist. None = all tools allowed.
    pub allowed_tools: Option<&'a str>,
    /// Task type: "prompt" (LLM task) or "command" (shell cron job). Defaults to "prompt".
    pub task_type: &'a str,
}

/// Parameters for enqueuing a delivery.
pub struct NewDelivery<'a> {
    pub id: &'a str,
    pub channel_name: &'a str,
    pub sender_id: &'a str,
    pub channel_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub payload_json: &'a str,
    pub max_retries: i32,
}

/// Parameters for updating an existing scheduled task. `None` fields are left unchanged.
pub struct UpdateTask<'a> {
    pub name: Option<&'a str>,
    pub prompt: Option<&'a str>,
    pub schedule_type: Option<&'a str>,
    pub schedule_expr: Option<&'a str>,
    pub timezone: Option<&'a str>,
}

/// Script row from SQLite.
#[derive(Debug, Clone)]
pub struct ScriptRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub runtime: String,
    pub entrypoint: String,
    pub sandbox_profile: String,
    pub network_access: bool,
    pub fs_read: String,
    pub fs_write: String,
    pub ephemeral: bool,
    pub hmac: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_run_at: Option<i64>,
    pub run_count: i64,
}

/// Parameters for creating a new script.
pub struct NewScript<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub runtime: &'a str,
    pub entrypoint: &'a str,
    pub sandbox_profile: &'a str,
    pub network_access: bool,
    pub fs_read: &'a str,
    pub fs_write: &'a str,
    pub ephemeral: bool,
    pub hmac: &'a str,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Health report for all event-sourced HMAC chains.
#[derive(Debug)]
pub struct ChainHealth {
    pub vitals_valid: bool,
    pub vitals_count: u32,
    pub bond_valid: bool,
    pub bond_count: u32,
    pub evolution_valid: bool,
    pub evolution_count: u32,
}
