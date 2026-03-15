use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::config::Config;
use crate::conversation::{compact_history, history_tokens, normalize_history, undo_last_turn};
use crate::db::Database;
use crate::hooks::{HookAction, HookContext, HookData, HookPoint, HookRegistry};
use crate::llm::{LlmClient, StreamEvent, UsageData};
use crate::logging::log_message;
use crate::memory::{load_memory_context, read_memory, write_memory_scoped};
use crate::policy::ExecutionPolicy;
use crate::secrets::redact_secrets;
use crate::session::Session;
use crate::skills::{load_all_skills, load_skills_context, Skill};
use crate::soul::load_soul;
use crate::tasks;
use crate::truncate::truncate_output;
use crate::types::{FunctionCall, Message, ToolCall, ToolDefinition};
use crate::web;
use tamagotchi_apply_patch::apply_patch_to_dir;
use tamagotchi_tools::registry::ToolRegistry;

/// Max tokens for tool output before truncation (head + tail preserved).
const TOOL_OUTPUT_MAX_TOKENS: usize = 4000;

/// Strip `<internal>...</internal>` blocks from text to prevent chain-of-thought leakage.
fn strip_internal_tags(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start) = remaining.find("<internal>") {
        result.push_str(&remaining[..start]);
        if let Some(end) = remaining[start..].find("</internal>") {
            remaining = &remaining[start + end + "</internal>".len()..];
        } else {
            // Unclosed tag — strip everything from <internal> onward
            return result;
        }
    }
    result.push_str(remaining);
    result
}

/// Streaming filter that buffers text to strip `<internal>` blocks in real-time.
struct InternalTagFilter {
    raw: String,
    emitted_len: usize,
}

impl InternalTagFilter {
    fn new() -> Self {
        Self {
            raw: String::new(),
            emitted_len: 0,
        }
    }

    /// Append new text and return the portion safe to emit.
    fn push(&mut self, delta: &str) -> Option<String> {
        self.raw.push_str(delta);
        let cleaned = strip_internal_tags(&self.raw);
        // Don't emit past an unclosed <internal> tag
        let safe_end = if let Some(pos) = self.raw.rfind("<internal") {
            // Check if this opening tag has a matching close
            if self.raw[pos..].contains("</internal>") {
                cleaned.len()
            } else {
                // Unclosed — only emit up to the tag start in cleaned text
                let raw_before_tag = &self.raw[..pos];
                strip_internal_tags(raw_before_tag).len()
            }
        } else {
            // Also hold back if we might be starting a tag (partial `<inter...`)
            let hold_back = partial_tag_overlap(&self.raw);
            cleaned.len().saturating_sub(hold_back)
        };

        if safe_end > self.emitted_len {
            let new_text = cleaned[self.emitted_len..safe_end].to_string();
            self.emitted_len = safe_end;
            Some(new_text)
        } else {
            None
        }
    }

    /// Flush remaining buffered text (called when stream ends).
    fn flush(&mut self) -> Option<String> {
        let cleaned = strip_internal_tags(&self.raw);
        if cleaned.len() > self.emitted_len {
            let remaining = cleaned[self.emitted_len..].to_string();
            self.emitted_len = cleaned.len();
            Some(remaining)
        } else {
            None
        }
    }

    /// Return the full cleaned text.
    fn full_clean(&self) -> String {
        strip_internal_tags(&self.raw)
    }
}

/// Check if the end of `text` is a partial match for `<internal>`.
fn partial_tag_overlap(text: &str) -> usize {
    let tag = "<internal>";
    let text_bytes = text.as_bytes();
    let tag_bytes = tag.as_bytes();
    for len in (1..tag_bytes.len()).rev() {
        if text_bytes.len() >= len && text_bytes[text_bytes.len() - len..] == tag_bytes[..len] {
            return len;
        }
    }
    0
}

pub enum AgentEvent {
    TextDelta(String),
    ThinkingDelta(String),
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
    Usage(UsageData),
    TurnComplete,
    Error(String),
}

