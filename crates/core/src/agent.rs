use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::warn;

use crate::config::Config;
use crate::conversation::compact_history;
use crate::llm::{LlmClient, StreamEvent};
use crate::memory::{load_memory_context, read_memory, write_memory};
use crate::skills::{load_all_skills, load_skills_context, Skill};
use crate::soul::load_soul;
use crate::types::{FunctionCall, Message, ToolCall, ToolDefinition};
use tamagotchi_apply_patch::apply_patch_to_dir;
use tamagotchi_tools::registry::ToolRegistry;

pub enum AgentEvent {
    TextDelta(String),
    ToolExecuting { name: String, args: String },
    ToolResult { name: String, result: String },
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
        // Convert tool registry definitions to core types
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
        self.history.push(Message::user(user_input));
        self.run_agent_loop(event_tx).await
    }

    pub async fn run_agent_loop(&mut self, event_tx: mpsc::Sender<AgentEvent>) -> Result<()> {
        loop {
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

            // Stream the response
            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
            let messages_clone = messages.clone();
            let tools_clone = tools.map(<[ToolDefinition]>::to_vec);
            let stream_handle = {
                let llm_client = LlmClient::new(self.config.clone())?;
                tokio::spawn(async move {
                    if let Err(e) = llm_client
                        .stream_chat(&messages_clone, tools_clone.as_deref(), stream_tx)
                        .await
                    {
                        warn!("LLM stream error: {e}");
                    }
                })
            };

            // Collect the full response
            let mut text_content = String::new();
            let mut tool_calls: Vec<PartialToolCall> = Vec::new();

            while let Some(event) = stream_rx.recv().await {
                match event {
                    StreamEvent::TextDelta(delta) => {
                        text_content.push_str(&delta);
                        let _ = event_tx.send(AgentEvent::TextDelta(delta)).await;
                    }
                    StreamEvent::ToolCallDelta {
                        index,
                        id,
                        name,
                        arguments_delta,
                    } => {
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
                    StreamEvent::Done => break,
                    StreamEvent::Error(e) => {
                        let _ = event_tx.send(AgentEvent::Error(e)).await;
                        break;
                    }
                }
            }

            let _ = stream_handle.await;

            // Build assistant message
            if tool_calls.is_empty() {
                // Text-only response — we're done
                self.history.push(Message::assistant(&text_content));
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            // Has tool calls — execute them and loop
            let tc: Vec<ToolCall> = tool_calls
                .iter()
                .map(|ptc| ToolCall {
                    id: ptc.id.clone(),
                    call_type: "function".to_string(),
                    function: FunctionCall {
                        name: ptc.name.clone(),
                        arguments: ptc.arguments.clone(),
                    },
                })
                .collect();

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
            self.history.push(assistant_msg);

            // Execute each tool call
            for tool_call in &tc {
                let name = &tool_call.function.name;
                let args = &tool_call.function.arguments;

                let _ = event_tx
                    .send(AgentEvent::ToolExecuting {
                        name: name.clone(),
                        args: args.clone(),
                    })
                    .await;

                let result = self
                    .execute_tool(name, args)
                    .await
                    .unwrap_or_else(|e| format!("Error: {e}"));

                let _ = event_tx
                    .send(AgentEvent::ToolResult {
                        name: name.clone(),
                        result: result.clone(),
                    })
                    .await;

                self.history
                    .push(Message::tool_result(&tool_call.id, &result));
            }

            // Loop to get next LLM response
        }
    }

    async fn execute_tool(&mut self, name: &str, args_json: &str) -> Result<String> {
        let args: serde_json::Value =
            serde_json::from_str(args_json).context("Invalid tool arguments JSON")?;

        match name {
            "write_memory" => {
                let filename = args["filename"].as_str().context("Missing 'filename'")?;
                let content = args["content"].as_str().context("Missing 'content'")?;
                let append = args["append"].as_bool().unwrap_or(false);
                write_memory(filename, content, append)
            }
            "read_memory" => {
                let filename = args["filename"].as_str().context("Missing 'filename'")?;
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
                let patch = args["patch"].as_str().context("Missing 'patch'")?;
                let base_dir = Config::data_dir()?.join("skills");
                std::fs::create_dir_all(&base_dir)?;
                apply_patch_to_dir(patch, &base_dir)?;
                Ok("Skill patch applied successfully.".to_string())
            }
            "apply_patch" => {
                let patch = args["patch"].as_str().context("Missing 'patch'")?;
                let base_dir = Config::data_dir()?.join("tools");
                std::fs::create_dir_all(&base_dir)?;
                apply_patch_to_dir(patch, &base_dir)?;
                // Reload tool registry
                self.tool_registry = ToolRegistry::new()?;
                Ok("Patch applied successfully. Tool registry reloaded.".to_string())
            }
            "run_shell" => {
                let command = args["command"].as_str().context("Missing 'command'")?;
                let output = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .await
                    .context("Failed to execute shell command")?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                let status = output.status.code().unwrap_or(-1);

                Ok(format!(
                    "Exit code: {status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
                ))
            }
            _ => {
                // Try user tools
                self.tool_registry
                    .execute_tool(name, args_json)
                    .await
                    .with_context(|| format!("Failed to execute tool '{name}'"))
            }
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
            "Create or modify files in the tools directory using a patch DSL. Use this to create new tools.\n\nPatch format:\n*** Begin Patch\n*** Add File: <tool-name>/tool.toml\n<content>\n*** Add File: <tool-name>/main.py\n<content>\n*** End Patch",
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
            "Execute a shell command. Use with caution.",
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
            "Create or modify skill files in the skills directory using the patch DSL. Use this to create new skills.\n\nPatch format:\n*** Begin Patch\n*** Add File: <skill-name>/SKILL.md\n<content>\n*** End Patch",
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
