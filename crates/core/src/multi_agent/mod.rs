pub mod names;
pub mod roles;
pub mod tools;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::Config;

/// Role definition for a sub-agent.
#[derive(Debug, Clone)]
pub struct AgentRole {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub temperature: Option<f32>,
    pub system_instructions: Option<String>,
    pub tools_allowed: Option<Vec<String>>,
    pub max_iterations: Option<u32>,
}

/// State machine for sub-agent lifecycle.
#[derive(Debug, Clone, PartialEq)]
pub enum SubAgentStatus {
    PendingInit,
    Running,
    Completed { result: String },
    Errored { error: String },
    Shutdown,
}

impl SubAgentStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::PendingInit => "pending_init",
            Self::Running => "running",
            Self::Completed { .. } => "completed",
            Self::Errored { .. } => "errored",
            Self::Shutdown => "shutdown",
        }
    }

    pub fn from_db(status: &str, result_text: Option<&str>, error_text: Option<&str>) -> Self {
        match status {
            "pending_init" => Self::PendingInit,
            "running" => Self::Running,
            "completed" => Self::Completed {
                result: result_text.unwrap_or_default().to_string(),
            },
            "errored" => Self::Errored {
                error: error_text.unwrap_or_default().to_string(),
            },
            "shutdown" => Self::Shutdown,
            _ => Self::PendingInit,
        }
    }
}

/// Metadata for a running or completed sub-agent.
#[derive(Debug, Clone)]
pub struct SubAgentInfo {
    pub id: String,
    pub nickname: String,
    pub role: String,
    pub parent_session_id: String,
    pub session_id: String,
    pub depth: u32,
    pub status: SubAgentStatus,
    pub created_at: i64,
}

/// Result delivered from a sub-agent to its parent.
#[derive(Debug, Clone)]
pub struct SubAgentCompletion {
    pub agent_id: String,
    pub nickname: String,
    pub status: SubAgentStatus,
    pub final_response: Option<String>,
}

/// Handle to a spawned sub-agent.
struct SubAgentHandle {
    pub info: SubAgentInfo,
    pub input_tx: mpsc::Sender<String>,
    pub cancel: CancellationToken,
    pub _join_handle: JoinHandle<()>,
}

/// Shared controller for managing sub-agents from a parent agent.
pub struct AgentControl {
    agents: HashMap<String, SubAgentHandle>,
    completion_tx: mpsc::Sender<SubAgentCompletion>,
    completion_rx: mpsc::Receiver<SubAgentCompletion>,
    semaphore: Arc<Semaphore>,
    pub max_spawn_depth: u32,
    pub max_children_per_agent: u32,
    _max_concurrent: u32,
    name_pool: names::NamePool,
    parent_session_id: String,
    current_depth: u32,
}

