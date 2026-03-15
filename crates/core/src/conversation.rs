use std::collections::HashSet;
use tracing::debug;

use crate::types::Message;

/// Rough token estimate: ~4 characters per token.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

/// Estimate the token count of a single message, including role overhead.
fn message_tokens(msg: &Message) -> usize {
    // Role token overhead (~4 tokens for role + formatting)
    let role_overhead = 4;
    let content_tokens = msg.content.as_deref().map(estimate_tokens).unwrap_or(0);
    let tool_call_tokens: usize = msg
        .tool_calls
        .as_ref()
        .map(|tcs| {
            tcs.iter()
                .map(|tc| {
                    estimate_tokens(&tc.function.name) + estimate_tokens(&tc.function.arguments)
                })
                .sum()
        })
        .unwrap_or(0);
    role_overhead + content_tokens + tool_call_tokens
}

/// Compact conversation history to fit within a token budget.
///
/// Strategy:
/// - Always preserve the most recent messages (they provide immediate context).
/// - When the history exceeds the budget, drop the oldest messages and insert
///   a marker so the LLM knows earlier context was truncated.
/// - Tool result messages are only kept if their corresponding assistant
///   tool-call message is also kept (orphaned tool results confuse the API).
pub fn compact_history(history: &mut Vec<Message>, max_tokens: usize) {
    let total = history_tokens(history);
    if total <= max_tokens {
        return;
    }

    debug!(
        "Conversation history ({total} tokens) exceeds budget ({max_tokens} tokens), compacting"
    );

    // Walk backwards from the end, accumulating tokens for messages we keep.
    let mut keep_from = history.len();
    let mut budget_used: usize = 0;
    // Reserve tokens for the truncation marker we'll insert.
    let marker_tokens = 20;
    let effective_budget = max_tokens.saturating_sub(marker_tokens);

    for i in (0..history.len()).rev() {
        let msg_tok = message_tokens(&history[i]);
        if budget_used + msg_tok > effective_budget {
            break;
        }
        budget_used += msg_tok;
        keep_from = i;
    }

    // Ensure we keep at least the last message.
    if keep_from >= history.len() {
        keep_from = history.len().saturating_sub(1);
    }

    // If the kept portion starts with a Tool message, we need to also drop it
    // because it would be an orphaned tool result (its assistant message with
    // the tool_calls was dropped). Walk forward past any leading Tool messages.
    while keep_from < history.len() && history[keep_from].role == crate::types::Role::Tool {
        keep_from += 1;
    }

    // If we ended up keeping everything, nothing to compact.
    if keep_from == 0 {
        return;
    }

    let dropped = keep_from;
    debug!("Dropping {dropped} old messages from conversation history");

    // Build compacted history: marker + kept tail.
    let marker = Message::system(
        "[Earlier conversation history was truncated to fit context limits. \
         The conversation continues below.]",
    );

    let mut compacted = Vec::with_capacity(history.len() - dropped + 1);
    compacted.push(marker);
    compacted.extend(history.drain(dropped..));
    *history = compacted;
}

/// Total estimated tokens for a conversation history.
pub fn history_tokens(history: &[Message]) -> usize {
    history.iter().map(message_tokens).sum()
}

/// Undo the last agent turn: remove everything after the last user message.
/// Returns the number of messages removed, or 0 if there is nothing to undo.
pub fn undo_last_turn(history: &mut Vec<Message>) -> usize {
    // Find the index of the last user message
    let last_user_idx = history
        .iter()
        .rposition(|m| m.role == crate::types::Role::User);

    match last_user_idx {
        Some(idx) => {
            // If the last message IS the user message, pop it too (undo the user's input)
            // Otherwise, pop everything after the last user message (undo the assistant turn)
            let remove_from = if idx == history.len() - 1 {
                // Last msg is user — remove it and find the *previous* user message
                // to also remove the prior assistant response
                idx
            } else {
                idx + 1
            };
            let removed = history.len() - remove_from;
            history.truncate(remove_from);
            removed
        }
        None => 0,
    }
}

