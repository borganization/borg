use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::constants;
use crate::secrets_resolve::SecretRef;

/// Collaboration mode that controls how the agent interacts during a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollaborationMode {
    /// Standard collaborative interaction — asks questions when needed.
    #[default]
    Default,
    /// Autonomous execution — makes assumptions, executes independently, reports progress.
    Execute,
    /// Read-only planning — explores codebase, asks questions, produces a plan, blocks mutations.
    Plan,
}

impl CollaborationMode {
    /// Returns true if this mode blocks mutating tool calls.
    pub fn blocks_mutations(&self) -> bool {
        matches!(self, Self::Plan)
    }
}

impl FromStr for CollaborationMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "default" => Ok(Self::Default),
            "execute" => Ok(Self::Execute),
            "plan" => Ok(Self::Plan),
            other => {
                anyhow::bail!("Unknown collaboration mode '{other}'. Valid: default, execute, plan")
            }
        }
    }
}

impl fmt::Display for CollaborationMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Execute => write!(f, "execute"),
            Self::Plan => write!(f, "plan"),
        }
    }
}

/// A credential value that can be either a plain env var name (legacy) or a full SecretRef.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CredentialValue {
    /// Legacy: bare string = env var name (backward compat)
    EnvVar(String),
    /// Full SecretRef (env, file, exec)
    Ref(SecretRef),
}

impl CredentialValue {
    pub fn resolve(&self) -> Result<String> {
        match self {
            CredentialValue::EnvVar(var) => {
                std::env::var(var).with_context(|| format!("Env var {var} not set"))
            }
            CredentialValue::Ref(sr) => sr.resolve(),
        }
    }
}

pub fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub headless: bool,
    pub executable: Option<String>,
    pub cdp_port: u16,
    pub no_sandbox: bool,
    pub timeout_ms: u64,
    pub startup_timeout_ms: u64,
    /// Max console log entries to buffer (default: 500).
    pub console_buffer_size: usize,
    /// Max page error entries to buffer (default: 200).
    pub error_buffer_size: usize,
    /// Max network request entries to buffer (default: 500).
    pub network_buffer_size: usize,
    /// Inner JS evaluation timeout in ms for Promise.race wrapper (default: 10000).
    pub js_eval_timeout_ms: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            headless: true,
            executable: None,
            cdp_port: 9222,
            no_sandbox: false,
            timeout_ms: 30000,
            startup_timeout_ms: 15000,
            console_buffer_size: 500,
            error_buffer_size: 200,
            network_buffer_size: 500,
            js_eval_timeout_ms: 10000,
        }
    }
}

/// Single transcription provider entry (cloud API or local CLI).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioModelConfig {
    /// Provider name: "openai", "groq", "deepgram".
    pub provider: String,
    /// Model name (e.g. "whisper-1", "nova-3").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Override API key env var for this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Language hint (BCP-47, e.g. "en").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Per-provider timeout override in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Audio transcription configuration with multi-provider fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    pub enabled: bool,
    /// Ordered fallback chain of transcription providers.
    #[serde(default)]
    pub models: Vec<AudioModelConfig>,
    /// Maximum audio file size in bytes (default: 20 MB).
    pub max_file_size: u64,
    /// Minimum audio file size in bytes (default: 1024).
    pub min_file_size: u64,
    /// Global language hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Default timeout in milliseconds.
    pub timeout_ms: u64,
    /// Echo transcript back to the sender.
    pub echo_transcript: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            models: Vec::new(),
            max_file_size: 20 * 1024 * 1024,
            min_file_size: 1024,
            language: None,
            timeout_ms: 60_000,
            echo_transcript: false,
        }
    }
}

/// Single TTS provider entry in the fallback chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsModelConfig {
    /// Provider name: "openai", "elevenlabs".
    pub provider: String,
    /// Model name (e.g. "tts-1", "tts-1-hd", "eleven_multilingual_v2").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Voice identifier (e.g. "alloy", "nova", or ElevenLabs voice ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Override API key env var for this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// Per-provider timeout override in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Text-to-speech configuration with multi-provider fallback.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub enabled: bool,
    /// Ordered fallback chain of TTS providers.
    #[serde(default)]
    pub models: Vec<TtsModelConfig>,
    /// Default voice name/ID.
    pub default_voice: String,
    /// Default output format (mp3, opus, aac, flac, wav).
    pub default_format: String,
    /// Maximum input text length in characters.
    pub max_text_length: usize,
    /// Default timeout in milliseconds.
    pub timeout_ms: u64,
    /// Auto-TTS mode: convert all gateway responses to voice.
    pub auto_mode: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            models: Vec::new(),
            default_voice: "alloy".into(),
            default_format: "mp3".into(),
            max_text_length: 4096,
            timeout_ms: 30_000,
            auto_mode: false,
        }
    }
}

/// Image compression and media processing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// Max image size in bytes after compression. Default: 6MB (matches OpenClaw).
    pub max_image_bytes: usize,
    /// Enable/disable image compression. Default: true.
    pub compression_enabled: bool,
    /// Max image dimension in pixels (longest side). Default: 2048.
    pub max_dimension_px: u32,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            max_image_bytes: 6 * 1024 * 1024,
            compression_enabled: true,
            max_dimension_px: 2048,
        }
    }
}

/// Image generation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageGenConfig {
    /// Enable image generation tools.
    pub enabled: bool,
    /// Provider override: "openai" | "fal". Auto-detects from API keys if omitted.
    pub provider: Option<String>,
    /// Model override (e.g. "dall-e-3", "fal-ai/flux/schnell").
    pub model: Option<String>,
    /// API key env var override.
    pub api_key_env: Option<String>,
    /// Default image size.
    pub default_size: String,
}

