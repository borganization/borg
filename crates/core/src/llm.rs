use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::config::Config;
use crate::provider::Provider;
use crate::types::{Message, Role, ToolCall, ToolDefinition};

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

// ── OpenAI-compatible request/response types ──

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
    provider: Provider,
    api_key: String,
}

impl LlmClient {
    pub fn new(config: Config) -> Result<Self> {
        let (provider, api_key) = config.resolve_provider()?;
        let client = Client::new();
        Ok(Self {
            client,
            config,
            provider,
            api_key,
        })
    }

    pub async fn stream_chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        if self.provider.is_openai_compatible() {
            self.stream_chat_openai(messages, tools, tx).await
        } else {
            self.stream_chat_anthropic(messages, tools, tx).await
        }
    }

    /// Non-streaming call for heartbeat and simple requests
    pub async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        if self.provider.is_openai_compatible() {
            self.chat_openai(messages, tools).await
        } else {
            self.chat_anthropic(messages, tools).await
        }
    }

    // ── OpenAI-compatible path ──

    async fn stream_chat_openai(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        let model = self.provider.normalize_model(&self.config.llm.model);
        let request = ChatRequest {
            model: model.clone(),
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: true,
        };

        debug!(
            "Sending streaming request to {} (model: {})",
            self.provider, model
        );

        let response = self
            .client
            .post(self.provider.base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to connect to {}", self.provider))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("{} returned {status}: {body}", self.provider);
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream read error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

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

    async fn chat_openai(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        let model = self.provider.normalize_model(&self.config.llm.model);
        let request = ChatRequest {
            model,
            messages: messages.to_vec(),
            tools: tools.map(<[ToolDefinition]>::to_vec),
            temperature: self.config.llm.temperature,
            max_tokens: self.config.llm.max_tokens,
            stream: false,
        };

        let response = self
            .client
            .post(self.provider.base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Failed to connect to {}", self.provider))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("{} returned {status}: {body}", self.provider);
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

    // ── Anthropic path ──

    fn build_anthropic_request(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        stream: bool,
    ) -> serde_json::Value {
        let model = self.provider.normalize_model(&self.config.llm.model);

        // Extract system message
        let system_text: Option<String> = messages
            .iter()
            .find(|m| m.role == Role::System)
            .and_then(|m| m.content.clone());

        // Convert messages (skip system)
        let anthropic_messages = build_anthropic_messages(messages);

        // Convert tools to Anthropic format
        let anthropic_tools: Option<Vec<serde_json::Value>> = tools.map(|ts| {
            ts.iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect()
        });

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": self.config.llm.max_tokens,
            "temperature": self.config.llm.temperature,
            "stream": stream,
        });

        if let Some(sys) = system_text {
            body["system"] = serde_json::json!(sys);
        }
        body["messages"] = serde_json::json!(anthropic_messages);
        if let Some(tools) = anthropic_tools {
            body["tools"] = serde_json::json!(tools);
        }

        body
    }

    async fn stream_chat_anthropic(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        let body = self.build_anthropic_request(messages, tools, true);
        let model = self.provider.normalize_model(&self.config.llm.model);

        debug!("Sending streaming request to Anthropic (model: {})", model);

        let response = self
            .client
            .post(self.provider.base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Anthropic")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Anthropic returned {status}: {body}");
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_tool_index: usize = 0;
        let mut current_block_is_tool = false;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Stream read error")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                // Anthropic SSE uses "event: <type>" followed by "data: <json>"
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        let event_type = event["type"].as_str().unwrap_or("");

                        match event_type {
                            "content_block_start" => {
                                let block = &event["content_block"];
                                if block["type"].as_str() == Some("tool_use") {
                                    current_block_is_tool = true;
                                    let id = block["id"].as_str().map(String::from);
                                    let name = block["name"].as_str().map(String::from);
                                    let _ = tx
                                        .send(StreamEvent::ToolCallDelta {
                                            index: current_tool_index,
                                            id,
                                            name,
                                            arguments_delta: String::new(),
                                        })
                                        .await;
                                } else {
                                    current_block_is_tool = false;
                                }
                            }
                            "content_block_delta" => {
                                let delta = &event["delta"];
                                match delta["type"].as_str() {
                                    Some("text_delta") => {
                                        if let Some(text) = delta["text"].as_str() {
                                            let _ = tx
                                                .send(StreamEvent::TextDelta(text.to_string()))
                                                .await;
                                        }
                                    }
                                    Some("input_json_delta") => {
                                        if let Some(json_delta) = delta["partial_json"].as_str() {
                                            let _ = tx
                                                .send(StreamEvent::ToolCallDelta {
                                                    index: current_tool_index,
                                                    id: None,
                                                    name: None,
                                                    arguments_delta: json_delta.to_string(),
                                                })
                                                .await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "content_block_stop" => {
                                if current_block_is_tool {
                                    current_tool_index += 1;
                                }
                            }
                            "message_stop" => {
                                let _ = tx.send(StreamEvent::Done).await;
                                return Ok(());
                            }
                            "message_delta" => {
                                // message_delta with stop_reason indicates end
                                if event["delta"]["stop_reason"].as_str().is_some() {
                                    let _ = tx.send(StreamEvent::Done).await;
                                    return Ok(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done).await;
        Ok(())
    }

    async fn chat_anthropic(
        &self,
        messages: &[Message],
        tools: Option<&[ToolDefinition]>,
    ) -> Result<Message> {
        let body = self.build_anthropic_request(messages, tools, false);

        let response = self
            .client
            .post(self.provider.base_url())
            .headers(self.provider.build_headers(&self.api_key)?)
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Anthropic")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Anthropic returned {status}: {body}");
        }

        let resp: serde_json::Value = response.json().await?;
        parse_anthropic_response(&resp)
    }
}

// ── Anthropic message conversion helpers ──

/// Convert internal Messages to Anthropic format.
fn build_anthropic_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    let mut result: Vec<serde_json::Value> = Vec::new();

    for msg in messages {
        if msg.role == Role::System {
            continue;
        }

        match msg.role {
            Role::User => {
                result.push(serde_json::json!({
                    "role": "user",
                    "content": msg.content.as_deref().unwrap_or(""),
                }));
            }
            Role::Assistant => {
                let mut content_blocks: Vec<serde_json::Value> = Vec::new();

                if let Some(ref text) = msg.content {
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }

                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                            .unwrap_or(serde_json::json!({}));
                        content_blocks.push(serde_json::json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.function.name,
                            "input": args,
                        }));
                    }
                }

                if content_blocks.is_empty() {
                    content_blocks.push(serde_json::json!({
                        "type": "text",
                        "text": "",
                    }));
                }

                result.push(serde_json::json!({
                    "role": "assistant",
                    "content": content_blocks,
                }));
            }
            Role::Tool => {
                let tool_result_block = serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""),
                    "content": msg.content.as_deref().unwrap_or(""),
                });

                // Anthropic requires tool results in a user message. Merge with previous
                // user message if the last message is already a user message with tool results.
                if let Some(last) = result.last_mut() {
                    if last["role"].as_str() == Some("user") {
                        if let Some(arr) = last["content"].as_array_mut() {
                            arr.push(tool_result_block);
                            continue;
                        }
                    }
                }

                result.push(serde_json::json!({
                    "role": "user",
                    "content": [tool_result_block],
                }));
            }
            Role::System => unreachable!(),
        }
    }

    result
}

