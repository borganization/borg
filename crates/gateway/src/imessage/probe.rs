use std::fmt;

/// Result of probing iMessage / chat.db availability.
pub struct ProbeResult {
    pub status: ProbeStatus,
    pub max_rowid: Option<i64>,
}

pub enum ProbeStatus {
    /// chat.db is accessible and queryable.
    Ok,
    /// chat.db does not exist (not on macOS or Messages never opened).
    NoDb,
    /// chat.db exists but cannot be read (Full Disk Access not granted).
    NoDiskAccess,
    /// chat.db opened but query failed.
    QueryError(String),
}

impl fmt::Display for ProbeStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::NoDb => write!(f, "no chat.db found"),
            Self::NoDiskAccess => write!(
                f,
                "Full Disk Access required (System Settings > Privacy & Security)"
            ),
            Self::QueryError(e) => write!(f, "query error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_status_display_ok() {
        assert_eq!(ProbeStatus::Ok.to_string(), "ok");
    }

    #[test]
    fn probe_status_display_no_db() {
        assert_eq!(ProbeStatus::NoDb.to_string(), "no chat.db found");
    }

    #[test]
    fn probe_status_display_no_disk_access() {
        let s = ProbeStatus::NoDiskAccess.to_string();
        assert!(s.contains("Full Disk Access"));
    }

    #[test]
    fn probe_status_display_query_error() {
        let s = ProbeStatus::QueryError("table missing".to_string()).to_string();
        assert!(s.contains("query error"));
        assert!(s.contains("table missing"));
    }

    #[test]
    fn probe_result_fields() {
        let result = ProbeResult {
            status: ProbeStatus::Ok,
            max_rowid: Some(42),
        };
        assert!(matches!(result.status, ProbeStatus::Ok));
        assert_eq!(result.max_rowid, Some(42));
    }
}

/// Probe whether chat.db is accessible and return status + max ROWID.
pub fn probe_imessage() -> ProbeResult {
    let db_path = match dirs::home_dir() {
        Some(h) => h.join("Library/Messages/chat.db"),
        None => {
            return ProbeResult {
                status: ProbeStatus::NoDb,
                max_rowid: None,
            }
        }
    };

    if !db_path.exists() {
        return ProbeResult {
            status: ProbeStatus::NoDb,
            max_rowid: None,
        };
    }

    let db_uri = format!("file:{}?mode=ro", db_path.display());
    let conn = match rusqlite::Connection::open_with_flags(
        &db_uri,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(_) => {
            return ProbeResult {
                status: ProbeStatus::NoDiskAccess,
                max_rowid: None,
            }
        }
    };

    match conn.query_row("SELECT MAX(ROWID) FROM message", [], |row| {
        row.get::<_, Option<i64>>(0)
    }) {
        Ok(max_rowid) => ProbeResult {
            status: ProbeStatus::Ok,
            max_rowid: max_rowid.or(Some(0)),
        },
        Err(e) => ProbeResult {
            status: ProbeStatus::QueryError(e.to_string()),
            max_rowid: None,
        },
    }
}
