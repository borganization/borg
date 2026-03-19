use std::path::PathBuf;

/// Result of Chrome/Chromium executable detection.
pub struct ChromeDetection {
    /// The best candidate executable (first found in priority order).
    pub executable: Option<PathBuf>,
    /// All Chrome-like executables found on this system.
    pub all_found: Vec<PathBuf>,
}

/// Detect Chrome/Chromium executables on the system.
///
/// If `configured_path` is provided and the file exists, it is used as the primary
/// executable. Otherwise, platform-specific detection runs through known paths.
pub fn find_chrome(configured_path: Option<&str>) -> ChromeDetection {
    let mut all_found = Vec::new();

    // If a configured path is provided and exists, use it as primary.
    if let Some(path) = configured_path {
        let p = PathBuf::from(path);
        if p.exists() {
            all_found.push(p.clone());
            return ChromeDetection {
                executable: Some(p),
                all_found,
            };
        }
    }

    // Platform-specific known paths
    let candidates = platform_candidates();

    for candidate in &candidates {
        let p = PathBuf::from(candidate);
        if p.exists() {
            all_found.push(p);
        }
    }

    // Also check PATH via `which`
    let which_names = which_candidates();
    for name in &which_names {
        if let Ok(p) = which::which(name) {
            if !all_found.contains(&p) {
                all_found.push(p);
            }
        }
    }

    let executable = all_found.first().cloned();
    ChromeDetection {
        executable,
        all_found,
    }
}

/// Check whether the `agent-browser` CLI is available on PATH.
pub fn detect_agent_browser() -> bool {
    which::which("agent-browser").is_ok()
}

#[cfg(target_os = "macos")]
fn platform_candidates() -> Vec<&'static str> {
    vec![
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ]
}

#[cfg(target_os = "linux")]
fn platform_candidates() -> Vec<&'static str> {
    vec![]
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_candidates() -> Vec<&'static str> {
    vec![]
}

#[cfg(target_os = "macos")]
fn which_candidates() -> Vec<&'static str> {
    vec!["google-chrome", "chromium"]
}

#[cfg(target_os = "linux")]
fn which_candidates() -> Vec<&'static str> {
    vec![
        "google-chrome-stable",
        "google-chrome",
        "chromium-browser",
        "chromium",
    ]
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn which_candidates() -> Vec<&'static str> {
    vec!["google-chrome", "chromium"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_agent_browser_returns_bool() {
        // Should not panic regardless of whether agent-browser is installed.
        let _result = detect_agent_browser();
    }

    #[test]
    fn find_chrome_with_no_configured_path() {
        let detection = find_chrome(None);
        // Struct should be well-formed; executable may or may not be present.
        if let Some(ref exe) = detection.executable {
            assert!(detection.all_found.contains(exe));
        }
    }

    #[test]
    fn find_chrome_with_invalid_configured_path() {
        let detection = find_chrome(Some("/nonexistent/path/to/chrome"));
        // Should fall through gracefully to platform detection.
        assert!(!detection
            .all_found
            .contains(&PathBuf::from("/nonexistent/path/to/chrome")));
    }

    #[test]
    fn find_chrome_with_valid_configured_path() {
        // Use /bin/sh as a stand-in for a valid executable path.
        let detection = find_chrome(Some("/bin/sh"));
        assert_eq!(detection.executable, Some(PathBuf::from("/bin/sh")));
        assert!(detection.all_found.contains(&PathBuf::from("/bin/sh")));
    }
}
