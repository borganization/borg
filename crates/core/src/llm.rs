use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::config::Config;
use crate::types::{Message, ToolDefinition};

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

#[derive(Debug)]
pub enum StreamEvent {
    TextDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Done,
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    temperature: f32,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Option<Vec<StreamChoice>>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<Delta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Delta {
    content: Option<String>,
    tool_calls: Option<Vec<DeltaToolCall>>,
}

#[derive(Debug, Deserialize)]
struct DeltaToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct DeltaFunction {
    name: Option<String>,
    arguments: Option<String>,
}

pub struct LlmClient {
    client: Client,
    config: Config,
    api_key: String,
}

impl LlmClient {
    pub fn new(config: Config) -> Result<Self> {
        let api_key = config.api_key()?;
        let client = Client::new();
        Ok(Self {
            client,
            config,
            api_key,
        })
    }

    pub async fn stream_chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        let request = ChatRequest {
            model: self.config.llm.model.clone(),
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: true,
        };

        debug!("Sending request to OpenRouter (model: {})", request.model);

        let response = self
            .client
            .post(OPENROUTER_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/tamagotchi-ai")
            .header("X-Title", "Tamagotchi AI Assistant")
            .json(&request)
            .send()
            .await
            .context("Failed to connect to OpenRouter")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenRouter returned {status}: {body}");
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream read error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if data.trim() == "[DONE]" {
                        let _ = tx.send(StreamEvent::Done).await;
                        return Ok(());
                    }

                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            if let Some(choices) = chunk.choices {
                                for choice in choices {
                                    if let Some(delta) = choice.delta {
                                        if let Some(content) = delta.content {
                                            let _ = tx.send(StreamEvent::TextDelta(content)).await;
                                        }
                                        if let Some(tool_calls) = delta.tool_calls {
                                            for tc in tool_calls {
                                                let _ = tx
                                                    .send(StreamEvent::ToolCallDelta {
                                                        index: tc.index.unwrap_or(0),
                                                        id: tc.id,
                                                        name: tc
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.name.clone()),
                                                        arguments_delta: tc
                                                            .function
                                                            .as_ref()
                                                            .and_then(|f| f.arguments.clone())
                                                            .unwrap_or_default(),
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                    if choice.finish_reason.is_some() {
                                        let _ = tx.send(StreamEvent::Done).await;
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to parse SSE chunk: {e}");
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done).await;
        Ok(())
    }

    /// Non-streaming call for heartbeat and simple requests
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        let request = ChatRequest {
            model: self.config.llm.model.clone(),
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: false,
        };

        let response = self
            .client
            .post(OPENROUTER_URL)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .header("HTTP-Referer", "https://github.com/tamagotchi-ai")
            .header("X-Title", "Tamagotchi AI Assistant")
            .json(&request)
            .send()
            .await
            .context("Failed to connect to OpenRouter")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenRouter returned {status}: {body}");
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<ChatChoice>,
        }
        #[derive(Deserialize)]
        struct ChatChoice {
            message: Message,
        }

        let resp: ChatResponse = response.json().await?;
        resp.choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("No response from LLM")
    }
}
