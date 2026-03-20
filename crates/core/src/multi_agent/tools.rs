use anyhow::Result;
use serde_json::json;

use crate::tool_handlers::require_str_param;
use crate::types::ToolDefinition;

use super::roles;
use super::AgentControl;

/// Tool definitions for the multi-agent system.
pub fn tool_definitions(spawn_depth: u32, max_spawn_depth: u32) -> Vec<ToolDefinition> {
    let mut defs = vec![
        ToolDefinition::new(
            "manage_roles",
            "Manage agent roles: list, create, update, or delete role definitions. Roles define model, temperature, allowed tools, and instructions for sub-agents.",
            json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "create", "update", "delete"],
                        "description": "Action to perform"
                    },
                    "name": {
                        "type": "string",
                        "description": "Role name (required for create/update/delete)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Role description"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model override for this role"
                    },
                    "provider": {
                        "type": "string",
                        "description": "Provider override (openrouter, openai, anthropic, gemini)"
                    },
                    "temperature": {
                        "type": "number",
                        "description": "Temperature override"
                    },
                    "system_instructions": {
                        "type": "string",
                        "description": "Additional system instructions for this role"
                    },
                    "tools_allowed": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Whitelist of tool names this role can use (null = all tools)"
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Max agent loop iterations for this role"
                    }
                },
                "required": ["action"]
            }),
        ),
    ];

    // Only add spawn/interact tools if we haven't hit depth limit
    if spawn_depth < max_spawn_depth {
        defs.push(ToolDefinition::new(
            "spawn_agent",
            "Spawn a new sub-agent to work on a task concurrently. The sub-agent runs independently and returns its result when done. Use roles (researcher, coder, writer) to specialize the agent.",
            json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The task/message to send to the sub-agent"
                    },
                    "role": {
                        "type": "string",
                        "description": "Role name (researcher, coder, writer, or custom). Determines model, temperature, and allowed tools."
                    },
                    "nickname": {
                        "type": "string",
                        "description": "Optional friendly name for the agent"
                    },
                    "model": {
                        "type": "string",
                        "description": "Override the model for this agent"
                    },
                    "fork_context": {
                        "type": "boolean",
                        "description": "If true, copies current conversation history to the sub-agent"
                    }
                },
                "required": ["message"]
            }),
        ));

        defs.push(ToolDefinition::new(
            "send_to_agent",
            "Send an additional message to a running sub-agent.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to send to"
                    },
                    "message": {
                        "type": "string",
                        "description": "The message to send"
                    }
                },
                "required": ["agent_id", "message"]
            }),
        ));

        defs.push(ToolDefinition::new(
            "wait_for_agent",
            "Wait for a sub-agent to complete and return its result. Blocks until the agent finishes or timeout is reached.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to wait for"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 300)",
                        "default": 300
                    }
                },
                "required": ["agent_id"]
            }),
        ));

        defs.push(ToolDefinition::new(
            "close_agent",
            "Shut down a sub-agent, cancelling any in-progress work.",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The agent ID to shut down"
                    }
                },
                "required": ["agent_id"]
            }),
        ));
    }

    defs
}

/// Handle the spawn_agent tool call.
pub async fn handle_spawn_agent(
    args: &serde_json::Value,
    agent_control: &mut AgentControl,
    parent_config: &crate::config::Config,
    parent_history: Option<&[crate::types::Message]>,
) -> Result<String> {
    let message = require_str_param(args, "message")?;
    let role_name = args["role"].as_str();
    let nickname = args["nickname"].as_str();
    let model_override = args["model"].as_str();
    let fork_context = args["fork_context"].as_bool().unwrap_or(false);

    let role = role_name.and_then(roles::load_role);

    let context = if fork_context { parent_history } else { None };

    let (agent_id, nickname) = agent_control
        .spawn_agent(
            message,
            role,
            nickname,
            model_override,
            parent_config,
            context,
        )
        .await?;

    Ok(json!({
        "agent_id": agent_id,
        "nickname": nickname,
        "status": "spawned"
    })
    .to_string())
}

/// Handle the send_to_agent tool call.
pub async fn handle_send_to_agent(
    args: &serde_json::Value,
    agent_control: &AgentControl,
) -> Result<String> {
    let agent_id = require_str_param(args, "agent_id")?;
    let message = require_str_param(args, "message")?;
    agent_control.send_input(agent_id, message).await?;
    Ok(json!({"status": "sent"}).to_string())
}

/// Handle the wait_for_agent tool call.
pub async fn handle_wait_for_agent(
    args: &serde_json::Value,
    agent_control: &mut AgentControl,
) -> Result<String> {
    let agent_id = require_str_param(args, "agent_id")?;
    let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(300);

    let completion = agent_control.wait_for_agent(agent_id, timeout_secs).await?;

    Ok(json!({
        "agent_id": completion.agent_id,
        "nickname": completion.nickname,
        "status": completion.status.as_str(),
        "result": completion.final_response,
    })
    .to_string())
}

/// Handle the list_agents tool call.
pub fn handle_list_agents(agent_control: &AgentControl) -> Result<String> {
    let agents = agent_control.list_agents();
    if agents.is_empty() {
        return Ok("No active sub-agents.".to_string());
    }

    let list: Vec<serde_json::Value> = agents
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "nickname": a.nickname,
                "role": a.role,
                "depth": a.depth,
                "status": a.status.as_str(),
                "created_at": a.created_at,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&list)?)
}

