use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

const GITHUB_API_RELEASES_LATEST: &str =
    "https://api.github.com/repos/borganization/borg/releases/latest";

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

enum UpdateCheck {
    AlreadyUpToDate(Version),
    UpdateAvailable {
        current: Version,
        latest: Version,
        release: GitHubRelease,
    },
}

fn detect_os() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("darwin")
    } else if cfg!(target_os = "linux") {
        Ok("linux")
    } else {
        bail!("Unsupported OS for self-update")
    }
}

fn detect_arch() -> Result<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Ok("x86_64")
    } else if cfg!(target_arch = "aarch64") {
        Ok("arm64")
    } else {
        bail!("Unsupported architecture for self-update")
    }
}

fn parse_version(tag: &str) -> Result<Version> {
    let stripped = tag.strip_prefix('v').unwrap_or(tag);
    Version::parse(stripped).with_context(|| format!("Invalid version: {tag}"))
}

fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("borg-update")
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .context("Failed to build HTTP client")
}

async fn fetch_latest_release(client: &reqwest::Client, api_url: &str) -> Result<GitHubRelease> {
    let resp = client
        .get(api_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .context("Failed to fetch latest release")?;

    if !resp.status().is_success() {
        bail!(
            "GitHub API returned {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        );
    }

    resp.json::<GitHubRelease>()
        .await
        .context("Failed to parse release JSON")
}

async fn check_for_update(client: &reqwest::Client, api_url: &str) -> Result<UpdateCheck> {
    let current = parse_version(env!("CARGO_PKG_VERSION"))?;
    let release = fetch_latest_release(client, api_url).await?;
    let latest = parse_version(&release.tag_name)?;

    if current >= latest {
        Ok(UpdateCheck::AlreadyUpToDate(current))
    } else {
        Ok(UpdateCheck::UpdateAvailable {
            current,
            latest,
            release,
        })
    }
}

async fn download_asset(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("Failed to download asset")?;

    if !resp.status().is_success() {
        bail!("Download failed with status {}", resp.status());
    }

    let bytes = resp.bytes().await.context("Failed to read download")?;
    std::fs::write(dest, &bytes).context("Failed to write downloaded file")?;
    Ok(())
}

fn parse_checksums(content: &str, asset_name: &str) -> Option<String> {
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() == 2 && parts[1] == asset_name {
            return Some(parts[0].to_string());
        }
    }
    None
}

fn verify_checksum(file_path: &Path, expected_hex: &str) -> Result<()> {
    let bytes = std::fs::read(file_path).context("Failed to read file for checksum")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected_hex.to_lowercase() {
        bail!("Checksum mismatch: expected {expected_hex}, got {actual}");
    }
    Ok(())
}

fn replace_binary(new_binary: &Path, current_binary: &Path) -> Result<()> {
    let backup = current_binary.with_extension("old");

    // Rename current binary out of the way (Unix keeps the fd open)
    std::fs::rename(current_binary, &backup).context("Failed to rename current binary")?;

    // Move new binary into place; fall back to copy for cross-filesystem
    let move_result = if std::fs::rename(new_binary, current_binary).is_err() {
        std::fs::copy(new_binary, current_binary)
            .map(|_| ())
            .context("Failed to copy new binary into place")
    } else {
        Ok(())
    };

    if let Err(e) = move_result {
        // Rollback: restore backup
        if let Err(rb) = std::fs::rename(&backup, current_binary) {
            eprintln!(
                "CRITICAL: rollback failed: {rb}. Restore manually from {}",
                backup.display()
            );
        }
        return Err(e);
    }

    // Set executable permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(current_binary, std::fs::Permissions::from_mode(0o755))?;
    }

    // Clean up backup (non-fatal)
    let _ = std::fs::remove_file(&backup);

    Ok(())
}

