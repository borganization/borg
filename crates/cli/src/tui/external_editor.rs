use std::io::Write;
use std::process::Command;

use anyhow::{bail, Result};

/// Resolve the user's preferred editor from $VISUAL or $EDITOR, falling back to "vi".
fn resolve_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string())
}

/// Open the user's external editor with `initial_text` pre-filled.
/// Returns the edited text, or an error if the editor failed.
pub fn open_external_editor(initial_text: &str) -> Result<String> {
    let editor = resolve_editor();

    // Create a temp file with initial content
    let mut tmpfile = tempfile::Builder::new()
        .prefix("borg-")
        .suffix(".md")
        .tempfile()?;
    tmpfile.write_all(initial_text.as_bytes())?;
    tmpfile.flush()?;
    let path = tmpfile.path().to_path_buf();

    // Split editor command in case it contains args (e.g. "code --wait")
    let parts: Vec<&str> = editor.split_whitespace().collect();
    let (cmd, args) = parts.split_first().unwrap_or((&"vi", &[]));

    let status = Command::new(cmd).args(args).arg(&path).status()?;

    if !status.success() {
        bail!("Editor exited with status: {status}");
    }

    let content = std::fs::read_to_string(&path)?;
    Ok(content)
}