impl Default for ImageGenConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: None,
            model: None,
            api_key_env: None,
            default_size: "1024x1024".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EvolutionConfig {
    pub enabled: bool,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub default_timeout_ms: u64,
    #[serde(default)]
    pub policy: super::security::ToolPolicyConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30000,
            policy: super::security::ToolPolicyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub max_context_tokens: usize,
    #[serde(default)]
    pub embeddings: EmbeddingsConfig,
    /// When set, load memory from ~/.borg/memory/scopes/{scope}/ instead of ~/.borg/memory/.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_scope: Option<String>,
    /// When true, extract durable information from messages about to be dropped before compaction.
    pub flush_before_compaction: bool,
    /// Minimum token count of dropped messages to trigger a pre-compaction flush.
    pub flush_soft_threshold_tokens: usize,
    /// Minimum number of dropped messages to trigger a pre-compaction flush.
    pub flush_min_messages: usize,
    /// Additional directories to scan for .md files and index alongside memory.
    #[serde(default)]
    pub extra_paths: Vec<String>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 8000,
            embeddings: EmbeddingsConfig::default(),
            memory_scope: None,
            flush_before_compaction: false,
            flush_soft_threshold_tokens: 2000,
            flush_min_messages: 4,
            extra_paths: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
    pub api_key_env: Option<String>,
    pub recency_weight: f32,
    /// Target token size per chunk when generating chunked embeddings.
    pub chunk_size_tokens: usize,
    /// Overlap tokens between adjacent chunks.
    pub chunk_overlap_tokens: usize,
    /// Weight for BM25/FTS scores in hybrid search (default 0.3).
    pub bm25_weight: f32,
    /// Weight for vector similarity scores in hybrid search (default 0.7).
    pub vector_weight: f32,
    /// Enable MMR diversity re-ranking of search results.
    pub mmr_enabled: bool,
    /// MMR lambda: 1.0 = pure relevance, 0.0 = pure diversity (default 0.7).
    pub mmr_lambda: f32,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: None,
            model: None,
            dimension: None,
            api_key_env: None,
            recency_weight: 0.2,
            chunk_size_tokens: 400,
            chunk_overlap_tokens: 80,
            bm25_weight: 0.3,
            vector_weight: 0.7,
            mmr_enabled: true,
            mmr_lambda: 0.7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntryConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

impl Default for SkillEntryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            env: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    pub enabled: bool,
    pub max_context_tokens: usize,
    #[serde(default)]
    pub entries: HashMap<String, SkillEntryConfig>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_context_tokens: 4000,
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversationConfig {
    pub max_history_tokens: usize,
    pub max_iterations: u32,
    pub show_thinking: bool,
    pub tool_output_max_tokens: usize,
    pub compaction_marker_tokens: usize,
    pub max_transcript_chars: usize,
    #[serde(default)]
    pub collaboration_mode: CollaborationMode,
}

impl Default for ConversationConfig {
    fn default() -> Self {
        Self {
            max_history_tokens: 32000,
            max_iterations: 25,
            show_thinking: true,
            tool_output_max_tokens: constants::TOOL_OUTPUT_MAX_TOKENS,
            compaction_marker_tokens: constants::COMPACTION_MARKER_TOKENS,
            max_transcript_chars: constants::MAX_TRANSCRIPT_CHARS,
            collaboration_mode: CollaborationMode::Default,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub enabled: bool,
    pub search_provider: String,
    pub search_api_key_env: Option<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            search_provider: "auto".into(),
            search_api_key_env: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TasksConfig {
    pub max_concurrent: usize,
}

impl Default for TasksConfig {
    fn default() -> Self {
        Self { max_concurrent: 3 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BudgetConfig {
    /// Monthly token limit. 0 = unlimited.
    pub monthly_token_limit: u64,
    /// Fraction of budget at which to warn (0.0–1.0).
    pub warning_threshold: f64,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            monthly_token_limit: 1_000_000,
            warning_threshold: 0.8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub auto_verify: bool,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_verify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MultiAgentConfig {
    pub enabled: bool,
    pub max_spawn_depth: u32,
    pub max_children_per_agent: u32,
    pub max_concurrent: u32,
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_spawn_depth: 1,
            max_children_per_agent: 5,
            max_concurrent: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub tracing_enabled: bool,
    pub metrics_enabled: bool,
    pub otlp_endpoint: String,
    pub service_name: String,
    pub sampling_ratio: f64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            tracing_enabled: false,
            metrics_enabled: false,
            otlp_endpoint: "http://localhost:4317".to_string(),
            service_name: "borg".to_string(),
            sampling_ratio: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    /// When true, log full LLM request/response to ~/.borg/logs/debug/
    #[serde(default)]
    pub llm_logging: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptsConfig {
    /// Enable the scripts system (default: true)
    pub enabled: bool,
    /// Default sandbox profile for new scripts: "default", "trusted", or "custom"
    pub default_sandbox_profile: String,
    /// Auto-cleanup ephemeral scripts older than this (seconds, default: 86400 = 24h)
    pub ephemeral_ttl_secs: u64,
    /// Maximum number of scripts allowed (default: 100)
    pub max_scripts: usize,
    /// Default timeout for script execution in ms (default: 60000)
    pub default_timeout_ms: u64,
}

impl Default for ScriptsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_sandbox_profile: "default".to_string(),
            ephemeral_ttl_secs: 86400,
            max_scripts: 100,
            default_timeout_ms: 60000,
        }
    }
}
