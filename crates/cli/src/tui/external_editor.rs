use std::io::Write;
use std::process::Command;

use anyhow::{bail, Result};

/// Resolve the user's preferred editor from $VISUAL or $EDITOR, falling back to "vi".
fn resolve_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(windows) {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        })
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
    let default_editor = if cfg!(windows) { "notepad" } else { "vi" };
    let (cmd, args) = parts.split_first().unwrap_or((&default_editor, &[]));

    let status = Command::new(cmd).args(args).arg(&path).status()?;

    if !status.success() {
        bail!("Editor exited with status: {status}");
    }

    let content = std::fs::read_to_string(&path)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_defaults_to_platform_editor() {
        // Temporarily unset VISUAL and EDITOR to test fallback
        let old_visual = std::env::var("VISUAL").ok();
        let old_editor = std::env::var("EDITOR").ok();
        std::env::remove_var("VISUAL");
        std::env::remove_var("EDITOR");

        let editor = resolve_editor();
        if cfg!(windows) {
            assert_eq!(editor, "notepad");
        } else {
            assert_eq!(editor, "vi");
        }

        // Restore
        if let Some(v) = old_visual {
            std::env::set_var("VISUAL", v);
        }
        if let Some(e) = old_editor {
            std::env::set_var("EDITOR", e);
        }
    }

    #[test]
    fn resolve_editor_uses_visual_first() {
        let old_visual = std::env::var("VISUAL").ok();
        let old_editor = std::env::var("EDITOR").ok();
        std::env::set_var("VISUAL", "nvim");
        std::env::set_var("EDITOR", "nano");

        let editor = resolve_editor();
        assert_eq!(editor, "nvim");

        // Restore
        match old_visual {
            Some(v) => std::env::set_var("VISUAL", v),
            None => std::env::remove_var("VISUAL"),
        }
        match old_editor {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    }

    #[test]
    fn resolve_editor_falls_back_to_editor() {
        let old_visual = std::env::var("VISUAL").ok();
        let old_editor = std::env::var("EDITOR").ok();
        std::env::remove_var("VISUAL");
        std::env::set_var("EDITOR", "emacs");

        let editor = resolve_editor();
        assert_eq!(editor, "emacs");

        // Restore
        match old_visual {
            Some(v) => std::env::set_var("VISUAL", v),
            None => std::env::remove_var("VISUAL"),
        }
        match old_editor {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    }

    #[test]
    fn open_external_editor_with_nonexistent_editor_returns_error() {
        let old_visual = std::env::var("VISUAL").ok();
        let old_editor = std::env::var("EDITOR").ok();
        std::env::set_var("VISUAL", "nonexistent_editor_binary_xyz_42");
        std::env::remove_var("EDITOR");

        let result = open_external_editor("test content");
        assert!(result.is_err());

        // Restore
        match old_visual {
            Some(v) => std::env::set_var("VISUAL", v),
            None => std::env::remove_var("VISUAL"),
        }
        match old_editor {
            Some(v) => std::env::set_var("EDITOR", v),
            None => std::env::remove_var("EDITOR"),
        }
    }
}
