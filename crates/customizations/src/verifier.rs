use crate::catalog::CustomizationDef;

/// Result of a verification check.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub id: String,
    pub ok: bool,
    pub message: String,
}

/// Verify that an installed customization is working.
///
/// For now, this checks that:
/// 1. Required binaries are available
/// 2. Required credentials are resolvable (env var or keychain)
///
/// Full API health checks (e.g., calling Telegram getMe) are done per-integration
/// when the credentials are available.
pub fn verify(def: &CustomizationDef, data_dir: &std::path::Path) -> VerifyResult {
    let id = def.id.to_string();

    // Check binaries
    for bin in def.required_bins {
        if which::which(bin).is_err() {
            return VerifyResult {
                id,
                ok: false,
                message: format!("Required binary not found: {bin}"),
            };
        }
    }

    // Check that files exist
    if !crate::installer::is_installed(def, data_dir) {
        return VerifyResult {
            id,
            ok: false,
            message: "Integration files not found".to_string(),
        };
    }

    // Check credentials are available (via env or keychain)
    for cred in def.required_credentials {
        if cred.is_optional {
            continue;
        }

        let has_env = std::env::var(cred.key).is_ok();
        let has_keychain = check_keychain_credential(def, cred.key);

        if !has_env && !has_keychain {
            return VerifyResult {
                id,
                ok: false,
                message: format!(
                    "Credential {} not found in environment or keychain",
                    cred.key
                ),
            };
        }
    }

    VerifyResult {
        id,
        ok: true,
        message: "All checks passed".to_string(),
    }
}

/// Verify all installed customizations and return results.
pub fn verify_all(installed_ids: &[String], data_dir: &std::path::Path) -> Vec<VerifyResult> {
    installed_ids
        .iter()
        .filter_map(|id| crate::catalog::find_by_id(id).map(|def| verify(def, data_dir)))
        .collect()
}

fn check_keychain_credential(def: &CustomizationDef, key: &str) -> bool {
    let service = format!("tamagotchi-{}", def.id.replace('/', "-"));
    let account = format!("tamagotchi-{key}");
    crate::keychain::check(&service, &account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::CATALOG;

    #[test]
    fn verify_uninstalled_fails() {
        let def = &CATALOG[0];
        let tmp = std::env::temp_dir().join("tamagotchi-verify-test");
        let result = verify(def, &tmp);
        assert!(!result.ok);
    }
}
