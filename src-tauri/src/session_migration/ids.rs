//! Import object identity. `imp:<uuid>` never enters native session APIs.

use uuid::Uuid;

use super::errors::{ErrorCode, ImportError};

pub const IMPORT_ID_PREFIX: &str = "imp:";

pub fn new_import_id() -> String {
    format!("{IMPORT_ID_PREFIX}{}", Uuid::new_v4())
}

pub fn is_import_id(id: &str) -> bool {
    let t = id.trim();
    t.len() > IMPORT_ID_PREFIX.len()
        && t.starts_with(IMPORT_ID_PREFIX)
        && Uuid::parse_str(&t[IMPORT_ID_PREFIX.len()..]).is_ok()
}

/// Filesystem-safe directory name. `imp:<uuid>` is not a valid Windows path.
pub fn import_id_dir_component(id: &str) -> String {
    id.replace(':', "_")
}

pub fn parse_import_uuid(id: &str) -> Result<Uuid, ImportError> {
    let t = id.trim();
    let rest = t
        .strip_prefix(IMPORT_ID_PREFIX)
        .ok_or_else(|| ImportError::new(ErrorCode::ImportedHistoryReadOnly, "not an import id"))?;
    Uuid::parse_str(rest).map_err(|_| {
        ImportError::new(
            ErrorCode::ImportedHistoryReadOnly,
            "import id is not a uuid",
        )
    })
}

/// Native connect/send/rewind/automation/IM must call this first.
pub fn reject_imported_history_id(id: Option<&str>) -> Result<(), String> {
    let Some(id) = id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if is_import_id(id) {
        return Err(ImportError::new(
            ErrorCode::ImportedHistoryReadOnly,
            "imported history cannot be used with native session commands",
        )
        .into_command_err());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_ids_roundtrip() {
        let id = new_import_id();
        assert!(is_import_id(&id));
        assert!(parse_import_uuid(&id).is_ok());
        assert!(!is_import_id("not-an-import"));
        assert!(!is_import_id("imp:not-a-uuid"));
        assert!(!is_import_id(&Uuid::new_v4().to_string()));
    }

    #[test]
    fn native_guard_rejects_import_ids_only() {
        let native = Uuid::new_v4().to_string();
        assert!(reject_imported_history_id(None).is_ok());
        assert!(reject_imported_history_id(Some(&native)).is_ok());
        let err = reject_imported_history_id(Some(&new_import_id())).unwrap_err();
        assert!(err.starts_with("IMPORTED_HISTORY_READ_ONLY"));
        assert!(
            !err.contains("Agent"),
            "guard must not start an Agent: {err}"
        );
    }
}
