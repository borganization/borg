use ratatui::style::Color;
use std::sync::OnceLock;

/// Cached terminal background: `Some(true)` = light, `Some(false)` = dark, `None` = unknown.
static TERMINAL_IS_LIGHT: OnceLock<Option<bool>> = OnceLock::new();

/// Detect whether the terminal has a light background.
/// Uses the `COLORFGBG` environment variable (set by rxvt-family terminals and some others).
/// Format is typically "fg;bg" where the bg value is a palette index.
/// Returns `None` if detection fails — callers should skip background styling.
pub(super) fn query_terminal_bg() {
    TERMINAL_IS_LIGHT.get_or_init(|| {
        if let Ok(val) = std::env::var("COLORFGBG") {
            if let Some(bg_str) = val.rsplit(';').next() {
                if let Ok(bg_idx) = bg_str.trim().parse::<u8>() {
                    // Standard 16-color palette: 0-6 are dark colors,
                    // 7 is light gray, 8 is dark gray, 9-15 are bright/light.
                    return Some(bg_idx == 7 || bg_idx >= 9);
                }
            }
        }
        None
    });
}

/// Alpha-blend a foreground value onto a background value.
fn blend_channel(fg: u8, bg: u8, alpha: f64) -> u8 {
    let v = fg as f64 * alpha + bg as f64 * (1.0 - alpha);
    v.round().clamp(0.0, 255.0) as u8
}

/// Compute the user message background color.
/// Light terminals: 4% black overlay (assume base ~240).
/// Dark terminals: 12% white overlay (assume base ~30).
/// Returns `None` when terminal background is unknown.
pub(super) fn user_message_bg() -> Option<Color> {
    let is_light = TERMINAL_IS_LIGHT.get().copied().flatten()?;
    if is_light {
        let v = blend_channel(0, 240, 0.04);
        Some(Color::Rgb(v, v, v))
    } else {
        let v = blend_channel(255, 30, 0.12);
        Some(Color::Rgb(v, v, v))
    }
}
