use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::config::Config;
use crate::conversation::{compact_history, history_tokens, normalize_history, undo_last_turn};
use crate::llm::{LlmClient, StreamEvent};
use crate::logging::log_message;
use crate::memory::{load_memory_context, read_memory, write_memory};
use crate::skills::{load_all_skills, load_skills_context, Skill};
use crate::soul::load_soul;
use crate::truncate::truncate_output;
use crate::types::{FunctionCall, Message, ToolCall, ToolDefinition};
use tamagotchi_apply_patch::apply_patch_to_dir;
use tamagotchi_tools::registry::ToolRegistry;

/// Max tokens for tool output before truncation (head + tail preserved).
const TOOL_OUTPUT_MAX_TOKENS: usize = 4000;

pub enum AgentEvent {
    TextDelta(String),
    ToolExecuting {
        name: String,
        args: String,
    },
    ToolResult {
        name: String,
        result: String,
    },
    /// Request confirmation from the user for a shell command. Send `true` to approve.
    ShellConfirmation {
        command: String,
        respond: oneshot::Sender<bool>,
    },
    TurnComplete,
    Error(String),
}

pub struct Agent {
    config: Config,
    _llm: LlmClient,
    history: Vec<Message>,
    tool_registry: ToolRegistry,
}

impl Agent {
    pub fn new(config: Config) -> Result<Self> {
        let _llm = LlmClient::new(config.clone())?;
        let tool_registry = ToolRegistry::new()?;
        Ok(Self {
            config,
            _llm,
            history: Vec::new(),
            tool_registry,
        })
    }

    pub fn history(&self) -> &[Message] {
        &self.history
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut Config {
        &mut self.config
    }

    /// Compact conversation history, returning (before_tokens, after_tokens).
    pub fn compact(&mut self) -> (usize, usize) {
        let before = history_tokens(&self.history);
        compact_history(
            &mut self.history,
            self.config.conversation.max_history_tokens,
        );
        let after = history_tokens(&self.history);
        (before, after)
    }

    /// Clear all conversation history.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Undo the last agent turn: remove everything back to the last user message.
    /// Returns the number of messages removed, or 0 if nothing to undo.
    pub fn undo(&mut self) -> usize {
        undo_last_turn(&mut self.history)
    }

    /// Returns (message_count, estimated_token_count) for the current session.
    pub fn conversation_stats(&self) -> (usize, usize) {
        (self.history.len(), history_tokens(&self.history))
    }

    fn build_system_prompt(&self) -> Result<String> {
        let soul = load_soul()?;
        let memory = load_memory_context(self.config.memory.max_context_tokens)?;
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");

        let mut system = format!("{soul}\n\n# Current Time\n{now}\n");

        if !memory.is_empty() {
            system.push_str(&format!("\n{memory}\n"));
        }

        if self.config.skills.enabled {
            let skills = load_skills_context(self.config.skills.max_context_tokens)?;
            if !skills.is_empty() {
                system.push_str(&format!("\n{skills}\n"));
            }
        }

        Ok(system)
    }

    fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut tools = core_tool_definitions();
        for td in self.tool_registry.tool_definitions() {
            tools.push(ToolDefinition::new(
                &td.function.name,
                &td.function.description,
                td.function.parameters.clone(),
            ));
        }
        tools
    }

    pub async fn send_message(
        &mut self,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
    ) -> Result<()> {
        self.send_message_with_cancel(user_input, event_tx, CancellationToken::new())
            .await
    }

    pub async fn send_message_with_cancel(
        &mut self,
        user_input: &str,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let msg = Message::user(user_input);
        log_message(&msg);
        self.history.push(msg);
        self.run_agent_loop(event_tx, cancel).await
    }