/// Normalize conversation history to prevent API errors.
///
/// Ensures structural invariants inspired by codex-rs:
/// 1. Every tool call has a corresponding tool result (synthesize if missing).
/// 2. Every tool result has a corresponding tool call (remove orphans).
///
/// This is called before sending to the LLM to prevent malformed conversations
/// that can cause API rejections.
pub fn normalize_history(history: &mut Vec<Message>) {
    // Collect all tool call IDs from assistant messages
    let call_ids: HashSet<String> = history
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flat_map(|tcs| tcs.iter().map(|tc| tc.id.clone()))
        .collect();

    // Collect all tool result IDs
    let result_ids: HashSet<String> = history
        .iter()
        .filter_map(|m| m.tool_call_id.as_ref())
        .cloned()
        .collect();

    // 1. Synthesize missing results for tool calls that have no result
    let missing: Vec<(usize, String)> = history
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.tool_calls.as_ref().map(|tcs| (i, tcs)))
        .flat_map(|(i, tcs)| {
            tcs.iter()
                .filter(|tc| !result_ids.contains(&tc.id))
                .map(move |tc| (i, tc.id.clone()))
        })
        .collect();

    // Insert synthetic results after their assistant message (in reverse to
    // preserve indices).
    for (assistant_idx, call_id) in missing.into_iter().rev() {
        let synthetic = Message::tool_result(&call_id, "[tool call aborted — no result received]");
        let insert_at = (assistant_idx + 1).min(history.len());
        history.insert(insert_at, synthetic);
    }

    // 2. Remove orphaned tool results (results with no matching call)
    history.retain(|m| {
        if let Some(ref tid) = m.tool_call_id {
            call_ids.contains(tid)
        } else {
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, Message, Role, ToolCall};

    fn make_user(text: &str) -> Message {
        Message::user(text)
    }

    fn make_assistant(text: &str) -> Message {
        Message::assistant(text)
    }

    fn make_tool_call_msg(text: &str, call_id: &str, tool_name: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            },
            tool_calls: Some(vec![ToolCall {
                id: call_id.to_string(),
                call_type: "function".to_string(),
                function: FunctionCall {
                    name: tool_name.to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            tool_call_id: None,
        }
    }

    fn make_tool_result(call_id: &str, result: &str) -> Message {
        Message::tool_result(call_id, result)
    }

    // -- estimate_tokens --

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_short() {
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("ab"), 0);
    }

    #[test]
    fn estimate_tokens_longer() {
        let text = "a".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    // -- message_tokens --

    #[test]
    fn message_tokens_text_only() {
        let msg = make_user("hello world!"); // 12 chars = 3 tokens + 4 overhead = 7
        assert_eq!(message_tokens(&msg), 7);
    }

    #[test]
    fn message_tokens_with_tool_calls() {
        let msg = make_tool_call_msg("", "id1", "read_memory");
        let tokens = message_tokens(&msg);
        // 4 overhead + 0 content + name("read_memory"=11/4=2) + args("{}"=2/4=0) = 6
        assert_eq!(tokens, 6);
    }

    #[test]
    fn message_tokens_empty_content() {
        let msg = Message {
            role: Role::Assistant,
            content: None,
            tool_calls: None,
            tool_call_id: None,
        };
        assert_eq!(message_tokens(&msg), 4); // just overhead
    }

    // -- compact_history --

    #[test]
    fn compact_noop_when_under_budget() {
        let mut history = vec![make_user("hi"), make_assistant("hello")];
        let before_len = history.len();
        compact_history(&mut history, 100_000);
        assert_eq!(history.len(), before_len);
    }

    #[test]
    fn compact_drops_oldest_messages() {
        // Create a history that exceeds a small budget
        let mut history: Vec<Message> = Vec::new();
        for i in 0..20 {
            history.push(make_user(&format!(
                "message number {i} with some padding text"
            )));
            history.push(make_assistant(&format!(
                "response number {i} with some padding text"
            )));
        }

        let original_len = history.len();
        compact_history(&mut history, 200);

        // Should have fewer messages now
        assert!(history.len() < original_len);
        // First message should be the truncation marker (system role)
        assert_eq!(history[0].role, Role::System);
        assert!(history[0].content.as_deref().unwrap().contains("truncated"));
    }

    #[test]
    fn compact_preserves_most_recent_messages() {
        let mut history = vec![
            make_user("old message 1"),
            make_assistant("old response 1"),
            make_user("old message 2"),
            make_assistant("old response 2"),
            make_user("recent question"),
            make_assistant("recent answer"),
        ];

        // Set budget to only fit the last pair
        let last_pair_tokens = message_tokens(&history[4]) + message_tokens(&history[5]);
        compact_history(&mut history, last_pair_tokens + 25);

        // The most recent messages should be preserved
        let last = history.last().unwrap();
        assert_eq!(last.content.as_deref(), Some("recent answer"));
        let second_last = &history[history.len() - 2];
        assert_eq!(second_last.content.as_deref(), Some("recent question"));
    }

    #[test]
    fn compact_drops_orphaned_tool_results() {
        let mut history = vec![
            make_user("do something"),
            make_tool_call_msg("", "call_1", "run_shell"),
            make_tool_result("call_1", "command output"),
            make_user("recent question"),
            make_assistant("recent answer"),
        ];

        // Budget that fits the last user+assistant pair but not the tool call pair
        let last_pair_tokens = message_tokens(&history[3]) + message_tokens(&history[4]);
        compact_history(&mut history, last_pair_tokens + 25);

        // No orphaned tool result messages should remain
        let has_orphan = history.iter().any(|m| {
            m.role == Role::Tool
                && !history.iter().any(|other| {
                    other.tool_calls.as_ref().is_some_and(|tcs| {
                        tcs.iter()
                            .any(|tc| Some(tc.id.as_str()) == m.tool_call_id.as_deref())
                    })
                })
        });
        assert!(!has_orphan, "Should not have orphaned tool results");
    }

    #[test]
    fn compact_with_zero_budget_keeps_last_message() {
        let mut history = vec![
            make_user("first"),
            make_assistant("second"),
            make_user("third"),
        ];
        compact_history(&mut history, 0);
        // Should still have at least the marker + last message
        assert!(!history.is_empty());
    }

    #[test]
    fn compact_single_message() {
        let mut history = vec![make_user("only message")];
        compact_history(&mut history, 0);
        // With only one message, there's nothing meaningful to drop
        assert!(!history.is_empty());
    }

    #[test]
    fn compact_empty_history() {
        let mut history: Vec<Message> = Vec::new();
        compact_history(&mut history, 100);
        assert!(history.is_empty());
    }

    // -- history_tokens --

    #[test]
    fn history_tokens_empty() {
        assert_eq!(history_tokens(&[]), 0);
    }

    #[test]
    fn history_tokens_sums_messages() {
        let history = vec![make_user("hello"), make_assistant("world")];
        let total = history_tokens(&history);
        assert_eq!(
            total,
            message_tokens(&history[0]) + message_tokens(&history[1])
        );
    }

    #[test]
    fn history_tokens_with_tool_calls() {
        let history = vec![
            make_user("test"),
            make_tool_call_msg("thinking", "c1", "run_shell"),
            make_tool_result("c1", "output data here"),
        ];
        let total = history_tokens(&history);
        assert!(total > 0);
        assert_eq!(
            total,
            message_tokens(&history[0]) + message_tokens(&history[1]) + message_tokens(&history[2])
        );
    }

    // -- normalize_history --

    #[test]
    fn normalize_synthesizes_missing_tool_result() {
        let mut history = vec![
            make_user("do something"),
            make_tool_call_msg("", "call_1", "run_shell"),
            // No tool result for call_1
            make_user("next question"),
        ];
        normalize_history(&mut history);

        // Should now have a synthetic result for call_1
        let has_result = history
            .iter()
            .any(|m| m.tool_call_id.as_deref() == Some("call_1"));
        assert!(has_result, "Should synthesize missing tool result");
        assert!(history
            .iter()
            .any(|m| m.content.as_deref().unwrap_or("").contains("aborted")));
    }

    #[test]
    fn normalize_removes_orphaned_tool_result() {
        let mut history = vec![
            make_user("do something"),
            make_tool_result("nonexistent_call", "orphaned output"),
            make_user("next"),
        ];
        normalize_history(&mut history);

        let has_orphan = history
            .iter()
            .any(|m| m.tool_call_id.as_deref() == Some("nonexistent_call"));
        assert!(!has_orphan, "Should remove orphaned tool result");
    }

    #[test]
    fn normalize_noop_on_valid_history() {
        let mut history = vec![
            make_user("test"),
            make_tool_call_msg("", "c1", "read_memory"),
            make_tool_result("c1", "memory content"),
            make_assistant("here is what I found"),
        ];
        let len_before = history.len();
        normalize_history(&mut history);
        assert_eq!(history.len(), len_before);
    }

    #[test]
    fn normalize_handles_empty_history() {
        let mut history: Vec<Message> = Vec::new();
        normalize_history(&mut history);
        assert!(history.is_empty());
    }

    // -- undo_last_turn --

    #[test]
    fn undo_removes_assistant_response() {
        let mut history = vec![
            make_user("hello"),
            make_assistant("hi there"),
            make_user("do something"),
            make_assistant("done"),
        ];
        let removed = undo_last_turn(&mut history);
        assert_eq!(removed, 1);
        assert_eq!(history.len(), 3);
        assert_eq!(
            history.last().unwrap().content.as_deref(),
            Some("do something")
        );
    }

    #[test]
    fn undo_removes_tool_call_and_results() {
        let mut history = vec![
            make_user("test"),
            make_tool_call_msg("", "c1", "run_shell"),
            make_tool_result("c1", "output"),
            make_assistant("done"),
        ];
        let removed = undo_last_turn(&mut history);
        // Should remove everything after the user message
        assert_eq!(removed, 3);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content.as_deref(), Some("test"));
    }

    #[test]
    fn undo_removes_trailing_user_message() {
        let mut history = vec![make_user("hello"), make_assistant("hi"), make_user("bye")];
        let removed = undo_last_turn(&mut history);
        assert_eq!(removed, 1);
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn undo_empty_history() {
        let mut history: Vec<Message> = Vec::new();
        let removed = undo_last_turn(&mut history);
        assert_eq!(removed, 0);
    }

    #[test]
    fn undo_single_user_message() {
        let mut history = vec![make_user("hello")];
        let removed = undo_last_turn(&mut history);
        assert_eq!(removed, 1);
        assert!(history.is_empty());
    }
}
