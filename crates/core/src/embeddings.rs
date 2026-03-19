use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::config::{Config, EmbeddingsConfig};
use crate::db::Database;

/// Resolved embedding provider with endpoint, key, model, and dimension.
#[derive(Debug, Clone)]
pub struct EmbeddingProvider {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub dimension: usize,
}

impl EmbeddingProvider {
    /// Resolve an embedding provider from config, returning None if unavailable.
    pub fn from_config(config: &EmbeddingsConfig) -> Option<Self> {
        // If embeddings are disabled, return None
        if !config.enabled {
            debug!("Embeddings disabled in config");
            return None;
        }

        // Determine provider and API key
        let (provider_name, api_key) = if let Some(ref explicit_provider) = config.provider {
            // Explicit provider configured
            let env_var = config
                .api_key_env
                .clone()
                .unwrap_or_else(|| default_env_var(explicit_provider).to_string());
            match std::env::var(&env_var) {
                Ok(key) if !key.is_empty() => (explicit_provider.clone(), key),
                _ => {
                    debug!(
                        "Embeddings: explicit provider '{}' configured but {} not set",
                        explicit_provider, env_var
                    );
                    return None;
                }
            }
        } else {
            // Auto-detect: try OpenAI -> OpenRouter -> Gemini
            let candidates = [
                ("openai", "OPENAI_API_KEY"),
                ("openrouter", "OPENROUTER_API_KEY"),
                ("gemini", "GEMINI_API_KEY"),
            ];
            let mut found = None;
            for (name, env_var) in candidates {
                if let Ok(key) = std::env::var(env_var) {
                    if !key.is_empty() {
                        debug!("Embeddings: auto-detected provider '{name}' via {env_var}");
                        found = Some((name.to_string(), key));
                        break;
                    }
                }
            }
            match found {
                Some(pair) => pair,
                None => {
                    debug!(
                        "Embeddings: no embedding-capable provider found, falling back to recency"
                    );
                    return None;
                }
            }
        };

        let (endpoint, default_model, default_dim) = match provider_name.as_str() {
            "openai" => (
                "https://api.openai.com/v1/embeddings".to_string(),
                "text-embedding-3-small",
                1536,
            ),
            "openrouter" => (
                "https://openrouter.ai/api/v1/embeddings".to_string(),
                "openai/text-embedding-3-small",
                1536,
            ),
            "gemini" => (
                "https://generativelanguage.googleapis.com/v1beta/openai/embeddings".to_string(),
                "text-embedding-004",
                768,
            ),
            other => {
                debug!("Embeddings: unknown provider '{other}', cannot resolve endpoint");
                return None;
            }
        };

        let model = config
            .model
            .clone()
            .unwrap_or_else(|| default_model.to_string());
        let dimension = config.dimension.unwrap_or(default_dim);

        Some(Self {
            endpoint,
            api_key,
            model,
            dimension,
        })
    }
}

fn default_env_var(provider: &str) -> &str {
    match provider {
        "openai" => "OPENAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        _ => "OPENAI_API_KEY",
    }
}

/// Generate an embedding vector via OpenAI-compatible API.
pub async fn generate_embedding(
    client: &reqwest::Client,
    provider: &EmbeddingProvider,
    text: &str,
) -> Result<Vec<f32>> {
    // Truncate to ~8000 tokens (~32000 chars as rough estimate)
    let truncated = if text.len() > 32000 {
        let mut end = 32000;
        while !text.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &text[..end]
    } else {
        text
    };

    let body = serde_json::json!({
        "model": provider.model,
        "input": truncated,
    });

    let resp = client
        .post(&provider.endpoint)
        .header("Authorization", format!("Bearer {}", provider.api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        bail!("Embedding API error {status}: {text}");
    }

    let json: serde_json::Value = resp.json().await?;
    let embedding = json["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: missing data[0].embedding"))?
        .iter()
        .map(|v| v.as_f64().unwrap_or(0.0) as f32)
        .collect();

    Ok(embedding)
}

/// Cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

/// Pack f32 embedding into little-endian bytes.
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Unpack little-endian bytes into f32 embedding.
pub fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// SHA-256 hash of content.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Blended score combining similarity and recency.
pub fn blended_score(similarity: f32, recency_score: f32, recency_weight: f32) -> f32 {
    (1.0 - recency_weight) * similarity + recency_weight * recency_score
}