pub async fn run_update() -> Result<()> {
    println!("Checking for updates...");

    let client = build_client()?;
    let check = check_for_update(&client, GITHUB_API_RELEASES_LATEST).await?;

    let (current, latest, release) = match check {
        UpdateCheck::AlreadyUpToDate(v) => {
            println!("Already up to date (v{v}).");
            return Ok(());
        }
        UpdateCheck::UpdateAvailable {
            current,
            latest,
            release,
        } => (current, latest, release),
    };

    println!("Updating v{current} → v{latest}...");

    let os = detect_os()?;
    let arch = detect_arch()?;
    let asset_name = format!("borg-{os}-{arch}.tar.gz");

    let asset_url = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .map(|a| a.browser_download_url.clone())
        .with_context(|| format!("No release asset found for {asset_name}"))?;

    let tmp = tempfile::tempdir().context("Failed to create temp directory")?;
    let tarball_path = tmp.path().join(&asset_name);

    // Download tarball
    println!("Downloading {asset_name}...");
    download_asset(&client, &asset_url, &tarball_path).await?;

    // Download and verify checksum
    let checksums_asset = release.assets.iter().find(|a| a.name == "checksums.txt");
    match checksums_asset {
        Some(cs) => {
            let cs_path = tmp.path().join("checksums.txt");
            download_asset(&client, &cs.browser_download_url, &cs_path).await?;
            let cs_content = std::fs::read_to_string(&cs_path)?;
            match parse_checksums(&cs_content, &asset_name) {
                Some(expected) => {
                    verify_checksum(&tarball_path, &expected)?;
                    println!("Checksum verified.");
                }
                None => bail!("Checksum for {asset_name} not found in checksums.txt"),
            }
        }
        None => {
            eprintln!("Warning: no checksums.txt in release, skipping integrity verification");
        }
    }

    // Extract tarball (--no-same-owner avoids permission issues)
    let extract_dir = tmp.path().join("extract");
    std::fs::create_dir_all(&extract_dir)?;
    let status = std::process::Command::new("tar")
        .args([
            "xzf",
            &tarball_path.to_string_lossy(),
            "--no-same-owner",
            "-C",
        ])
        .arg(&extract_dir)
        .status()
        .context("Failed to run tar")?;

    if !status.success() {
        bail!("tar extraction failed");
    }

    // Find extracted binary and verify it's inside the extract dir
    let new_binary = extract_dir.join("borg");
    if !new_binary.exists() {
        bail!("Extracted archive does not contain 'borg' binary");
    }
    let canonical = new_binary.canonicalize()?;
    let canonical_dir = extract_dir.canonicalize()?;
    if !canonical.starts_with(&canonical_dir) {
        bail!("Extracted binary path escapes temp directory");
    }

    // Replace current binary
    let current_exe = std::env::current_exe().context("Failed to determine current binary path")?;
    replace_binary(&new_binary, &current_exe)?;

    // Sync bundled skills
    let data_dir = dirs::home_dir().context("No home directory")?.join(".borg");
    if data_dir.exists() {
        match borg_core::skills::install_default_skills(&data_dir) {
            Ok(n) if n > 0 => println!("Synced {n} new skill(s)."),
            Ok(_) => {}
            Err(e) => eprintln!("Warning: failed to sync skills: {e}"),
        }
    }

    println!("Updated to v{latest} successfully.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("v1.2.3").unwrap(), Version::new(1, 2, 3));
        assert_eq!(parse_version("0.2.0").unwrap(), Version::new(0, 2, 0));
        assert_eq!(
            parse_version("v10.20.30").unwrap(),
            Version::new(10, 20, 30)
        );
    }

    #[test]
    fn test_parse_version_invalid() {
        assert!(parse_version("bad").is_err());
        assert!(parse_version("").is_err());
        assert!(parse_version("v").is_err());
    }

    #[test]
    fn test_detect_os() {
        let os = detect_os().unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(os, "darwin");
        } else if cfg!(target_os = "linux") {
            assert_eq!(os, "linux");
        }
    }

    #[test]
    fn test_detect_arch() {
        let arch = detect_arch().unwrap();
        assert!(
            arch == "x86_64" || arch == "arm64",
            "unexpected arch: {arch}"
        );
    }

    #[test]
    fn test_verify_checksum_valid() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        f.flush().unwrap();

        let mut hasher = Sha256::new();
        hasher.update(b"hello world");
        let expected = format!("{:x}", hasher.finalize());

        verify_checksum(f.path(), &expected).unwrap();
    }

    #[test]
    fn test_verify_checksum_mismatch() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello world").unwrap();
        f.flush().unwrap();

        let result = verify_checksum(
            f.path(),
            "0000000000000000000000000000000000000000000000000000000000000000",
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Checksum mismatch"),
            "expected checksum mismatch error"
        );
    }

    #[test]
    fn test_parse_checksums() {
        let content = "abc123  borg-darwin-arm64.tar.gz\ndef456  borg-linux-x86_64.tar.gz\n";
        assert_eq!(
            parse_checksums(content, "borg-darwin-arm64.tar.gz"),
            Some("abc123".to_string())
        );
        assert_eq!(
            parse_checksums(content, "borg-linux-x86_64.tar.gz"),
            Some("def456".to_string())
        );
        assert_eq!(parse_checksums(content, "nonexistent.tar.gz"), None);
    }

    #[test]
    fn test_replace_binary() {
        let dir = tempfile::tempdir().unwrap();
        let current = dir.path().join("borg");
        let new = dir.path().join("borg-new");

        std::fs::write(&current, b"old binary").unwrap();
        std::fs::write(&new, b"new binary").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&current, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        replace_binary(&new, &current).unwrap();

        assert_eq!(std::fs::read(&current).unwrap(), b"new binary");
        assert!(!dir.path().join("borg.old").exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&current).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o755);
        }
    }

    #[tokio::test]
    async fn test_fetch_latest_release() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.2.0",
                "assets": [
                    {
                        "name": "borg-darwin-arm64.tar.gz",
                        "browser_download_url": format!("{}/dl/borg-darwin-arm64.tar.gz", server.uri())
                    },
                    {
                        "name": "checksums.txt",
                        "browser_download_url": format!("{}/dl/checksums.txt", server.uri())
                    }
                ]
            })))
            .mount(&server)
            .await;

        let client = build_client().unwrap();
        let url = format!("{}/releases/latest", server.uri());
        let release = fetch_latest_release(&client, &url).await.unwrap();

        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.assets.len(), 2);
        assert_eq!(release.assets[0].name, "borg-darwin-arm64.tar.gz");
    }

    #[tokio::test]
    async fn test_check_for_update_newer_available() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v99.0.0",
                "assets": []
            })))
            .mount(&server)
            .await;

        let client = build_client().unwrap();
        let url = format!("{}/releases/latest", server.uri());
        let result = check_for_update(&client, &url).await.unwrap();

        assert!(matches!(result, UpdateCheck::UpdateAvailable { .. }));
        if let UpdateCheck::UpdateAvailable { latest, .. } = result {
            assert_eq!(latest, Version::new(99, 0, 0));
        }
    }

    #[tokio::test]
    async fn test_check_for_update_already_up_to_date() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let current = env!("CARGO_PKG_VERSION");

        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": format!("v{current}"),
                "assets": []
            })))
            .mount(&server)
            .await;

        let client = build_client().unwrap();
        let url = format!("{}/releases/latest", server.uri());
        let result = check_for_update(&client, &url).await.unwrap();

        assert!(matches!(result, UpdateCheck::AlreadyUpToDate(_)));
    }

    #[tokio::test]
    async fn test_download_asset() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/dl/test.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"binary content"))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("test.bin");

        let client = build_client().unwrap();
        download_asset(&client, &format!("{}/dl/test.bin", server.uri()), &dest)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"binary content");
    }
}