pub struct Agent {
    config: Config,
    history: Vec<Message>,
    tool_registry: ToolRegistry,
    session: Session,
    policy: ExecutionPolicy,
    hook_registry: HookRegistry,
    turn_count: u32,
}

impl Agent {
    pub fn new(config: Config) -> Result<Self> {
        // Validate that the LLM client can be constructed (provider + API key present)
        let _ = LlmClient::new(config.clone())?;
        let tool_registry = ToolRegistry::new()?;
        let policy = config.policy.clone();
        Ok(Self {
            config,
            history: Vec::new(),
            tool_registry,
            session: Session::new(),
            policy,
            hook_registry: HookRegistry::new(),
            turn_count: 0,
        })
    }

    pub fn hook_registry_mut(&mut self) -> &mut HookRegistry {
        &mut self.hook_registry
    }

    /// Restore a session's history into this agent.
    pub fn load_session(&mut self, id: &str) -> Result<()> {
        let session = Session::load(id)?;
        self.history = session.messages.clone();
        self.session = session;
        Ok(())
    }

    /// Start a new session, clearing history.
    pub fn new_session(&mut self) {
        self.history.clear();
        self.session = Session::new();
    }

    /// Auto-save current session state.
    pub fn auto_save(&mut self) {
        self.session.update_from_history(&self.history);
        if let Err(e) = self.session.save() {
            warn!("Failed to auto-save session: {e}");
        }
    }

    /// Push a message to history and persist it to SQLite for crash recovery.
    fn persist_message(&mut self, msg: Message) {
        let session_id = self.session.meta.id.clone();
        let role = match msg.role {
            crate::types::Role::System => "system",
            crate::types::Role::User => "user",
            crate::types::Role::Assistant => "assistant",
            crate::types::Role::Tool => "tool",
        };
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .and_then(|tc| serde_json::to_string(tc).ok());
        if let Ok(db) = Database::open() {
            let _ = db.insert_message(
                &session_id,
                role,
                msg.content.as_deref(),
                tool_calls_json.as_deref(),
                msg.tool_call_id.as_deref(),
                msg.timestamp.as_deref(),
            );
        }
        self.history.push(msg);
    }

