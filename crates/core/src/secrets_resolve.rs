use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;

/// A reference to a secret value that can be resolved at runtime.
/// Supports multiple backends: environment variables, files, and external commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source")]
pub enum SecretRef {
    /// Read from an environment variable.
    #[serde(rename = "env")]
    Env { var: String },

    /// Read from a file. Optionally extract a JSON key.
    #[serde(rename = "file")]
    File {
        path: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },

    /// Execute a command and use its stdout as the secret.
    #[serde(rename = "exec")]
    Exec {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl SecretRef {
    /// Resolve the secret reference to its plaintext value.
    pub fn resolve(&self) -> Result<String> {
        match self {
            SecretRef::Env { var } => {
                std::env::var(var).with_context(|| format!("Environment variable {var} not set"))
            }

            SecretRef::File { path, key } => {
                let expanded = shellexpand::tilde(path);
                let content = std::fs::read_to_string(expanded.as_ref())
                    .with_context(|| format!("Failed to read secret file: {path}"))?;

                match key {
                    Some(json_key) => {
                        let value: serde_json::Value = serde_json::from_str(&content)
                            .with_context(|| format!("Failed to parse {path} as JSON"))?;
                        value[json_key]
                            .as_str()
                            .map(std::string::ToString::to_string)
                            .with_context(|| {
                                format!("Key '{json_key}' not found or not a string in {path}")
                            })
                    }
                    None => Ok(content.trim().to_string()),
                }
            }

            SecretRef::Exec { command, args } => {
                let output = Command::new(command)
                    .args(args)
                    .output()
                    .with_context(|| format!("Failed to execute: {command}"))?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    bail!(
                        "Secret command `{command}` failed (exit {}): {stderr}",
                        output.status.code().unwrap_or(-1)
                    );
                }

                let secret = String::from_utf8(output.stdout)
                    .context("Secret command output is not valid UTF-8")?;
                Ok(secret.trim().to_string())
            }
        }
    }
}

/// Try to resolve the first successful key from a list of SecretRefs.
/// Returns the resolved key and the index that succeeded.
pub fn resolve_first(refs: &[SecretRef]) -> Result<(String, usize)> {
    let mut last_err = None;
    for (i, secret_ref) in refs.iter().enumerate() {
        match secret_ref.resolve() {
            Ok(key) if !key.is_empty() => return Ok((key, i)),
            Ok(_) => {
                last_err = Some(anyhow::anyhow!(
                    "SecretRef at index {i} resolved to empty string"
                ));
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("No secret references provided")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resolve_env_var() {
        let var_name = "TAMAGOTCHI_TEST_SECRET_REF_ENV";
        std::env::set_var(var_name, "test-secret-value");
        let sr = SecretRef::Env {
            var: var_name.to_string(),
        };
        let val = sr.resolve().expect("should resolve");
        assert_eq!(val, "test-secret-value");
        std::env::remove_var(var_name);
    }

    #[test]
    fn resolve_env_var_missing() {
        let sr = SecretRef::Env {
            var: "TAMAGOTCHI_TEST_SECRET_REF_MISSING".to_string(),
        };
        assert!(sr.resolve().is_err());
    }

    #[test]
    fn resolve_file_plaintext() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("secret.txt");
        std::fs::write(&file_path, "my-api-key\n").expect("write");

        let sr = SecretRef::File {
            path: file_path.to_string_lossy().to_string(),
            key: None,
        };
        let val = sr.resolve().expect("should resolve");
        assert_eq!(val, "my-api-key");
    }

    #[test]
    fn resolve_file_json_key() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("secrets.json");
        std::fs::write(&file_path, r#"{"api_key": "json-secret-123"}"#).expect("write");

        let sr = SecretRef::File {
            path: file_path.to_string_lossy().to_string(),
            key: Some("api_key".to_string()),
        };
        let val = sr.resolve().expect("should resolve");
        assert_eq!(val, "json-secret-123");
    }

    #[test]
    fn resolve_file_json_missing_key() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file_path = dir.path().join("secrets.json");
        std::fs::write(&file_path, r#"{"other": "value"}"#).expect("write");

        let sr = SecretRef::File {
            path: file_path.to_string_lossy().to_string(),
            key: Some("api_key".to_string()),
        };
        assert!(sr.resolve().is_err());
    }

    #[test]
    fn resolve_file_missing() {
        let sr = SecretRef::File {
            path: "/tmp/tamagotchi_test_nonexistent_secret_file.txt".to_string(),
            key: None,
        };
        assert!(sr.resolve().is_err());
    }

    #[test]
    fn resolve_exec_echo() {
        let sr = SecretRef::Exec {
            command: "echo".to_string(),
            args: vec!["exec-secret-value".to_string()],
        };
        let val = sr.resolve().expect("should resolve");
        assert_eq!(val, "exec-secret-value");
    }

    #[test]
    fn resolve_exec_failure() {
        let sr = SecretRef::Exec {
            command: "false".to_string(),
            args: vec![],
        };
        assert!(sr.resolve().is_err());
    }

    #[test]
    fn resolve_exec_nonexistent_command() {
        let sr = SecretRef::Exec {
            command: "tamagotchi_nonexistent_command_12345".to_string(),
            args: vec![],
        };
        assert!(sr.resolve().is_err());
    }

    #[test]
    fn resolve_first_picks_first_success() {
        let var_name = "TAMAGOTCHI_TEST_RESOLVE_FIRST";
        std::env::set_var(var_name, "first-key");
        let refs = vec![
            SecretRef::Env {
                var: "NONEXISTENT_VAR_12345".to_string(),
            },
            SecretRef::Env {
                var: var_name.to_string(),
            },
        ];
        let (val, idx) = resolve_first(&refs).expect("should resolve");
        assert_eq!(val, "first-key");
        assert_eq!(idx, 1);
        std::env::remove_var(var_name);
    }

    #[test]
    fn resolve_first_all_fail() {
        let refs = vec![
            SecretRef::Env {
                var: "NONEXISTENT_A_12345".to_string(),
            },
            SecretRef::Env {
                var: "NONEXISTENT_B_12345".to_string(),
            },
        ];
        assert!(resolve_first(&refs).is_err());
    }

    #[test]
    fn resolve_first_empty_list() {
        assert!(resolve_first(&[]).is_err());
    }

    #[test]
    fn serde_roundtrip_env() {
        let sr = SecretRef::Env {
            var: "MY_KEY".to_string(),
        };
        let toml_str = toml::to_string(&sr).expect("serialize");
        let parsed: SecretRef = toml::from_str(&toml_str).expect("deserialize");
        if let SecretRef::Env { var } = parsed {
            assert_eq!(var, "MY_KEY");
        } else {
            panic!("expected Env variant");
        }
    }

    #[test]
    fn serde_roundtrip_exec() {
        let sr = SecretRef::Exec {
            command: "security".to_string(),
            args: vec!["find-generic-password".to_string(), "-w".to_string()],
        };
        let toml_str = toml::to_string(&sr).expect("serialize");
        let parsed: SecretRef = toml::from_str(&toml_str).expect("deserialize");
        if let SecretRef::Exec { command, args } = parsed {
            assert_eq!(command, "security");
            assert_eq!(args.len(), 2);
        } else {
            panic!("expected Exec variant");
        }
    }

    #[test]
    fn deserialize_inline_table_env() {
        // This is the format used in config.toml inline tables
        let toml_str = r#"secret = { source = "env", var = "MY_API_KEY" }"#;
        #[derive(Deserialize)]
        struct Wrapper {
            secret: SecretRef,
        }
        let w: Wrapper = toml::from_str(toml_str).expect("parse");
        if let SecretRef::Env { var } = w.secret {
            assert_eq!(var, "MY_API_KEY");
        } else {
            panic!("expected Env variant");
        }
    }

    #[test]
    fn deserialize_inline_table_exec() {
        let toml_str = r#"secret = { source = "exec", command = "security", args = ["find-generic-password", "-s", "tamagotchi", "-w"] }"#;
        #[derive(Deserialize)]
        struct Wrapper {
            secret: SecretRef,
        }
        let w: Wrapper = toml::from_str(toml_str).expect("parse");
        if let SecretRef::Exec { command, args } = w.secret {
            assert_eq!(command, "security");
            assert_eq!(args.len(), 4);
        } else {
            panic!("expected Exec variant");
        }
    }
}
