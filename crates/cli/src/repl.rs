use anyhow::Result;
use std::io::{self, Write};
use tokio::sync::mpsc;

use borg_core::agent::{Agent, AgentEvent};
use borg_core::config::Config;

pub async fn run() -> Result<()> {
    crate::tui::run().await
}

pub async fn one_shot(message: &str, auto_approve: bool, json_output: bool) -> Result<()> {
    let config = Config::load()?;
    let mut agent = Agent::new(config)?;

    let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(256);

    let send_future = agent.send_message(message, event_tx);

    let render_handle = tokio::spawn(async move {
        let mut stdout = io::stdout();
        let mut full_response = String::new();
        let mut exit_code: i32 = 0;

        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TextDelta(delta) => {
                    if json_output {
                        full_response.push_str(&delta);
                    } else {
                        print!("{delta}");
                        let _ = stdout.flush();
                    }
                }
                AgentEvent::TurnComplete => {
                    if json_output {
                        let output = serde_json::json!({
                            "response": full_response,
                            "exit_code": exit_code,
                        });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&output).unwrap_or_default()
                        );
                    } else {
                        println!();
                    }
                }
                AgentEvent::ShellConfirmation { respond, command } => {
                    if auto_approve {
                        let _ = respond.send(true);
                        if !json_output {
                            eprintln!("[auto-approved] {command}");
                        }
                    } else {
                        let _ = respond.send(false);
                        if !json_output {
                            eprintln!("Shell command denied in one-shot mode. Use --yes to allow.");
                        }
                    }
                }
                AgentEvent::ToolConfirmation {
                    respond,
                    tool_name,
                    reason,
                } => {
                    if auto_approve {
                        let _ = respond.send(true);
                        if !json_output {
                            eprintln!("[auto-approved] {tool_name}: {reason}");
                        }
                    } else {
                        let _ = respond.send(false);
                        if !json_output {
                            eprintln!(
                                "Dangerous operation denied in one-shot mode ({tool_name}: {reason}). Use --yes to allow."
                            );
                        }
                    }
                }
                AgentEvent::Error(e) => {
                    eprintln!("Error: {e}");
                    exit_code = 1;
                }
                AgentEvent::ToolExecuting { name, .. } => {
                    if !json_output {
                        eprintln!("[running {name}]");
                    }
                }
                AgentEvent::ToolResult { name, result } => {
                    if !json_output {
                        let preview = if result.len() > 200 {
                            &result[..200]
                        } else {
                            &result
                        };
                        eprintln!("[{name} done] {preview}");
                    }
                }
                AgentEvent::ToolOutputDelta {
                    delta, is_stderr, ..
                } => {
                    if !json_output {
                        let prefix = if is_stderr { "! " } else { "" };
                        eprintln!("  {prefix}{delta}");
                    }
                }
                _ => {}
            }
        }

        exit_code
    });

    if let Err(e) = send_future.await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    let exit_code = render_handle.await.unwrap_or(1);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}