    pub fn session(&self) -> &Session {
        &self.session
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

    /// Compact conversation history using LLM summarization, returning (before_tokens, after_tokens).
    pub async fn compact(&mut self) -> (usize, usize) {
        let before = history_tokens(&self.history);
        let llm = match LlmClient::new(self.config.clone()) {
            Ok(l) => l,
            Err(_) => return (before, before),
        };
        compact_history(
            &mut self.history,
            self.config.conversation.max_history_tokens,
            &llm,
        )
        .await;
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

        // Richer context: working directory, git branch, OS info
        system.push_str("\n# Environment\n");
        if let Ok(cwd) = std::env::current_dir() {
            system.push_str(&format!("Working directory: {}\n", cwd.display()));
        }
        if let Ok(output) = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .output()
        {
            if output.status.success() {
                let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !branch.is_empty() {
                    system.push_str(&format!("Git branch: {branch}\n"));
                }
            }
        }
        system.push_str(&format!(
            "OS: {} {}\n",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));

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
        let mut tools = core_tool_definitions(&self.config);
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
        self.persist_message(msg);
        self.turn_count += 1;
        self.run_agent_loop(event_tx, cancel).await
    }

    pub async fn run_agent_loop(
        &mut self,
        event_tx: mpsc::Sender<AgentEvent>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let max_iterations = self.config.conversation.max_iterations as usize;
        let mut iteration: usize = 0;

        loop {
            if cancel.is_cancelled() {
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            // Budget enforcement
            let budget_limit = self.config.budget.monthly_token_limit;
            if budget_limit > 0 {
                if let Ok(db) = Database::open() {
                    if let Ok(used) = db.monthly_token_total() {
                        if used >= budget_limit {
                            let _ = event_tx
                                .send(AgentEvent::Error(format!(
                                    "Monthly token budget exceeded ({used}/{budget_limit}). \
                                     Increase budget.monthly_token_limit in /settings to continue."
                                )))
                                .await;
                            let _ = event_tx.send(AgentEvent::TurnComplete).await;
                            return Ok(());
                        }
                        let threshold = self.config.budget.warning_threshold;
                        let ratio = used as f64 / budget_limit as f64;
                        if ratio >= 0.95 || ratio >= threshold {
                            let pct = (ratio * 100.0) as u64;
                            let _ = event_tx
                                .send(AgentEvent::Error(format!(
                                    "Warning: {pct}% of monthly token budget used ({used}/{budget_limit})"
                                )))
                                .await;
                        }
                    }
                }
            }

            iteration += 1;
            if iteration > max_iterations {
                let _ = event_tx
                    .send(AgentEvent::Error(format!(
                        "Max iterations ({max_iterations}) reached — stopping agent loop"
                    )))
                    .await;
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            normalize_history(&mut self.history);

            // Only run LLM-based compaction when history exceeds the token budget
            if history_tokens(&self.history) > self.config.conversation.max_history_tokens {
                let compaction_llm = LlmClient::new(self.config.clone())?;
                compact_history(
                    &mut self.history,
                    self.config.conversation.max_history_tokens,
                    &compaction_llm,
                )
                .await;
            }

            let mut system_prompt = self.build_system_prompt()?;
            let tool_defs = self.build_tool_definitions();

            // Fire BeforeAgentStart (first iteration) or BeforeLlmCall
            let hook_point = if iteration == 1 {
                HookPoint::BeforeAgentStart
            } else {
                HookPoint::BeforeLlmCall
            };
            let user_msg = self
                .history
                .iter()
                .rev()
                .find(|m| m.role == crate::types::Role::User)
                .and_then(|m| m.content.clone())
                .unwrap_or_default();
            let hook_data = if iteration == 1 {
                HookData::AgentStart {
                    user_message: user_msg,
                }
            } else {
                HookData::LlmCall {
                    message_count: self.history.len(),
                }
            };
            let hook_ctx = HookContext {
                point: hook_point,
                session_id: self.session.meta.id.clone(),
                turn_count: self.turn_count,
                data: hook_data,
            };
            if let HookAction::InjectContext(extra) = self.hook_registry.dispatch(&hook_ctx) {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&extra);
            }

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

            let mut tag_filter = InternalTagFilter::new();
            let mut tool_calls: Vec<PartialToolCall> = Vec::new();

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let text_content = tag_filter.full_clean();
                        if !text_content.is_empty() {
                            let content = format!("{text_content}\n\n[response interrupted]");
                            let msg = Message::assistant(&content);
                            log_message(&msg);
                            self.persist_message(msg);
                        }
                        let _ = event_tx.send(AgentEvent::TurnComplete).await;
                        let _ = stream_handle.await;
                        return Ok(());
                    }
                    event = stream_rx.recv() => {
                        match event {
                            Some(StreamEvent::TextDelta(delta)) => {
                                if let Some(filtered) = tag_filter.push(&delta) {
                                    let _ = event_tx.send(AgentEvent::TextDelta(filtered)).await;
                                }
                            }
                            Some(StreamEvent::ToolCallDelta {
                                index, id, name, arguments_delta,
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
                            Some(StreamEvent::Usage(usage)) => {
                                let _ = event_tx.send(AgentEvent::Usage(usage)).await;
                            }
                            Some(StreamEvent::Done) => break,
                            Some(StreamEvent::Error(e)) => {
                                let _ = event_tx.send(AgentEvent::Error(e)).await;
                                break;
                            }
                            Some(StreamEvent::ThinkingDelta(delta)) => {
                                if self.config.conversation.show_thinking {
                                    let _ = event_tx.send(AgentEvent::ThinkingDelta(delta)).await;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = stream_handle.await;

            // Flush any remaining buffered text from the internal-tag filter
            if let Some(remaining) = tag_filter.flush() {
                let _ = event_tx.send(AgentEvent::TextDelta(remaining)).await;
            }
            let text_content = tag_filter.full_clean();

            // Fire AfterLlmResponse hook
            let hook_ctx = HookContext {
                point: HookPoint::AfterLlmResponse,
                session_id: self.session.meta.id.clone(),
                turn_count: self.turn_count,
                data: HookData::LlmResponse {
                    has_tool_calls: !tool_calls.is_empty(),
                    text_length: text_content.len(),
                },
            };
            self.hook_registry.dispatch(&hook_ctx);

            if tool_calls.is_empty() {
                // Fire TurnComplete hook
                let hook_ctx = HookContext {
                    point: HookPoint::TurnComplete,
                    session_id: self.session.meta.id.clone(),
                    turn_count: self.turn_count,
                    data: HookData::TurnEnd {
                        total_tool_calls: 0,
                    },
                };
                self.hook_registry.dispatch(&hook_ctx);

                let msg = Message::assistant(&text_content);
                log_message(&msg);
                self.persist_message(msg);
                self.auto_save();
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

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
                let content = if text_content.is_empty() {
                    "[response interrupted — incomplete tool calls discarded]".to_string()
                } else {
                    format!("{text_content}\n\n[incomplete tool calls discarded]")
                };
                let msg = Message::assistant(&content);
                log_message(&msg);
                self.persist_message(msg);
                let _ = event_tx.send(AgentEvent::TurnComplete).await;
                return Ok(());
            }

            let assistant_msg = if text_content.is_empty() {
                Message {
                    role: crate::types::Role::Assistant,
                    content: None,
                    tool_calls: Some(tc.clone()),
                    tool_call_id: None,
                    timestamp: Some(chrono::Local::now().to_rfc3339()),
                }
            } else {
                Message {
                    role: crate::types::Role::Assistant,
                    content: Some(text_content.clone()),
                    tool_calls: Some(tc.clone()),
                    tool_call_id: None,
                    timestamp: Some(chrono::Local::now().to_rfc3339()),
                }
            };
            log_message(&assistant_msg);
            self.persist_message(assistant_msg);

            let (sequential, parallel): (Vec<_>, Vec<_>) =
                tc.iter().partition(|t| t.function.name == "run_shell");

            self.run_tool_calls(&parallel, &event_tx, &cancel).await;
            self.run_tool_calls(&sequential, &event_tx, &cancel).await;
        }
    }

    async fn run_tool_calls(
        &mut self,
        tool_calls: &[&ToolCall],
        event_tx: &mpsc::Sender<AgentEvent>,
        cancel: &CancellationToken,
    ) {
        for tool_call in tool_calls {
            if cancel.is_cancelled() {
                let remaining_msg =
                    Message::tool_result(&tool_call.id, "[tool call cancelled by user]");
                log_message(&remaining_msg);
                self.persist_message(remaining_msg);
                continue;
            }

            let name = &tool_call.function.name;
            let args = &tool_call.function.arguments;

            // Fire BeforeToolCall hook
            let hook_ctx = HookContext {
                point: HookPoint::BeforeToolCall,
                session_id: self.session.meta.id.clone(),
                turn_count: self.turn_count,
                data: HookData::ToolCall {
                    name: name.clone(),
                    args: args.clone(),
                },
            };
            if matches!(self.hook_registry.dispatch(&hook_ctx), HookAction::Skip) {
                let skip_msg = Message::tool_result(&tool_call.id, "[tool call skipped by hook]");
                log_message(&skip_msg);
                self.persist_message(skip_msg);
                continue;
            }

            let _ = event_tx
                .send(AgentEvent::ToolExecuting {
                    name: name.clone(),
                    args: args.clone(),
                })
                .await;

            let raw_result = self
                .execute_tool(name, args, event_tx)
                .await
                .unwrap_or_else(|e| format!("Error: {e}"));
            let truncated = truncate_output(&raw_result, TOOL_OUTPUT_MAX_TOKENS);
            let result = if self.config.security.secret_detection {
                redact_secrets(&truncated)
            } else {
                truncated
            };

            // Fire AfterToolCall hook
            let hook_ctx = HookContext {
                point: HookPoint::AfterToolCall,
                session_id: self.session.meta.id.clone(),
                turn_count: self.turn_count,
                data: HookData::ToolResult {
                    name: name.clone(),
                    result: result.clone(),
                    is_error: result.starts_with("Error:"),
                },
            };
            self.hook_registry.dispatch(&hook_ctx);

            let _ = event_tx
                .send(AgentEvent::ToolResult {
                    name: name.clone(),
                    result: result.clone(),
                })
                .await;
            let msg = Message::tool_result(&tool_call.id, &result);
            log_message(&msg);
            self.persist_message(msg);
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
                let filename = require_str_param(&args, "filename")?;
                let content = require_str_param(&args, "content")?;
                let append = args["append"].as_bool().unwrap_or(false);
                let scope = args["scope"].as_str().unwrap_or("global");
                write_memory_scoped(filename, content, append, scope)
            }
            "read_memory" => {
                let filename = require_str_param(&args, "filename")?;
                read_memory(filename)
            }
            "list_tools" => {
                let tool_list = self.tool_registry.list_tools();
                Ok(if tool_list.is_empty() {
                    "No user tools installed.".to_string()
                } else {
                    tool_list.join("\n")
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
                let patch = require_str_param(&args, "patch")?;
                let base_dir = Config::skills_dir()?;
                std::fs::create_dir_all(&base_dir)?;
                match apply_patch_to_dir(patch, &base_dir) {
                    Ok(_) => Ok("Skill patch applied successfully.".to_string()),
                    Err(e) => Ok(format!("Error applying skill patch: {e}")),
                }
            }
            "apply_patch" => {
                let patch = require_str_param(&args, "patch")?;
                let base_dir = std::env::current_dir()
                    .context("Failed to determine current working directory")?;
                match apply_patch_to_dir(patch, &base_dir) {
                    Ok(affected) => Ok(format!(
                        "Patch applied successfully. Files affected: {}",
                        affected.join(", ")
                    )),
                    Err(e) => Ok(format!("Error applying patch: {e}")),
                }
            }
            "create_tool" => {
                let patch = require_str_param(&args, "patch")?;
                let base_dir = Config::tools_dir()?;
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

                match self.policy.check(command) {
                    crate::policy::PolicyDecision::Deny => {
                        return Ok("Shell command denied by policy.".to_string());
                    }
                    crate::policy::PolicyDecision::AutoApprove => {}
                    crate::policy::PolicyDecision::Prompt => {
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
            "web_fetch" => {
                if !self.config.web.enabled {
                    return Ok(
                        "Web access is disabled. Enable it in config: [web] enabled = true"
                            .to_string(),
                    );
                }
                let url = require_str_param(&args, "url")?;
                let max_chars = args["max_chars"].as_u64().map(|v| v as usize);
                match web::web_fetch(url, max_chars).await {
                    Ok(content) => Ok(content),
                    Err(e) => Ok(format!("Error fetching URL: {e}")),
                }
            }
            "web_search" => {
                if !self.config.web.enabled {
                    return Ok(
                        "Web access is disabled. Enable it in config: [web] enabled = true"
                            .to_string(),
                    );
                }
                let query = require_str_param(&args, "query")?;
                match web::web_search(query, &self.config.web).await {
                    Ok(results) => Ok(results),
                    Err(e) => Ok(format!("Error searching: {e}")),
                }
            }
            "schedule_task" => {
                let task_name = require_str_param(&args, "name")?;
                let prompt = require_str_param(&args, "prompt")?;
                let schedule_type = args["schedule_type"].as_str().unwrap_or("interval");
                let schedule_expr = require_str_param(&args, "schedule_expr")?;
                let timezone = args["timezone"].as_str().unwrap_or("local");
                let next_run = match tasks::calculate_next_run(schedule_type, schedule_expr) {
                    Ok(nr) => nr,
                    Err(e) => return Ok(format!("Error: Invalid schedule: {e}")),
                };
                let id = uuid::Uuid::new_v4().to_string();
                match Database::open() {
                    Ok(db) => match db.create_task(&crate::db::NewTask {
                        id: &id,
                        name: task_name,
                        prompt,
                        schedule_type,
                        schedule_expr,
                        timezone,
                        next_run,
                    }) {
                        Ok(()) => Ok(format!(
                            "Scheduled task created: {task_name} (id: {})",
                            &id[..8]
                        )),
                        Err(e) => Ok(format!("Error creating task: {e}")),
                    },
                    Err(e) => Ok(format!("Error opening database: {e}")),
                }
            }
            "list_scheduled_tasks" => match Database::open() {
                Ok(db) => match db.list_tasks() {
                    Ok(tl) if tl.is_empty() => Ok("No scheduled tasks.".to_string()),
                    Ok(tl) => Ok(tl
                        .iter()
                        .map(tasks::format_task)
                        .collect::<Vec<_>>()
                        .join("\n\n")),
                    Err(e) => Ok(format!("Error listing tasks: {e}")),
                },
                Err(e) => Ok(format!("Error opening database: {e}")),
            },
            "pause_task" => {
                let task_id = require_str_param(&args, "task_id")?;
                update_task_status(task_id, "paused", "paused")
            }
            "resume_task" => {
                let task_id = require_str_param(&args, "task_id")?;
                update_task_status(task_id, "active", "resumed")
            }
            "cancel_task" => {
                let task_id = require_str_param(&args, "task_id")?;
                update_task_status(task_id, "cancelled", "cancelled")
            }
            "read_pdf" => {
                let file_path = require_str_param(&args, "file_path")?;
                let max_chars = args["max_chars"].as_u64().unwrap_or(50000) as usize;
                let path = std::path::Path::new(file_path);
                if !path.exists() {
                    return Ok(format!("File not found: {file_path}"));
                }
                match pdf_extract::extract_text(path) {
                    Ok(text) => {
                        if text.len() > max_chars {
                            let truncated: String = text.chars().take(max_chars).collect();
                            Ok(format!(
                                "{truncated}\n\n[truncated — {max_chars}/{} chars shown]",
                                text.len()
                            ))
                        } else {
                            Ok(text)
                        }
                    }
                    Err(e) => Ok(format!("Error reading PDF: {e}")),
                }
            }
            _ => {
                let cred_names = self.tool_registry.tool_credentials(name);
                let extra_env: Vec<(String, String)> = cred_names
                    .iter()
                    .filter_map(|cred_name| {
                        self.config.credentials.get(cred_name).and_then(|env_var| {
                            std::env::var(env_var)
                                .ok()
                                .map(|val| (cred_name.to_uppercase(), val))
                        })
                    })
                    .collect();
                match self
                    .tool_registry
                    .execute_tool_full(
                        name,
                        args_json,
                        &extra_env,
                        &self.config.security.blocked_paths,
                    )
                    .await
                {
                    Ok(result) => Ok(result),
                    Err(e) => Ok(format!("Error executing tool '{name}': {e}")),
                }
            }
        }
    }
}

fn require_str_param<'a>(args: &'a serde_json::Value, name: &str) -> Result<&'a str> {
    args[name]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter '{name}'."))
}

fn update_task_status(task_id: &str, status: &str, verb: &str) -> Result<String> {
    match Database::open() {
        Ok(db) => match db.update_task_status(task_id, status) {
            Ok(true) => Ok(format!("Task {task_id} {verb}.")),
            Ok(false) => Ok(format!("Task {task_id} not found.")),
            Err(e) => Ok(format!("Error: {e}")),
        },
        Err(e) => Ok(format!("Error opening database: {e}")),
    }
}

#[derive(Default)]
struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_internal_tags_basic() {
        let input = "Hello <internal>secret thinking</internal> world";
        assert_eq!(strip_internal_tags(input), "Hello  world");
    }

    #[test]
    fn strip_internal_tags_multiple() {
        let input = "A <internal>x</internal> B <internal>y</internal> C";
        assert_eq!(strip_internal_tags(input), "A  B  C");
    }

    #[test]
    fn strip_internal_tags_multiline() {
        let input = "Hello <internal>\nthinking\nacross lines\n</internal> world";
        assert_eq!(strip_internal_tags(input), "Hello  world");
    }

    #[test]
    fn strip_internal_tags_no_tags() {
        let input = "Hello world";
        assert_eq!(strip_internal_tags(input), "Hello world");
    }

    #[test]
    fn strip_internal_tags_unclosed() {
        let input = "Hello <internal>never closed";
        assert_eq!(strip_internal_tags(input), "Hello ");
    }

    #[test]
    fn strip_internal_tags_empty() {
        assert_eq!(strip_internal_tags(""), "");
    }

    #[test]
    fn internal_tag_filter_streaming() {
        let mut filter = InternalTagFilter::new();
        // Simulate streaming: "Hello <internal>secret</internal> world"
        let r1 = filter.push("Hello ");
        assert_eq!(r1, Some("Hello ".to_string()));

        let r2 = filter.push("<internal>sec");
        assert_eq!(r2, None); // buffered, inside tag

        let r3 = filter.push("ret</internal> world");
        assert!(r3.is_some());
        assert_eq!(r3.as_deref(), Some(" world"));
    }

    #[test]
    fn internal_tag_filter_no_tags() {
        let mut filter = InternalTagFilter::new();
        let r = filter.push("Hello world");
        assert_eq!(r, Some("Hello world".to_string()));
    }

    #[test]
    fn partial_tag_overlap_basic() {
        assert_eq!(partial_tag_overlap("text<"), 1);
        assert_eq!(partial_tag_overlap("text<int"), 4);
        assert_eq!(partial_tag_overlap("text<internal"), 9);
        assert_eq!(partial_tag_overlap("text"), 0);
    }

    #[test]
    fn require_str_param_present() {
        let args = serde_json::json!({"name": "hello"});
        let val = require_str_param(&args, "name").unwrap();
        assert_eq!(val, "hello");
    }

    #[test]
    fn require_str_param_missing() {
        let args = serde_json::json!({"other": "value"});
        let err = require_str_param(&args, "name").unwrap_err();
        assert!(err.to_string().contains("name"));
    }

    #[test]
    fn require_str_param_null() {
        let args = serde_json::json!({"name": null});
        assert!(require_str_param(&args, "name").is_err());
    }

    #[test]
    fn require_str_param_wrong_type() {
        let args = serde_json::json!({"name": 42});
        assert!(require_str_param(&args, "name").is_err());
    }

    #[test]
    fn require_str_param_empty_string() {
        let args = serde_json::json!({"name": ""});
        let val = require_str_param(&args, "name").unwrap();
        assert_eq!(val, "");
    }

    #[test]
    fn update_task_status_not_found() {
        let id = "test-nonexistent-00000000";
        let result = update_task_status(id, "paused", "paused").unwrap();
        assert!(
            result.contains("not found"),
            "expected 'not found' in: {result}"
        );
    }

    #[test]
    fn update_task_status_formats_verb() {
        // Verify the verb parameter is used in the output format
        let id = "test-nonexistent-00000001";
        let result = update_task_status(id, "cancelled", "cancelled").unwrap();
        assert!(result.contains(id));
    }
}

fn core_tool_definitions(config: &Config) -> Vec<ToolDefinition> {
    let mut defs = vec![
        ToolDefinition::new("write_memory", "Write or append to a memory file. Use filename 'SOUL.md' to update personality, 'MEMORY.md' for the index, or any other name for topic-specific memories. Use scope='local' to write to project-local memory (.tamagotchi/ in CWD).", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file"},"content":{"type":"string","description":"Content to write"},"append":{"type":"boolean","description":"Append instead of overwriting","default":false},"scope":{"type":"string","enum":["global","local"],"description":"Memory scope: 'global' (default, ~/.tamagotchi/) or 'local' (CWD/.tamagotchi/)","default":"global"}},"required":["filename","content"]})),
        ToolDefinition::new("read_memory", "Read a memory file.", serde_json::json!({"type":"object","properties":{"filename":{"type":"string","description":"Name of the memory file to read"}},"required":["filename"]})),
        ToolDefinition::new("list_tools", "List all available user-created tools.", serde_json::json!({"type":"object","properties":{}})),
        ToolDefinition::new("apply_patch", "Create, update, or delete files in the current working directory using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("create_tool", "Create or modify user tools in ~/.tamagotchi/tools/ using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("run_shell", "Execute a shell command. Requires user confirmation before execution.", serde_json::json!({"type":"object","properties":{"command":{"type":"string","description":"Shell command to execute"}},"required":["command"]})),
        ToolDefinition::new("list_skills", "List all available skills with their status and source.", serde_json::json!({"type":"object","properties":{}})),
        ToolDefinition::new("apply_skill_patch", "Create or modify skill files in the skills directory using the patch DSL.", serde_json::json!({"type":"object","properties":{"patch":{"type":"string","description":"The patch content in the patch DSL format"}},"required":["patch"]})),
        ToolDefinition::new("read_pdf", "Read and extract text from a PDF file.", serde_json::json!({"type":"object","properties":{"file_path":{"type":"string","description":"Path to the PDF file"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["file_path"]})),
    ];

    if config.web.enabled {
        defs.push(ToolDefinition::new("web_fetch", "Fetch a URL and return its text content. HTML pages are automatically converted to plain text.", serde_json::json!({"type":"object","properties":{"url":{"type":"string","description":"The URL to fetch"},"max_chars":{"type":"integer","description":"Maximum characters to return (default: 50000)","default":50000}},"required":["url"]})));
        defs.push(ToolDefinition::new("web_search", "Search the web and return results with titles, URLs, and snippets.", serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"The search query"}},"required":["query"]})));
    }

    defs.push(ToolDefinition::new("schedule_task", "Create a scheduled task that runs automatically.", serde_json::json!({"type":"object","properties":{"name":{"type":"string","description":"A short name for the task"},"prompt":{"type":"string","description":"The prompt to execute on each run"},"schedule_type":{"type":"string","enum":["cron","interval","once"],"description":"Type of schedule"},"schedule_expr":{"type":"string","description":"Schedule expression (cron string or interval like '30m', '2h')"},"timezone":{"type":"string","description":"Timezone (default: 'local')","default":"local"}},"required":["name","prompt","schedule_type","schedule_expr"]})));
    defs.push(ToolDefinition::new(
        "list_scheduled_tasks",
        "List all scheduled tasks with their status and next run time.",
        serde_json::json!({"type":"object","properties":{}}),
    ));
    defs.push(ToolDefinition::new("pause_task", "Pause a scheduled task.", serde_json::json!({"type":"object","properties":{"task_id":{"type":"string","description":"The task ID to pause"}},"required":["task_id"]})));
    defs.push(ToolDefinition::new("resume_task", "Resume a paused scheduled task.", serde_json::json!({"type":"object","properties":{"task_id":{"type":"string","description":"The task ID to resume"}},"required":["task_id"]})));
    defs.push(ToolDefinition::new("cancel_task", "Cancel a scheduled task permanently.", serde_json::json!({"type":"object","properties":{"task_id":{"type":"string","description":"The task ID to cancel"}},"required":["task_id"]})));

    defs
}