/// Handle the close_agent tool call.
pub fn handle_close_agent(
    args: &serde_json::Value,
    agent_control: &mut AgentControl,
) -> Result<String> {
    let agent_id = require_str_param(args, "agent_id")?;
    agent_control.shutdown_agent(agent_id)?;
    Ok(json!({"status": "shutdown"}).to_string())
}

/// Handle the manage_roles tool call.
pub fn handle_manage_roles(args: &serde_json::Value) -> Result<String> {
    let action = require_str_param(args, "action")?;

    match action {
        "list" => {
            let all_roles = roles::list_all_roles();
            let list: Vec<serde_json::Value> = all_roles
                .iter()
                .map(|r| {
                    json!({
                        "name": r.name,
                        "description": r.description,
                        "model": r.model,
                        "provider": r.provider,
                        "temperature": r.temperature,
                        "tools_allowed": r.tools_allowed,
                        "max_iterations": r.max_iterations,
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&list)?)
        }
        "create" => {
            let name = require_str_param(args, "name")?;
            let description = require_str_param(args, "description")?;
            let model = args["model"].as_str();
            let provider = args["provider"].as_str();
            let temperature = args["temperature"].as_f64().map(|v| v as f32);
            let system_instructions = args["system_instructions"].as_str();
            let tools_json = args.get("tools_allowed").and_then(|v| {
                if v.is_array() {
                    serde_json::to_string(v).ok()
                } else {
                    None
                }
            });
            let max_iterations = args["max_iterations"].as_u64().map(|v| v as i64);

            let db = crate::db::Database::open()?;
            db.insert_role(
                name,
                description,
                model,
                provider,
                temperature,
                system_instructions,
                tools_json.as_deref(),
                max_iterations,
                false,
            )?;
            Ok(json!({"status": "created", "name": name}).to_string())
        }
        "update" => {
            let name = require_str_param(args, "name")?;
            let db = crate::db::Database::open()?;
            if db.get_role(name)?.is_none() {
                return Ok(json!({"error": format!("Role '{name}' not found")}).to_string());
            }
            let description = args["description"].as_str();
            let model = args["model"].as_str();
            let provider = args["provider"].as_str();
            let temperature = args["temperature"].as_f64().map(|v| v as f32);
            let system_instructions = args["system_instructions"].as_str();
            let tools_json = args.get("tools_allowed").and_then(|v| {
                if v.is_array() {
                    serde_json::to_string(v).ok()
                } else {
                    None
                }
            });
            let max_iterations = args["max_iterations"].as_u64().map(|v| v as i64);

            db.update_role(
                name,
                description,
                model,
                provider,
                temperature,
                system_instructions,
                tools_json.as_deref(),
                max_iterations,
            )?;
            Ok(json!({"status": "updated", "name": name}).to_string())
        }
        "delete" => {
            let name = require_str_param(args, "name")?;
            let db = crate::db::Database::open()?;
            db.delete_role(name)?;
            Ok(json!({"status": "deleted", "name": name}).to_string())
        }
        _ => Ok(json!({"error": format!("Unknown action: {action}")}).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_agent_tool_definition() {
        let defs = tool_definitions(0, 1);
        let spawn = defs.iter().find(|d| d.function.name == "spawn_agent");
        assert!(spawn.is_some(), "spawn_agent should be in tool definitions");
        let spawn = spawn.unwrap();
        let params = &spawn.function.parameters;
        assert!(params["required"]
            .as_array()
            .unwrap()
            .contains(&json!("message")));
    }

    #[test]
    fn test_tool_filtering_at_max_depth() {
        let defs = tool_definitions(1, 1);
        assert!(
            defs.iter().all(|d| d.function.name != "spawn_agent"),
            "spawn_agent should be excluded at max depth"
        );
        assert!(
            defs.iter().all(|d| d.function.name != "wait_for_agent"),
            "wait_for_agent should be excluded at max depth"
        );
        assert!(
            defs.iter().all(|d| d.function.name != "send_to_agent"),
            "send_to_agent should be excluded at max depth"
        );
        assert!(
            defs.iter().all(|d| d.function.name != "close_agent"),
            "close_agent should be excluded at max depth"
        );
        // manage_roles should still be present (list_agents moved to unified `list` tool)
        assert!(defs.iter().any(|d| d.function.name == "manage_roles"));
    }

    #[test]
    fn test_handle_list_agents_empty() {
        let config = crate::config::MultiAgentConfig::default();
        let ctrl = AgentControl::new(&config, "session-1", 0);
        let result = handle_list_agents(&ctrl).unwrap();
        assert_eq!(result, "No active sub-agents.");
    }

    #[tokio::test]
    async fn test_handle_spawn_agent_missing_message() {
        let config = crate::config::MultiAgentConfig::default();
        let mut ctrl = AgentControl::new(&config, "session-1", 0);
        let parent_config = crate::config::Config::default();
        let args = json!({});
        let result = handle_spawn_agent(&args, &mut ctrl, &parent_config, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("message"));
    }

    #[test]
    fn test_handle_close_agent_nonexistent() {
        let config = crate::config::MultiAgentConfig::default();
        let mut ctrl = AgentControl::new(&config, "session-1", 0);
        let args = json!({"agent_id": "nonexistent"});
        let result = handle_close_agent(&args, &mut ctrl);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_manage_roles_list() {
        let args = json!({"action": "list"});
        let result = handle_manage_roles(&args);
        assert!(result.is_ok());
    }
}
