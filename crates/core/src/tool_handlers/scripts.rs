use anyhow::Result;
use tracing::instrument;

use crate::config::Config;
use crate::db::Database;

use super::require_str_param;

pub fn handle_manage_scripts(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.scripts.enabled {
        return Ok("Scripts system is disabled.".to_string());
    }
    let action = require_str_param(args, "action")?;
    let db = Database::open()?;

    match action {
        "create" => {
            let name = require_str_param(args, "name")?;
            let patch = require_str_param(args, "patch")?;
            let description = args["description"].as_str().unwrap_or("");
            let runtime = args["runtime"].as_str().unwrap_or("python");
            let entrypoint = args["entrypoint"].as_str().unwrap_or("main.py");
            let sandbox_profile = args["sandbox_profile"]
                .as_str()
                .unwrap_or(&config.scripts.default_sandbox_profile);
            let network_access = args["network_access"].as_bool().unwrap_or(false);
            let fs_read: Vec<String> = args["fs_read"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let fs_write: Vec<String> = args["fs_write"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let ephemeral = args["ephemeral"].as_bool().unwrap_or(false);

            crate::scripts::create_script(
                &db,
                &crate::scripts::CreateScriptParams {
                    name,
                    description,
                    patch,
                    runtime,
                    entrypoint,
                    sandbox_profile,
                    network_access,
                    fs_read: &fs_read,
                    fs_write: &fs_write,
                    ephemeral,
                    max_scripts: config.scripts.max_scripts,
                },
            )
        }
        "update" => {
            let name = require_str_param(args, "name")?;
            let patch = require_str_param(args, "patch")?;
            crate::scripts::update_script(&db, name, patch)
        }
        "delete" => {
            let name = require_str_param(args, "name")?;
            crate::scripts::delete_script(&db, name)
        }
        "get" => {
            let name = require_str_param(args, "name")?;
            crate::scripts::get_script(&db, name)
        }
        "list" => crate::scripts::list_scripts(&db),
        other => Ok(format!(
            "Unknown action: {other}. Use: create, list, get, update, delete."
        )),
    }
}

#[instrument(skip_all, fields(tool.name = "run_script"))]
pub async fn handle_run_script(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.scripts.enabled {
        return Ok("Scripts system is disabled.".to_string());
    }
    let name = require_str_param(args, "name")?;
    let script_args = args.get("args").cloned().unwrap_or(serde_json::json!({}));
    crate::scripts::execute_script(config, name, &script_args.to_string()).await
}

pub fn handle_security_audit(args: &serde_json::Value, config: &Config) -> Result<String> {
    if !config.security.host_audit {
        return Ok(
            "Host audit is disabled. Enable it in config: security.host_audit = true".to_string(),
        );
    }
    use crate::doctor::DiagnosticReport;
    use crate::host_audit;

    let mut audit_checks = Vec::new();
    match args.get("category").and_then(|v| v.as_str()) {
        Some("firewall") => host_audit::check_firewall(&mut audit_checks),
        Some("ports") => host_audit::check_listening_ports(&mut audit_checks),
        Some("ssh") => host_audit::check_ssh_config(&mut audit_checks),
        Some("permissions") => host_audit::check_sensitive_permissions(&mut audit_checks),
        Some("encryption") => host_audit::check_disk_encryption(&mut audit_checks),
        Some("updates") => host_audit::check_os_updates(&mut audit_checks),
        Some("services") => host_audit::check_running_services(&mut audit_checks),
        Some(other) => {
            return Ok(format!(
                "Unknown audit category: {other}. Valid: firewall, ports, ssh, permissions, encryption, updates, services"
            ))
        }
        None => host_audit::run_host_security_checks(&mut audit_checks),
    }
    let report = DiagnosticReport {
        checks: audit_checks,
    };
    Ok(report.format())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn handle_security_audit_disabled() {
        let mut config = Config::default();
        config.security.host_audit = false;
        let result = handle_security_audit(&json!({}), &config).unwrap();
        assert!(result.contains("disabled"));
    }

    #[test]
    fn handle_security_audit_unknown_category() {
        let mut config = Config::default();
        config.security.host_audit = true;
        let result = handle_security_audit(&json!({"category": "invalid"}), &config).unwrap();
        assert!(result.contains("Unknown audit category"));
    }
}