    pub async fn run_agent_loop(
        &mut self,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<()> {
        loop {
            if cancel.is_cancelled() {
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            // Normalize history to fix structural invariants (missing/orphaned tool results)
            normalize_history(&mut self.history);

            // Compact history if it exceeds the token budget
            compact_history(
                &mut self.history,
                self.config.conversation.max_history_tokens,
            );

            let system_prompt = self.build_system_prompt()?;
            let tool_defs = self.build_tool_definitions();

            let mut messages = vec![Message::system(&system_prompt)];
            messages.extend(self.history.clone());

            let tools = if tool_defs.is_empty() {
                None
            } else {
                Some(tool_defs.as_slice())
            };

            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
            let messages_clone = messages.clone();
            let tools_clone = tools.map(<[ToolDefinition]>::to_vec);
            let cancel_clone = cancel.clone();
            let stream_handle = {
                let llm_client = LlmClient::new(self.config.clone())?;
                tokio::spawn(async move {
                    if let Err(e) = llm_client
                        .stream_chat_with_cancel(
                            &messages_clone,
                            tools_clone.as_deref(),
                            stream_tx,
                            cancel_clone,
                        )
                        .await
                    {
                        warn!("LLM stream error: {e}");
                    }
                })
            };

            let mut text_content = String::new();
            let mut tool_calls: Vec<PartialToolCall> = Vec::new();

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        // Append whatever we have as an interrupted message
                        if !text_content.is_empty() {
                            let mut content = text_content.clone();
                            content.push_str("\n\n[response interrupted]");
                            let msg = Message::assistant(&content);
                            log_message(&msg);
                            self.history.push(msg);
                        }
                        let _ = event_tx.send(AgentEvent::TurnComplete).await;
                        let _ = stream_handle.await;
                        return Ok(());
                    }
                    event = stream_rx.recv() => {
                        match event {
                            Some(StreamEvent::TextDelta(delta)) => {
                                text_content.push_str(&delta);
                                let _ = event_tx.send(AgentEvent::TextDelta(delta)).await;
                            }
                            Some(StreamEvent::ToolCallDelta {
                                index,
                                id,
                                name,
                                arguments_delta,
                            }) => {
                                while tool_calls.len() <= index {
                                    tool_calls.push(PartialToolCall::default());
                                }
                                if let Some(id) = id {
                                    tool_calls[index].id = id;
                                }
                                if let Some(name) = name {
                                    tool_calls[index].name = name;
                                }
                                tool_calls[index].arguments.push_str(&arguments_delta);
                            }
                            Some(StreamEvent::Done) => break,
                            Some(StreamEvent::Error(e)) => {
                                let _ = event_tx.send(AgentEvent::Error(e)).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = stream_handle.await;

            if tool_calls.is_empty() {
                let msg = Message::assistant(&text_content);
                log_message(&msg);
                self.history.push(msg);
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            // Validate tool call JSON; drop incomplete ones with a warning
            let tc: Vec<ToolCall> = tool_calls
                .iter()
                .filter(|ptc| {
                    if ptc.name.is_empty() || ptc.id.is_empty() {
                        warn!("Dropping incomplete tool call (missing name or id)");
                        return false;
                    }
                    if serde_json::from_str::<serde_json::Value>(&ptc.arguments).is_err() {
                        warn!(
                            "Dropping tool call '{}' with incomplete JSON arguments",
                            ptc.name
                        );
                        return false;
                    }
                    true
                })
                .map(|ptc| ToolCall {
                    id: ptc.id.clone(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: ptc.name.clone(),
                        arguments: ptc.arguments.clone(),
                    },
                })
                .collect();

            if tc.is_empty() {
                // All tool calls were incomplete — treat as text-only response
                let content = if text_content.is_empty() {
                    "[response interrupted — incomplete tool calls discarded]".to_string()
                } else {
                    format!("{text_content}\n\n[incomplete tool calls discarded]")
                };
                let msg = Message::assistant(&content);
                log_message(&msg);
                self.history.push(msg);
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            let assistant_msg = if text_content.is_empty() {
                Message {
                    role: crate::types::Role::Assistant,
                    content: None,
                    tool_calls: Some(tc.clone()),
                    tool_call_id: None,
                }
            } else {
                Message {
                    role: crate::types::Role::Assistant,
                    content: Some(text_content.clone()),
                    tool_calls: Some(tc.clone()),
                    tool_call_id: None,
                }
            };
            log_message(&assistant_msg);
            self.history.push(assistant_msg);

            for tool_call in &tc {
                if cancel.is_cancelled() {
                    // Synthesize results for remaining tool calls
                    let remaining_msg =
                        Message::tool_result(&tool_call.id, "[tool call cancelled by user]");
                    log_message(&remaining_msg);
                    self.history.push(remaining_msg);
                    continue;
                }

                let name = &tool_call.function.name;
                let args = &tool_call.function.arguments;

                let _ = event_tx
                    .send(AgentEvent::ToolExecuting {
                        name: name.clone(),
                        args: args.clone(),
                    })
                    .await;

                let raw_result = self
                    .execute_tool(name, args, &event_tx)
                    .await
                    .unwrap_or_else(|e| format!("Error: {e}"));

                // Truncate large tool outputs to prevent blowing the context window
                let result = truncate_output(&raw_result, TOOL_OUTPUT_MAX_TOKENS);

                let _ = event_tx
                    .send(AgentEvent::ToolResult {
                        name: name.clone(),
                        result: result.clone(),
                    })
                    .await;

                let msg = Message::tool_result(&tool_call.id, &result);
                log_message(&msg);
                self.history.push(msg);
            }
        }
    }

    async fn execute_tool(
        &mut self,
        name: &str,
        args_json: &str,
        event_tx: &mpsc::Sender<AgentEvent>,
    ) -> Result<String> {
        let args: serde_json::Value = match serde_json::from_str(args_json) {
            Ok(v) => v,
            Err(e) => {
                return Ok(format!(
                    "Error: Invalid JSON arguments: {e}. Please provide valid JSON."
                ));
            }
        };

        match name {
            "write_memory" => {
                let Some(filename) = args["filename"].as_str() else {
                    return Ok("Error: Missing required parameter 'filename'.".to_string());
                };
                let Some(content) = args["content"].as_str() else {
                    return Ok("Error: Missing required parameter 'content'.".to_string());
                };
                let append = args["append"].as_bool().unwrap_or(false);
                write_memory(filename, content, append)
            }
            "read_memory" => {
                let Some(filename) = args["filename"].as_str() else {
                    return Ok("Error: Missing required parameter 'filename'.".to_string());
                };
                read_memory(filename)
            }
            "list_tools" => {
                let tools = self.tool_registry.list_tools();
                Ok(if tools.is_empty() {
                    "No user tools installed.".to_string()
                } else {
                    tools.join("\n")
                })
            }
            "list_skills" => {
                let skills = load_all_skills()?;
                if skills.is_empty() {
                    Ok("No skills installed.".to_string())
                } else {
                    Ok(skills
                        .iter()
                        .map(Skill::summary_line)
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }
            "apply_skill_patch" => {
                let Some(patch) = args["patch"].as_str() else {
                    return Ok("Error: Missing required parameter 'patch'.".to_string());
                };
                let base_dir = Config::data_dir()?.join("skills");
                std::fs::create_dir_all(&base_dir)?;
                match apply_patch_to_dir(patch, &base_dir) {
                    Ok(_) => Ok("Skill patch applied successfully.".to_string()),
                    Err(e) => Ok(format!("Error applying skill patch: {e}")),
                }
            }
            "apply_patch" => {
                let Some(patch) = args["patch"].as_str() else {
                    return Ok("Error: Missing required parameter 'patch'.".to_string());
                };
                let base_dir = Config::data_dir()?.join("tools");
                std::fs::create_dir_all(&base_dir)?;
                match apply_patch_to_dir(patch, &base_dir) {
                    Ok(_) => {
                        self.tool_registry = ToolRegistry::new()?;
                        Ok("Patch applied successfully. Tool registry reloaded.".to_string())
                    }
                    Err(e) => Ok(format!("Error applying patch: {e}")),
                }
            }
            "run_shell" => {
                let command = args["command"].as_str().context("Missing 'command'")?;
                let timeout_ms = self.config.tools.default_timeout_ms;
                let timeout_dur = Duration::from_millis(timeout_ms);

                // Request confirmation from the user
                let (confirm_tx, confirm_rx) = oneshot::channel();
                let _ = event_tx
                    .send(AgentEvent::ShellConfirmation {
                        command: command.to_string(),
                        respond: confirm_tx,
                    })
                    .await;

                match confirm_rx.await {
                    Ok(true) => {}
                    Ok(false) => {
                        return Ok("Shell command denied by user.".to_string());
                    }
                    Err(_) => {
                        return Ok("Shell command cancelled (no response).".to_string());
                    }
                }

                let child = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output();

                match tokio::time::timeout(timeout_dur, child).await {
                    Ok(Ok(output)) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let status = output.status.code().unwrap_or(-1);

                        Ok(format!(
                            "Exit code: {status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
                        ))
                    }
                    Ok(Err(e)) => Err(anyhow::anyhow!("Failed to execute shell command: {e}")),
                    Err(_) => Ok(format!(
                        "Error: command timed out after {timeout_ms}ms\nCommand: {command}"
                    )),
                }
            }
            _ => match self.tool_registry.execute_tool(name, args_json).await {
                Ok(result) => Ok(result),
                Err(e) => Ok(format!("Error executing tool '{name}': {e}")),
            },
        }
    }
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

fn core_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "write_memory",
            "Write or append to a memory file. Use filename 'SOUL.md' to update personality, 'MEMORY.md' for the index, or any other name for topic-specific memories.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "filename": {
                        "type": "string",
                        "description": "Name of the memory file (e.g., 'MEMORY.md', 'SOUL.md', 'user_preferences.md')"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    },
                    "append": {
                        "type": "boolean",
                        "description": "If true, append to existing file instead of overwriting",
                        "default": false
                    }
                },
                "required": ["filename", "content"]
            }),
        ),
        ToolDefinition::new(
            "read_memory",
            "Read a memory file.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "filename": {
                        "type": "string",
                        "description": "Name of the memory file to read"
                    }
                },
                "required": ["filename"]
            }),
        ),
        ToolDefinition::new(
            "list_tools",
            "List all available user-created tools.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "apply_patch",
            "Create or modify files in the tools directory using a patch DSL. Use this to create new tools.\n\nPatch format:\n*** Begin Patch\n*** Add File: <tool-name>/tool.toml\n+<line1>\n+<line2>\n*** Add File: <tool-name>/main.py\n+<line1>\n+<line2>\n*** Update File: <tool-name>/main.py\n@@\n context\n-old line\n+new line\n*** Delete File: <tool-name>/old.py\n*** End Patch\n\nIMPORTANT: Every content line in Add File MUST start with '+'. Update File lines must start with ' ' (context), '-' (remove), or '+' (add).",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "The patch content in the patch DSL format"
                    }
                },
                "required": ["patch"]
            }),
        ),
        ToolDefinition::new(
            "run_shell",
            "Execute a shell command. Requires user confirmation before execution.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute"
                    }
                },
                "required": ["command"]
            }),
        ),
        ToolDefinition::new(
            "list_skills",
            "List all available skills with their status and source.",
            serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        ),
        ToolDefinition::new(
            "apply_skill_patch",
            "Create or modify skill files in the skills directory using the patch DSL. Use this to create new skills.\n\nPatch format:\n*** Begin Patch\n*** Add File: <skill-name>/SKILL.md\n+<line1>\n+<line2>\n*** End Patch\n\nIMPORTANT: Every content line in Add File MUST start with '+'.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "patch": {
                        "type": "string",
                        "description": "The patch content in the patch DSL format"
                    }
                },
                "required": ["patch"]
            }),
        ),
    ]
}