/// Parse Anthropic non-streaming response into internal Message.
fn parse_anthropic_response(resp: &serde_json::Value) -> Result<Message> {
    let content_blocks = resp["content"]
        .as_array()
        .with_context(|| format!("Missing 'content' array in Anthropic response: {resp}"))?;

    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in content_blocks {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(text) = block["text"].as_str() {
                    text_parts.push(text.to_string());
                }
            }
            Some("tool_use") => {
                let id = block["id"].as_str().unwrap_or("").to_string();
                let name = block["name"].as_str().unwrap_or("").to_string();
                let input = block["input"].clone();
                tool_calls.push(ToolCall {
                    id,
                    call_type: "function".to_string(),
                    function: crate::types::FunctionCall {
                        name,
                        arguments: serde_json::to_string(&input).unwrap_or_default(),
                    },
                });
            }
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    Ok(Message {
        role: Role::Assistant,
        content,
        tool_calls,
        tool_call_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, ToolCall, ToolDefinition};

    /// Helper to create an LlmClient with Anthropic provider for testing.
    /// Uses a unique env var to avoid races with parallel tests.
    fn make_test_anthropic_client() -> LlmClient {
        let env_var = "TAMAGOTCHI_TEST_ANTHROPIC_LLM";
        std::env::set_var(env_var, "sk-test");
        let mut config = Config::default();
        config.llm.provider = Some("anthropic".to_string());
        config.llm.api_key_env = env_var.to_string();
        config.llm.model = "claude-sonnet-4".to_string();
        LlmClient::new(config).unwrap()
    }

    #[test]
    fn anthropic_system_extraction() {
        let messages = vec![Message::system("You are helpful."), Message::user("Hello")];

        let client = make_test_anthropic_client();
        let body = client.build_anthropic_request(&messages, None, false);

        assert_eq!(body["system"].as_str(), Some("You are helpful."));

        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn anthropic_tool_result_conversion() {
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: "read_memory".to_string(),
                        arguments: r#"{"filename":"test.md"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            Message::tool_result("call_1", "file contents here"),
        ];

        let anthropic_msgs = build_anthropic_messages(&messages);

        // Should have: user, assistant, user (with tool_result)
        assert_eq!(anthropic_msgs.len(), 3);

        // Assistant message should have tool_use block
        let assistant = &anthropic_msgs[1];
        let content = assistant["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_use");
        assert_eq!(content[0]["name"], "read_memory");

        // Tool result should be in a user message
        let tool_user = &anthropic_msgs[2];
        assert_eq!(tool_user["role"], "user");
        let tool_content = tool_user["content"].as_array().unwrap();
        assert_eq!(tool_content[0]["type"], "tool_result");
        assert_eq!(tool_content[0]["tool_use_id"], "call_1");
    }

    #[test]
    fn anthropic_tool_definition_conversion() {
        let tools = vec![ToolDefinition::new(
            "test_tool",
            "A test tool",
            serde_json::json!({"type": "object", "properties": {"x": {"type": "string"}}}),
        )];

        let client = make_test_anthropic_client();

        let body = client.build_anthropic_request(&[Message::user("hi")], Some(&tools), false);

        let api_tools = body["tools"].as_array().unwrap();
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0]["name"], "test_tool");
        assert_eq!(api_tools[0]["description"], "A test tool");
        assert!(api_tools[0]["input_schema"].is_object());
        // Should NOT have "function" wrapping or "type":"function"
        assert!(api_tools[0]["function"].is_null());
    }

    #[test]
    fn anthropic_adjacent_tool_results_merge() {
        let messages = vec![
            Message::user("test"),
            Message {
                role: Role::Assistant,
                content: None,
                tool_calls: Some(vec![
                    ToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: "tool_a".to_string(),
                            arguments: "{}".to_string(),
                        },
                    },
                    ToolCall {
                        id: "call_2".to_string(),
                        call_type: "function".to_string(),
                        function: FunctionCall {
                            name: "tool_b".to_string(),
                            arguments: "{}".to_string(),
                        },
                    },
                ]),
                tool_call_id: None,
            },
            Message::tool_result("call_1", "result a"),
            Message::tool_result("call_2", "result b"),
        ];

        let anthropic_msgs = build_anthropic_messages(&messages);

        // user, assistant, user (merged tool results)
        assert_eq!(anthropic_msgs.len(), 3);

        let tool_user = &anthropic_msgs[2];
        let tool_content = tool_user["content"].as_array().unwrap();
        assert_eq!(tool_content.len(), 2);
        assert_eq!(tool_content[0]["tool_use_id"], "call_1");
        assert_eq!(tool_content[1]["tool_use_id"], "call_2");
    }

    #[test]
    fn parse_anthropic_response_text_only() {
        let resp = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello there!"}
            ],
            "stop_reason": "end_turn"
        });

        let msg = parse_anthropic_response(&resp).unwrap();
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.as_deref(), Some("Hello there!"));
        assert!(msg.tool_calls.is_none());
    }

    #[test]
    fn parse_anthropic_response_with_tool_use() {
        let resp = serde_json::json!({
            "content": [
                {"type": "text", "text": "Let me check."},
                {
                    "type": "tool_use",
                    "id": "toolu_123",
                    "name": "read_memory",
                    "input": {"filename": "test.md"}
                }
            ],
            "stop_reason": "tool_use"
        });

        let msg = parse_anthropic_response(&resp).unwrap();
        assert_eq!(msg.content.as_deref(), Some("Let me check."));
        let tcs = msg.tool_calls.unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "toolu_123");
        assert_eq!(tcs[0].function.name, "read_memory");
        assert!(tcs[0].function.arguments.contains("test.md"));
    }
}
