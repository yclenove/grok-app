//! Shared bounded session facts; message IDs and bare filenames are not identity.
use super::native_identity::{locator_identity_scoped, subagent_identity, NativeFamily};
use crate::session_migration::errors::{ErrorCode, ImportError};
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use std::path::Path;

const LOCATOR_NAMESPACE: &str = "cursor-transcript-locator:";

pub(crate) struct TranscriptIdentity {
    declared: Option<(NativeFamily, String, String)>,
    locator_id: String,
    subagent: bool,
}

impl TranscriptIdentity {
    #[cfg(test)]
    pub(crate) fn new(path: &Path, root: Option<&Path>) -> Result<Self, ImportError> {
        Self::new_scoped(path, root, None)
    }

    pub(crate) fn new_scoped(
        path: &Path,
        root: Option<&Path>,
        location_scope: Option<&str>,
    ) -> Result<Self, ImportError> {
        Ok(Self {
            declared: None,
            locator_id: locator_identity_scoped("transcript", path, root, location_scope)?,
            // Retain the child boundary even when the selected root itself is
            // the subagents directory, not only when it is inside that root.
            subagent: path.components().any(|c| {
                #[cfg(windows)]
                {
                    c.as_os_str()
                        .to_str()
                        .is_some_and(|s| s.eq_ignore_ascii_case("subagents"))
                }
                #[cfg(not(windows))]
                {
                    c.as_os_str() == "subagents"
                }
            }),
        })
    }

    pub(crate) fn observe(&mut self, record: &Value) -> Result<(), ImportError> {
        for field in ["sessionId", "composerId"] {
            let Some(value) = record.get(field).filter(|v| !v.is_null()) else {
                continue;
            };
            let id = value.as_str().ok_or_else(|| {
                ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "Cursor transcript declares a non-string session identity",
                )
            })?;
            if id.starts_with(LOCATOR_NAMESPACE) {
                return Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "Cursor transcript declares an invalid session identity",
                ));
            }
            let family = if field == "composerId" {
                NativeFamily::Composer
            } else {
                NativeFamily::Session
            };
            let native = family.declared(id)?;
            if let Some((_, previous, _)) = &self.declared {
                if previous != id {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        "Cursor transcript declares conflicting session identities",
                    ));
                }
            }
            // An explicit same-ID declaration of both families in this source
            // can establish the Composer association; bare sessionId cannot.
            if self.declared.is_none() || family == NativeFamily::Composer {
                self.declared = Some((family, id.into(), native));
            }
        }
        Ok(())
    }

    pub(crate) fn finish(&self) -> String {
        match &self.declared {
            Some((_, _, native)) if !self.subagent => native.clone(),
            Some((_, _, native)) => subagent_identity(native, &self.locator_id),
            None => self.locator_id.clone(),
        }
    }
}

#[cfg(test)]
mod source_fidelity_tests {
    use super::*;

    #[test]
    fn source_fidelity_subagent_root_selection_keeps_parent_scope_separate() {
        let root = Path::new("synthetic cursor");
        let child_root = root.join("subagents");
        let mut parent = TranscriptIdentity::new(&root.join("same.jsonl"), Some(root)).unwrap();
        let mut child =
            TranscriptIdentity::new(&child_root.join("same.jsonl"), Some(&child_root)).unwrap();
        for identity in [&mut parent, &mut child] {
            identity
                .observe(&json!({"composerId":"parent-fact","id":"message-only"}))
                .unwrap();
        }
        assert_eq!(parent.finish(), "cursor-identity-v1:composer:parent-fact");
        assert_ne!(child.finish(), parent.finish());
    }

    #[test]
    fn source_fidelity_declared_alias_conflict_and_outside_root_are_rejected() {
        let root = Path::new("synthetic cursor");
        assert!(TranscriptIdentity::new(Path::new("other/same.jsonl"), Some(root)).is_err());
        let mut identity = TranscriptIdentity::new(&root.join("same.jsonl"), Some(root)).unwrap();
        let error = identity
            .observe(&json!({"sessionId":"one","composerId":"two"}))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::FormatUnsupported);
    }

    #[cfg(unix)]
    #[test]
    fn source_fidelity_unix_backslash_filename_is_not_a_directory_separator() {
        let root = Path::new("synthetic cursor");
        let literal = TranscriptIdentity::new(&root.join("a\\same.jsonl"), Some(root)).unwrap();
        let nested = TranscriptIdentity::new(&root.join("a/same.jsonl"), Some(root)).unwrap();
        assert_ne!(literal.finish(), nested.finish());
    }
}
