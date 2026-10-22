//! Typed Cursor native facts. Unproven Store paths are location identities,
//! not claims that a directory basename is a Composer/session ID.
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::hash::options_hash;
use serde_json::json;
use std::path::Path;

const NAMESPACE: &str = "cursor-identity-v1:";
const ID_MAX_BYTES: usize = 4096;

/// Only adapter-proved declaration families can link formats. Locator and
/// subagent identities deliberately do not qualify as cross-format evidence.
pub(crate) fn declared_family(identity: &str) -> Option<&'static str> {
    let typed = identity.strip_prefix(NAMESPACE)?;
    for family in ["session", "composer", "legacy-chat"] {
        if let Some(raw) = typed.strip_prefix(family).and_then(|s| s.strip_prefix(':')) {
            if !raw.trim().is_empty()
                && raw.len() <= ID_MAX_BYTES
                && !raw.chars().any(char::is_control)
            {
                return Some(family);
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeFamily {
    Session,
    Composer,
    LegacyChat,
}

impl NativeFamily {
    pub(super) fn declared(self, id: &str) -> Result<String, ImportError> {
        if id.len() > ID_MAX_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "Cursor session identity exceeds metadata budget",
            ));
        }
        if id.trim().is_empty() || id.chars().any(char::is_control) {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "Cursor declares an invalid session identity",
            ));
        }
        let family = match self {
            Self::Session => "session",
            Self::Composer => "composer",
            Self::LegacyChat => "legacy-chat",
        };
        // The entire opaque raw ID remains after a fixed family prefix. Even
        // an ID containing another namespace cannot forge a different family.
        Ok(format!("{NAMESPACE}{family}:{id}"))
    }
}

pub(crate) fn database_identity(key: &str) -> Result<String, ImportError> {
    if let Some(id) = key.strip_prefix("composerData:") {
        NativeFamily::Composer.declared(id)
    } else if let Some(id) = key.strip_prefix("chatData:") {
        NativeFamily::LegacyChat.declared(id)
    } else {
        Err(ImportError::new(
            ErrorCode::FormatUnsupported,
            "Cursor database key has no supported identity family",
        ))
    }
}

pub(super) fn locator_identity(
    family: &str,
    path: &Path,
    root: Option<&Path>,
) -> Result<String, ImportError> {
    locator_identity_scoped(family, path, root, None)
}

pub(super) fn locator_identity_scoped(
    family: &str,
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<String, ImportError> {
    let relative = match root {
        Some(root) => path.strip_prefix(root).map_err(|_| {
            ImportError::new(
                ErrorCode::FormatUnsupported,
                "Cursor artifact is outside its source root",
            )
        })?,
        None => path,
    };
    let locator = relative.to_str().filter(|s| !s.is_empty()).ok_or_else(|| {
        ImportError::new(
            ErrorCode::FormatUnsupported,
            "Cursor artifact has no representable source locator",
        )
    })?;
    // Backslash is a separator only on Windows, not in Unix filenames.
    #[cfg(windows)]
    let locator = locator.replace('\\', "/");
    let digest = match location_scope {
        None => {
            options_hash(&json!({"shape":"cursor-location-v1", "family":family, "locator":locator}))
        }
        Some(scope) => options_hash(
            &json!({"shape":"cursor-location-v2", "family":family, "locator":locator, "locationId":scope}),
        ),
    };
    Ok(format!("{NAMESPACE}{family}-locator:{digest}"))
}

pub(crate) fn store_identity(path: &Path, root: Option<&Path>) -> Result<String, ImportError> {
    // Discovery has not proved a supported Store layout/native association.
    // Do not infer that association from arbitrary parent directory names.
    locator_identity("store", path, root)
}

pub(crate) fn store_identity_scoped(
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<String, ImportError> {
    match location_scope {
        None => store_identity(path, root),
        Some(scope) => locator_identity_scoped("store", path, root, Some(scope)),
    }
}

pub(super) fn subagent_identity(parent: &str, locator: &str) -> String {
    format!(
        "{NAMESPACE}subagent:{}",
        options_hash(&json!({"parent":parent, "childLocator":locator}))
    )
}

#[cfg(test)]
mod source_fidelity_tests {
    use super::*;

    #[test]
    fn source_fidelity_cursor_families_cannot_be_forged_by_opaque_ids() {
        let mut identities = std::collections::HashSet::new();
        for family in [
            NativeFamily::Session,
            NativeFamily::Composer,
            NativeFamily::LegacyChat,
        ] {
            for id in [
                "same",
                "composer:same",
                "cursor-identity-v1:session:same",
                "会话 🚀",
            ] {
                assert!(identities.insert(family.declared(id).unwrap()));
            }
        }
        assert_eq!(
            database_identity("chatData:same").unwrap(),
            "cursor-identity-v1:legacy-chat:same"
        );
        assert_eq!(
            database_identity("composerData:same").unwrap(),
            "cursor-identity-v1:composer:same"
        );
        for key in ["other:same", "chatData:", "composerData: \t"] {
            assert_eq!(
                database_identity(key).unwrap_err().code,
                ErrorCode::FormatUnsupported
            );
        }
    }
}