impl AgentControl {
    pub fn new(
        config: &crate::config::MultiAgentConfig,
        parent_session_id: &str,
        current_depth: u32,
    ) -> Self {
        let (completion_tx, completion_rx) = mpsc::channel(64);
        Self {
            agents: HashMap::new(),
            completion_tx,
            completion_rx,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent as usize)),
            max_spawn_depth: config.max_spawn_depth,
            max_children_per_agent: config.max_children_per_agent,
            _max_concurrent: config.max_concurrent,
            name_pool: names::NamePool::new(),
            parent_session_id: parent_session_id.to_string(),
            current_depth,
        }
    }

    /// Build a sub-agent config by applying role overrides and model override to the parent config.
    fn build_sub_agent_config(
        &self,
        role: Option<&AgentRole>,
        model_override: Option<&str>,
        parent_config: &Config,
    ) -> Config {
        let mut child_config = parent_config.clone();
        if let Some(role) = role {
            if let Some(ref model) = role.model {
                child_config.llm.model = model.clone();
            }
            if let Some(ref provider) = role.provider {
                child_config.llm.provider = Some(provider.to_string());
            }
            if let Some(temp) = role.temperature {
                child_config.llm.temperature = temp;
            }
        }
        if let Some(model) = model_override {
            child_config.llm.model = model.to_string();
        }

        // Sub-agents auto-approve shell commands but preserve deny list
        child_config.policy = crate::policy::ExecutionPolicy {
            auto_approve: vec!["*".to_string()],
            deny: parent_config.policy.deny.clone(),
        };

        child_config
    }

    /// Build the SubAgentInfo and record it in the database.
    fn prepare_sub_agent_info(
        &self,
        agent_id: &str,
        nickname: &str,
        role_name: &str,
        session_id: &str,
        child_depth: u32,
    ) -> SubAgentInfo {
        let info = SubAgentInfo {
            id: agent_id.to_string(),
            nickname: nickname.to_string(),
            role: role_name.to_string(),
            parent_session_id: self.parent_session_id.clone(),
            session_id: session_id.to_string(),
            depth: child_depth,
            status: SubAgentStatus::PendingInit,
            created_at: chrono::Utc::now().timestamp(),
        };

        // Record in DB
        if let Ok(db) = crate::db::Database::open() {
            let _ = db.insert_sub_agent_run(
                agent_id,
                nickname,
                role_name,
                &self.parent_session_id,
                session_id,
                child_depth,
            );
        }

        info
    }

    /// Validate spawn limits (depth and active children count).
    fn validate_spawn_limits(&self) -> Result<()> {
        if self.current_depth >= self.max_spawn_depth {
            bail!(
                "Cannot spawn sub-agent: max spawn depth ({}) reached",
                self.max_spawn_depth
            );
        }

        let active_count = self
            .agents
            .values()
            .filter(|h| {
                matches!(
                    h.info.status,
                    SubAgentStatus::PendingInit | SubAgentStatus::Running
                )
            })
            .count();
        if active_count >= self.max_children_per_agent as usize {
            bail!(
                "Cannot spawn sub-agent: max children ({}) reached",
                self.max_children_per_agent
            );
        }

        Ok(())
    }

    /// Spawn a new sub-agent. Returns (agent_id, nickname).
    pub async fn spawn_agent(
        &mut self,
        message: &str,
        role: Option<AgentRole>,
        nickname: Option<&str>,
        model_override: Option<&str>,
        parent_config: &Config,
        fork_context: Option<&[crate::types::Message]>,
    ) -> Result<(String, String)> {
        // Validate limits
        self.validate_spawn_limits()?;

        // Generate ID and nickname
        let agent_id = uuid::Uuid::new_v4().to_string();
        let chosen_nickname = match nickname {
            Some(n) => n.to_string(),
            None => self.name_pool.next_name(),
        };
        let role_name = role
            .as_ref()
            .map_or("default", |r| r.name.as_str())
            .to_string();

        // Build config
        let child_config =
            self.build_sub_agent_config(role.as_ref(), model_override, parent_config);

        let child_depth = self.current_depth + 1;
        let session_id = uuid::Uuid::new_v4().to_string();

        // Prepare info and record in DB
        let info = self.prepare_sub_agent_info(
            &agent_id,
            &chosen_nickname,
            &role_name,
            &session_id,
            child_depth,
        );

        // Spawn tokio task
        let (input_tx, input_rx) = mpsc::channel::<String>(16);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let completion_tx = self.completion_tx.clone();
        let semaphore = self.semaphore.clone();
        let agent_id_clone = agent_id.clone();
        let nickname_clone = chosen_nickname.clone();
        let message_owned = message.to_string();
        let fork_context_owned: Option<Vec<crate::types::Message>> =
            fork_context.map(<[crate::types::Message]>::to_vec);
        let agents_config = parent_config.agents.clone();

        let join_handle = tokio::spawn(async move {
            // Acquire semaphore permit
            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    let _ = completion_tx
                        .send(SubAgentCompletion {
                            agent_id: agent_id_clone,
                            nickname: nickname_clone,
                            status: SubAgentStatus::Errored {
                                error: "Semaphore closed".to_string(),
                            },
                            final_response: None,
                        })
                        .await;
                    return;
                }
            };

            let result = run_sub_agent(
                agent_id_clone.clone(),
                nickname_clone.clone(),
                child_config,
                child_depth,
                agents_config,
                message_owned,
                role,
                fork_context_owned,
                input_rx,
                cancel_clone,
            )
            .await;

            let (status, final_response) = match result {
                Ok(response) => (
                    SubAgentStatus::Completed {
                        result: response.clone(),
                    },
                    Some(response),
                ),
                Err(e) => (
                    SubAgentStatus::Errored {
                        error: e.to_string(),
                    },
                    None,
                ),
            };

            // Update DB
            if let Ok(db) = crate::db::Database::open() {
                let _ = db.update_sub_agent_status(
                    &agent_id_clone,
                    status.as_str(),
                    final_response.as_deref(),
                    match &status {
                        SubAgentStatus::Errored { error } => Some(error.as_str()),
                        _ => None,
                    },
                );
            }

            let _ = completion_tx
                .send(SubAgentCompletion {
                    agent_id: agent_id_clone,
                    nickname: nickname_clone,
                    status,
                    final_response,
                })
                .await;
        });

        // Store handle
        let handle = SubAgentHandle {
            info,
            input_tx,
            cancel,
            _join_handle: join_handle,
        };
        self.agents.insert(agent_id.clone(), handle);

        Ok((agent_id, chosen_nickname))
    }

    /// Send an additional message to a running sub-agent.
    pub async fn send_input(&self, agent_id: &str, message: &str) -> Result<()> {
        let handle = self
            .agents
            .get(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{agent_id}' not found"))?;
        handle
            .input_tx
            .send(message.to_string())
            .await
            .map_err(|_| anyhow::anyhow!("Agent '{agent_id}' is no longer running"))?;
        Ok(())
    }

    /// Wait for a specific sub-agent to complete.
    pub async fn wait_for_agent(
        &mut self,
        agent_id: &str,
        timeout_secs: u64,
    ) -> Result<SubAgentCompletion> {
        let timeout = tokio::time::Duration::from_secs(timeout_secs);
        let deadline = tokio::time::Instant::now() + timeout;

        // Check if already completed
        if let Some(handle) = self.agents.get(agent_id) {
            match &handle.info.status {
                SubAgentStatus::Completed { .. }
                | SubAgentStatus::Errored { .. }
                | SubAgentStatus::Shutdown => {
                    return Ok(SubAgentCompletion {
                        agent_id: agent_id.to_string(),
                        nickname: handle.info.nickname.clone(),
                        status: handle.info.status.clone(),
                        final_response: match &handle.info.status {
                            SubAgentStatus::Completed { result } => Some(result.clone()),
                            _ => None,
                        },
                    });
                }
                _ => {}
            }
        }

        // Poll completion channel until we get the one we want
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("Timeout waiting for agent '{agent_id}'");
            }

            match tokio::time::timeout(remaining, self.completion_rx.recv()).await {
                Ok(Some(completion)) => {
                    let completed_id = completion.agent_id.clone();
                    // Update internal state
                    if let Some(handle) = self.agents.get_mut(&completed_id) {
                        handle.info.status = completion.status.clone();
                    }
                    if completed_id == agent_id {
                        return Ok(completion);
                    }
                    // Not the one we want, keep polling
                }
                Ok(None) => bail!("Completion channel closed"),
                Err(_) => bail!("Timeout waiting for agent '{agent_id}'"),
            }
        }
    }

    /// List all sub-agents with their current info.
    pub fn list_agents(&self) -> Vec<SubAgentInfo> {
        self.agents.values().map(|h| h.info.clone()).collect()
    }

    /// Get status of a specific agent.
    pub fn get_status(&self, agent_id: &str) -> Option<&SubAgentInfo> {
        self.agents.get(agent_id).map(|h| &h.info)
    }

    /// Shut down a specific sub-agent.
    pub fn shutdown_agent(&mut self, agent_id: &str) -> Result<()> {
        let handle = self
            .agents
            .get_mut(agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent '{agent_id}' not found"))?;
        handle.cancel.cancel();
        handle.info.status = SubAgentStatus::Shutdown;

        // Update DB
        if let Ok(db) = crate::db::Database::open() {
            let _ = db.update_sub_agent_status(agent_id, "shutdown", None, None);
        }

        Ok(())
    }

    /// Drain any pending completions without blocking.
    pub fn drain_completions(&mut self) -> Vec<SubAgentCompletion> {
        let mut completions = Vec::new();
        while let Ok(completion) = self.completion_rx.try_recv() {
            if let Some(handle) = self.agents.get_mut(&completion.agent_id) {
                handle.info.status = completion.status.clone();
            }
            completions.push(completion);
        }
        completions
    }

    /// Shut down all sub-agents.
    pub fn shutdown_all(&mut self) {
        for handle in self.agents.values_mut() {
            handle.cancel.cancel();
            handle.info.status = SubAgentStatus::Shutdown;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

impl Drop for AgentControl {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

/// Run a sub-agent task. Returns the final text response.
/// Uses Box::pin to break the async recursion cycle (Agent -> AgentControl -> spawn -> Agent).
#[allow(clippy::too_many_arguments)]
fn run_sub_agent(
    agent_id: String,
    nickname: String,
    config: Config,
    depth: u32,
    agents_config: crate::config::MultiAgentConfig,
    initial_message: String,
    role: Option<AgentRole>,
    fork_context: Option<Vec<crate::types::Message>>,
    mut input_rx: mpsc::Receiver<String>,
    cancel: CancellationToken,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>> {
    Box::pin(async move {
        use crate::agent::{Agent, AgentEvent};

        let mut agent = Agent::new_sub_agent(
            config.clone(),
            depth,
            &agents_config,
            crate::telemetry::BorgMetrics::noop(),
        )?;

        // Inject fork context if provided
        if let Some(context) = fork_context {
            for msg in context {
                agent.inject_history_message(msg);
            }
        }

        // Prepend role instructions to the initial message
        let mut full_message =
            format!("You are a sub-agent named \"{nickname}\" (id: {agent_id}).\n");
        if let Some(ref role) = role {
            full_message.push_str(&format!("Role: {} — {}\n", role.name, role.description));
            if let Some(ref instructions) = role.system_instructions {
                full_message.push_str(instructions);
                full_message.push('\n');
            }
        }
        full_message.push_str(
            "Complete the task and provide a clear, concise final response. \
             Your full response will be returned to the parent agent.\n\n",
        );
        full_message.push_str(&initial_message);

        // Update DB status to running
        if let Ok(db) = crate::db::Database::open() {
            let _ = db.update_sub_agent_status(&agent_id, "running", None, None);
        }

        let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);
        let cancel_clone = cancel.clone();

        // Send initial message in a spawned task
        let send_handle = {
            let event_tx_clone = event_tx.clone();
            let cancel = cancel_clone.clone();
            tokio::spawn(async move {
                agent
                    .send_message_with_cancel(&full_message, event_tx_clone, cancel)
                    .await
            })
        };

        // Collect the final text response
        let mut final_text = String::new();
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    break;
                }
                event = event_rx.recv() => {
                    match event {
                        Some(AgentEvent::TextDelta(delta)) => {
                            final_text.push_str(&delta);
                        }
                        Some(AgentEvent::TurnComplete) | None => {
                            break;
                        }
                        Some(AgentEvent::ShellConfirmation { respond, .. }) => {
                            let _ = respond.send(true);
                        }
                        Some(AgentEvent::ToolConfirmation { respond, .. }) => {
                            // Deny dangerous ops in sub-agents (like gateway mode)
                            let _ = respond.send(false);
                        }
                        _ => {}
                    }
                }
                msg = input_rx.recv() => {
                    match msg {
                        Some(_additional_message) => {
                            // Future: support sending additional messages to running sub-agents
                        }
                        None => break,
                    }
                }
            }
        }

        let _ = send_handle.await;

        if final_text.is_empty() {
            Ok("(sub-agent completed with no text output)".to_string())
        } else {
            Ok(final_text)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MultiAgentConfig;

    fn test_config() -> MultiAgentConfig {
        MultiAgentConfig {
            enabled: true,
            max_spawn_depth: 1,
            max_children_per_agent: 5,
            max_concurrent: 3,
        }
    }

    #[test]
    fn test_agent_control_new() {
        let config = test_config();
        let ctrl = AgentControl::new(&config, "session-1", 0);
        assert!(ctrl.is_empty());
        assert_eq!(ctrl.max_spawn_depth, 1);
        assert_eq!(ctrl.max_children_per_agent, 5);
    }

    #[test]
    fn test_sub_agent_status_transitions() {
        let status = SubAgentStatus::PendingInit;
        assert_eq!(status.as_str(), "pending_init");

        let status = SubAgentStatus::Running;
        assert_eq!(status.as_str(), "running");

        let status = SubAgentStatus::Completed {
            result: "done".to_string(),
        };
        assert_eq!(status.as_str(), "completed");

        let status = SubAgentStatus::Errored {
            error: "oops".to_string(),
        };
        assert_eq!(status.as_str(), "errored");

        let status = SubAgentStatus::Shutdown;
        assert_eq!(status.as_str(), "shutdown");
    }

    #[test]
    fn test_sub_agent_status_from_db() {
        assert_eq!(
            SubAgentStatus::from_db("pending_init", None, None),
            SubAgentStatus::PendingInit
        );
        assert_eq!(
            SubAgentStatus::from_db("running", None, None),
            SubAgentStatus::Running
        );
        assert_eq!(
            SubAgentStatus::from_db("completed", Some("result"), None),
            SubAgentStatus::Completed {
                result: "result".to_string()
            }
        );
        assert_eq!(
            SubAgentStatus::from_db("errored", None, Some("fail")),
            SubAgentStatus::Errored {
                error: "fail".to_string()
            }
        );
        assert_eq!(
            SubAgentStatus::from_db("shutdown", None, None),
            SubAgentStatus::Shutdown
        );
        // Unknown defaults to PendingInit
        assert_eq!(
            SubAgentStatus::from_db("unknown", None, None),
            SubAgentStatus::PendingInit
        );
    }

    #[tokio::test]
    async fn test_spawn_depth_limit() {
        let config = MultiAgentConfig {
            enabled: true,
            max_spawn_depth: 1,
            max_children_per_agent: 5,
            max_concurrent: 3,
        };
        // current_depth=1 means we're already at the limit
        let mut ctrl = AgentControl::new(&config, "session-1", 1);
        let parent_config = Config::default();
        let result = ctrl
            .spawn_agent("test", None, None, None, &parent_config, None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max spawn depth"));
    }

    #[tokio::test]
    async fn test_max_children_limit() {
        let config = MultiAgentConfig {
            enabled: true,
            max_spawn_depth: 2,
            max_children_per_agent: 0, // no children allowed
            max_concurrent: 3,
        };
        let mut ctrl = AgentControl::new(&config, "session-1", 0);
        let parent_config = Config::default();
        let result = ctrl
            .spawn_agent("test", None, None, None, &parent_config, None)
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("max children"));
    }

    #[test]
    fn test_list_agents_empty() {
        let config = test_config();
        let ctrl = AgentControl::new(&config, "session-1", 0);
        assert!(ctrl.list_agents().is_empty());
    }

    #[test]
    fn test_shutdown_agent_nonexistent() {
        let config = test_config();
        let mut ctrl = AgentControl::new(&config, "session-1", 0);
        assert!(ctrl.shutdown_agent("nonexistent").is_err());
    }

    #[test]
    fn test_drain_completions_empty() {
        let config = test_config();
        let mut ctrl = AgentControl::new(&config, "session-1", 0);
        assert!(ctrl.drain_completions().is_empty());
    }

    #[test]
    fn test_config_merge_role_overrides() {
        let role = AgentRole {
            name: "researcher".to_string(),
            description: "Research agent".to_string(),
            model: Some("gpt-4".to_string()),
            provider: None,
            temperature: Some(0.3),
            system_instructions: None,
            tools_allowed: None,
            max_iterations: None,
        };
        let mut parent_config = Config::default();
        parent_config.llm.model = "default-model".to_string();
        parent_config.llm.temperature = 0.7;

        let ctrl = AgentControl::new(&test_config(), "session-1", 0);
        let child_config = ctrl.build_sub_agent_config(Some(&role), None, &parent_config);

        assert_eq!(child_config.llm.model, "gpt-4");
        assert!((child_config.llm.temperature - 0.3).abs() < f32::EPSILON);
    }

    #[test]
    fn test_config_merge_model_override_takes_precedence() {
        let role = AgentRole {
            name: "researcher".to_string(),
            description: "Research agent".to_string(),
            model: Some("gpt-4".to_string()),
            provider: None,
            temperature: None,
            system_instructions: None,
            tools_allowed: None,
            max_iterations: None,
        };
        let parent_config = Config::default();

        let ctrl = AgentControl::new(&test_config(), "session-1", 0);
        let child_config =
            ctrl.build_sub_agent_config(Some(&role), Some("claude-3"), &parent_config);

        // model_override should win over role.model
        assert_eq!(child_config.llm.model, "claude-3");
    }

    #[test]
    fn test_validate_spawn_limits_depth() {
        let config = MultiAgentConfig {
            enabled: true,
            max_spawn_depth: 2,
            max_children_per_agent: 5,
            max_concurrent: 3,
        };
        // At depth 2 with max_spawn_depth=2, should fail
        let ctrl = AgentControl::new(&config, "session-1", 2);
        assert!(ctrl.validate_spawn_limits().is_err());

        // At depth 1 with max_spawn_depth=2, should pass
        let ctrl = AgentControl::new(&config, "session-1", 1);
        assert!(ctrl.validate_spawn_limits().is_ok());
    }
}
