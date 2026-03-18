use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUnderstandingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
}

fn default_true() -> bool {
    true
}

fn default_concurrency() -> usize {
    2
}

impl Default for MediaUnderstandingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            concurrency: 2,
        }
    }
}
