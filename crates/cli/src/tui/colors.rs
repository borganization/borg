use ratatui::style::Color;
use std::sync::OnceLock;

/// Cached terminal background: `Some(true)` = light, `Some(false)` = dark, `None` = unknown.
static TERMINAL_IS_LIGHT: OnceLock<Option<bool>> = OnceLock::new();

/// Detect whether the terminal has a light background.
/// Uses the `COLORFGBG` environment variable (set by many terminals).
/// Format is typically "fg;bg" where bg >= 8 is often light.
/// Falls back to `None` (unknown) if detection fails.
pub fn query_terminal_bg() {
    TERMINAL_IS_LIGHT.get_or_init(|| {
        if let Ok(val) = std::env::var("COLORFGBG") {
            if let Some(bg_str) = val.rsplit(';').next() {
                if let Ok(bg_idx) = bg_str.trim().parse::<u8>() {
                    // In the standard 16-color palette, indices 0-6 are dark, 7-15 are light.
                    // Index 8 is "bright black" (dark gray) — still dark.
                    return Some(bg_idx >= 9);
                }
            }
        }
        // Default: assume dark terminal (most common)
        Some(false)
    });
}

fn is_light() -> bool {
    TERMINAL_IS_LIGHT
        .get()
        .copied()
        .flatten()
        .unwrap_or(false)
}

/// Alpha-blend a foreground channel onto a background channel.
fn blend_channel(fg: u8, bg: u8, alpha: f64) -> u8 {
    let v = fg as f64 * alpha + bg as f64 * (1.0 - alpha);
    v.round().clamp(0.0, 255.0) as u8
}

/// Compute the user message background color.
/// Light terminals: 4% black overlay. Dark terminals: 12% white overlay.
pub fn user_message_bg() -> Option<Color> {
    // We always have a detection result (defaults to dark).
    if is_light() {
        // 4% black over white-ish background (assume ~240,240,240)
        let base: u8 = 240;
        let r = blend_channel(0, base, 0.04);
        let g = blend_channel(0, base, 0.04);
        let b = blend_channel(0, base, 0.04);
        Some(Color::Rgb(r, g, b))
    } else {
        // 12% white over dark background (assume ~30,30,30)
        let base: u8 = 30;
        let r = blend_channel(255, base, 0.12);
        let g = blend_channel(255, base, 0.12);
        let b = blend_channel(255, base, 0.12);
        Some(Color::Rgb(r, g, b))
    }
}
