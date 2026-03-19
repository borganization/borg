use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::constants;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    ToolCall,
    ShellCommand,
    FileWrite,
    MemoryWrite,
    WebRequest,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RateDecision {
    Allow,
    Warn(String),
    Block(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLimits {
    #[serde(default = "default_tool_calls_warn")]
    pub tool_calls_warn: u32,
    #[serde(default = "default_tool_calls_block")]
    pub tool_calls_block: u32,
    #[serde(default = "default_shell_commands_warn")]
    pub shell_commands_warn: u32,
    #[serde(default = "default_shell_commands_block")]
    pub shell_commands_block: u32,
    #[serde(default = "default_file_writes_warn")]
    pub file_writes_warn: u32,
    #[serde(default = "default_file_writes_block")]
    pub file_writes_block: u32,
    #[serde(default = "default_memory_writes_warn")]
    pub memory_writes_warn: u32,
    #[serde(default = "default_memory_writes_block")]
    pub memory_writes_block: u32,
    #[serde(default = "default_web_requests_warn")]
    pub web_requests_warn: u32,
    #[serde(default = "default_web_requests_block")]
    pub web_requests_block: u32,
}

macro_rules! serde_default {
    ($name:ident, $value:expr) => {
        fn $name() -> u32 {
            $value
        }
    };
}

serde_default!(default_tool_calls_warn, constants::RATE_TOOL_CALLS_WARN);
serde_default!(default_tool_calls_block, constants::RATE_TOOL_CALLS_BLOCK);
serde_default!(
    default_shell_commands_warn,
    constants::RATE_SHELL_COMMANDS_WARN
);
serde_default!(
    default_shell_commands_block,
    constants::RATE_SHELL_COMMANDS_BLOCK
);
serde_default!(default_file_writes_warn, constants::RATE_FILE_WRITES_WARN);
serde_default!(default_file_writes_block, constants::RATE_FILE_WRITES_BLOCK);
serde_default!(
    default_memory_writes_warn,
    constants::RATE_MEMORY_WRITES_WARN
);
serde_default!(
    default_memory_writes_block,
    constants::RATE_MEMORY_WRITES_BLOCK
);
serde_default!(default_web_requests_warn, constants::RATE_WEB_REQUESTS_WARN);
serde_default!(
    default_web_requests_block,
    constants::RATE_WEB_REQUESTS_BLOCK
);

impl Default for ActionLimits {
    fn default() -> Self {
        Self {
            tool_calls_warn: default_tool_calls_warn(),
            tool_calls_block: default_tool_calls_block(),
            shell_commands_warn: default_shell_commands_warn(),
            shell_commands_block: default_shell_commands_block(),
            file_writes_warn: default_file_writes_warn(),
            file_writes_block: default_file_writes_block(),
            memory_writes_warn: default_memory_writes_warn(),
            memory_writes_block: default_memory_writes_block(),
            web_requests_warn: default_web_requests_warn(),
            web_requests_block: default_web_requests_block(),
        }
    }
}

impl ActionLimits {
    fn limits_for(&self, action: ActionType) -> (u32, u32) {
        match action {
            ActionType::ToolCall => (self.tool_calls_warn, self.tool_calls_block),
            ActionType::ShellCommand => (self.shell_commands_warn, self.shell_commands_block),
            ActionType::FileWrite => (self.file_writes_warn, self.file_writes_block),
            ActionType::MemoryWrite => (self.memory_writes_warn, self.memory_writes_block),
            ActionType::WebRequest => (self.web_requests_warn, self.web_requests_block),
        }
    }
}

pub struct SessionRateGuard {
    counters: HashMap<ActionType, u32>,
    limits: ActionLimits,
}

impl SessionRateGuard {
    pub fn new(limits: ActionLimits) -> Self {
        Self {
            counters: HashMap::new(),
            limits,
        }
    }

    /// Update the action limits in place (used by config hot reload).
    pub fn update_limits(&mut self, new_limits: ActionLimits) {
        self.limits = new_limits;
    }

    /// Record an action and return the rate decision.
    pub fn record(&mut self, action: ActionType) -> RateDecision {
        let count = self.counters.entry(action).or_insert(0);
        *count += 1;
        let current = *count;

        let (warn_threshold, block_threshold) = self.limits.limits_for(action);

        if current >= block_threshold {
            RateDecision::Block(format!(
                "Rate limit exceeded: {action:?} count ({current}) reached block threshold ({block_threshold}). \
                 Session action limit reached — please start a new session to continue."
            ))
        } else if current >= warn_threshold {
            RateDecision::Warn(format!(
                "Rate warning: {action:?} count ({current}) reached warn threshold ({warn_threshold})"
            ))
        } else {
            RateDecision::Allow
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_under_limit() {
        let mut guard = SessionRateGuard::new(ActionLimits::default());
        let decision = guard.record(ActionType::ToolCall);
        assert_eq!(decision, RateDecision::Allow);
    }

    #[test]
    fn test_warn_at_threshold() {
        let limits = ActionLimits {
            tool_calls_warn: 3,
            tool_calls_block: 5,
            ..Default::default()
        };
        let mut guard = SessionRateGuard::new(limits);
        for _ in 0..2 {
            assert_eq!(guard.record(ActionType::ToolCall), RateDecision::Allow);
        }
        match guard.record(ActionType::ToolCall) {
            RateDecision::Warn(_) => {}
            other => panic!("Expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn test_block_at_limit() {
        let limits = ActionLimits {
            tool_calls_warn: 2,
            tool_calls_block: 3,
            ..Default::default()
        };
        let mut guard = SessionRateGuard::new(limits);
        guard.record(ActionType::ToolCall);
        guard.record(ActionType::ToolCall);
        match guard.record(ActionType::ToolCall) {
            RateDecision::Block(_) => {}
            other => panic!("Expected Block, got {other:?}"),
        }
    }

    #[test]
    fn test_independent_counters() {
        let limits = ActionLimits {
            tool_calls_warn: 2,
            shell_commands_warn: 2,
            ..Default::default()
        };
        let mut guard = SessionRateGuard::new(limits);
        guard.record(ActionType::ToolCall);
        assert_eq!(guard.record(ActionType::ShellCommand), RateDecision::Allow);
    }

    #[test]
    fn test_default_limits() {
        let limits = ActionLimits::default();
        assert_eq!(limits.tool_calls_warn, 50);
        assert_eq!(limits.tool_calls_block, 100);
        assert_eq!(limits.shell_commands_warn, 20);
        assert_eq!(limits.shell_commands_block, 50);
        assert_eq!(limits.file_writes_warn, 15);
        assert_eq!(limits.file_writes_block, 30);
        assert_eq!(limits.memory_writes_warn, 10);
        assert_eq!(limits.memory_writes_block, 20);
        assert_eq!(limits.web_requests_warn, 20);
        assert_eq!(limits.web_requests_block, 50);
    }

    #[test]
    fn test_custom_limits() {
        let limits = ActionLimits {
            tool_calls_warn: 10,
            tool_calls_block: 20,
            ..Default::default()
        };
        let mut guard = SessionRateGuard::new(limits);
        for _ in 0..9 {
            assert_eq!(guard.record(ActionType::ToolCall), RateDecision::Allow);
        }
        match guard.record(ActionType::ToolCall) {
            RateDecision::Warn(_) => {}
            other => panic!("Expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn test_multiple_action_types() {
        let limits = ActionLimits {
            file_writes_warn: 3,
            file_writes_block: 5,
            memory_writes_warn: 3,
            memory_writes_block: 5,
            ..Default::default()
        };
        let mut guard = SessionRateGuard::new(limits);

        assert_eq!(guard.record(ActionType::FileWrite), RateDecision::Allow);
        assert_eq!(guard.record(ActionType::MemoryWrite), RateDecision::Allow);
        assert_eq!(guard.record(ActionType::FileWrite), RateDecision::Allow);

        // 3rd FileWrite should hit warn threshold
        match guard.record(ActionType::FileWrite) {
            RateDecision::Warn(_) => {}
            other => panic!("Expected Warn for FileWrite, got {other:?}"),
        }

        // MemoryWrite should still be at 1 (under warn of 3)
        assert_eq!(guard.record(ActionType::MemoryWrite), RateDecision::Allow);
    }
}