/// Embed a memory file and store in the database. Skips if content unchanged.
pub async fn embed_memory_file(
    config: &Config,
    filename: &str,
    content: &str,
    scope: &str,
) -> Result<()> {
    let provider = match EmbeddingProvider::from_config(&config.memory.embeddings) {
        Some(p) => p,
        None => return Ok(()),
    };

    let hash = content_hash(content);

    // Check if embedding already exists with same hash
    let db = Database::open()?;
    if let Some(existing) = db.get_embedding(scope, filename)? {
        if existing.content_hash == hash {
            debug!("Embedding for {scope}/{filename} is up to date, skipping");
            return Ok(());
        }
    }

    let client = reqwest::Client::new();
    let embedding = generate_embedding(&client, &provider, content).await?;
    let bytes = embedding_to_bytes(&embedding);

    db.upsert_embedding(
        scope,
        filename,
        &hash,
        &bytes,
        provider.dimension,
        &provider.model,
    )?;
    debug!(
        "Stored embedding for {scope}/{filename} (dim={})",
        provider.dimension
    );
    Ok(())
}

/// Generate a query embedding for ranking. Returns (provider, embedding) or None.
pub async fn generate_query_embedding(
    config: &Config,
    query: &str,
) -> Result<(EmbeddingProvider, Vec<f32>)> {
    let provider = match EmbeddingProvider::from_config(&config.memory.embeddings) {
        Some(p) => p,
        None => bail!("No embedding provider available"),
    };
    let client = reqwest::Client::new();
    let embedding = generate_embedding(&client, &provider, query).await?;
    Ok((provider, embedding))
}

/// Rank memory files by similarity to a pre-computed query embedding.
/// Returns (filename, blended_score) sorted desc.
pub fn rank_embeddings_by_similarity(
    query_embedding: &[f32],
    scope: &str,
    recency_weight: f32,
) -> Result<Vec<(String, f32)>> {
    let db = Database::open()?;
    let stored = db.get_all_embeddings(scope)?;

    if stored.is_empty() {
        return Ok(Vec::new());
    }

    // Compute recency scores: normalize created_at to [0.0, 1.0]
    let min_time = stored.iter().map(|r| r.created_at).min().unwrap_or(0);
    let max_time = stored.iter().map(|r| r.created_at).max().unwrap_or(0);
    let time_range = (max_time - min_time) as f32;

    let mut scored: Vec<(String, f32)> = stored
        .iter()
        .map(|row| {
            let stored_emb = bytes_to_embedding(&row.embedding);
            let similarity = cosine_similarity(query_embedding, &stored_emb);
            let recency_score = if time_range > 0.0 {
                (row.created_at - min_time) as f32 / time_range
            } else {
                1.0
            };
            let score = blended_score(similarity, recency_score, recency_weight);
            (row.filename.clone(), score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Ok(scored)
}

/// Convenience: rank memories by query text (generates embedding then ranks).
pub async fn rank_memories_by_similarity(
    config: &Config,
    query: &str,
    scope: &str,
) -> Result<Vec<(String, f32)>> {
    let (_provider, query_embedding) = generate_query_embedding(config, query).await?;
    rank_embeddings_by_similarity(
        &query_embedding,
        scope,
        config.memory.embeddings.recency_weight,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_similarity_empty() {
        let sim = cosine_similarity(&[], &[]);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn cosine_similarity_mismatched_lengths() {
        let sim = cosine_similarity(&[1.0, 2.0], &[1.0]);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn embedding_bytes_roundtrip() {
        let original = vec![1.0f32, -2.5, 3.14159, 0.0, f32::MIN, f32::MAX];
        let bytes = embedding_to_bytes(&original);
        let recovered = bytes_to_embedding(&bytes);
        assert_eq!(original, recovered);
    }

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_changes_with_content() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn blended_score_pure_similarity() {
        let score = blended_score(0.8, 0.5, 0.0);
        assert!((score - 0.8).abs() < 1e-6);
    }

    #[test]
    fn blended_score_pure_recency() {
        let score = blended_score(0.8, 0.5, 1.0);
        assert!((score - 0.5).abs() < 1e-6);
    }

    #[test]
    fn blended_score_balanced() {
        let score = blended_score(0.8, 0.4, 0.5);
        // 0.5 * 0.8 + 0.5 * 0.4 = 0.6
        assert!((score - 0.6).abs() < 1e-6);
    }

    #[test]
    fn blended_score_default_weight() {
        // recency_weight = 0.2
        let score = blended_score(1.0, 0.0, 0.2);
        // 0.8 * 1.0 + 0.2 * 0.0 = 0.8
        assert!((score - 0.8).abs() < 1e-6);
    }
}
