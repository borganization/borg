use tracing::instrument;

use crate::agent::AgentEvent;
use crate::types::{PlanStepStatus, ToolOutput};

use super::require_str_param;

/// Handle the `update_plan` tool: parse structured plan steps and emit a PlanUpdated event.
#[instrument(skip_all, fields(tool.name = "update_plan"))]
pub async fn handle_update_plan(
    args: &serde_json::Value,
    event_tx: &tokio::sync::mpsc::Sender<AgentEvent>,
) -> anyhow::Result<ToolOutput> {
    let steps_val = args
        .get("steps")
        .ok_or_else(|| anyhow::anyhow!("Missing required parameter 'steps'"))?;
    let steps: Vec<crate::types::PlanStep> = serde_json::from_value(steps_val.clone())
        .map_err(|e| anyhow::anyhow!("Invalid steps format: {e}. Each step needs 'title' (string) and 'status' (pending|in_progress|completed)."))?;

    // Validate: at most one step may be in_progress
    let in_progress_count = steps
        .iter()
        .filter(|s| s.status == PlanStepStatus::InProgress)
        .count();
    if in_progress_count > 1 {
        return Ok(ToolOutput::Text(
            "Error: At most one step may be in_progress at a time.".to_string(),
        ));
    }

    let _ = event_tx.send(AgentEvent::PlanUpdated { steps }).await;

    Ok(ToolOutput::Text("Plan updated.".to_string()))
}

/// Handle the `request_user_input` tool: prompt the user for input and block until they respond.
#[instrument(skip_all, fields(tool.name = "request_user_input"))]
pub async fn handle_request_user_input(
    args: &serde_json::Value,
    event_tx: &tokio::sync::mpsc::Sender<AgentEvent>,
) -> anyhow::Result<ToolOutput> {
    let prompt = require_str_param(args, "prompt")?;

    let (respond_tx, respond_rx) = tokio::sync::oneshot::channel::<String>();
    let _ = event_tx
        .send(AgentEvent::UserInputRequest {
            prompt: prompt.to_string(),
            respond: respond_tx,
        })
        .await;

    // Wait for user response with a 5-minute timeout
    match tokio::time::timeout(std::time::Duration::from_secs(300), respond_rx).await {
        Ok(Ok(response)) => Ok(ToolOutput::Text(response)),
        Ok(Err(_)) => Ok(ToolOutput::Text(
            "[No response received — channel closed]".to_string(),
        )),
        Err(_) => Ok(ToolOutput::Text(
            "[No response received — timed out after 5 minutes]".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn handle_request_user_input_requires_prompt() {
        let args = json!({});
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel(16);
        let result = handle_request_user_input(&args, &event_tx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn handle_request_user_input_emits_event_and_returns_response() {
        let args = json!({"prompt": "Which DB?"});
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(16);

        let handle = tokio::spawn(async move { handle_request_user_input(&args, &event_tx).await });

        let event = event_rx.recv().await.unwrap();
        match event {
            AgentEvent::UserInputRequest { prompt, respond } => {
                assert_eq!(prompt, "Which DB?");
                let _ = respond.send("PostgreSQL".to_string());
            }
            _ => panic!("expected UserInputRequest"),
        }

        let result = handle.await.unwrap().unwrap();
        match result {
            ToolOutput::Text(t) => assert_eq!(t, "PostgreSQL"),
            _ => panic!("expected Text output"),
        }
    }
}
