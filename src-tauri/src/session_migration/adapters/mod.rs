//! Versioned source adapters. Each returns evidence + graph + inventory + issues.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod generic;
pub(super) mod titles;

use std::path::Path;

use super::errors::ImportError;
#[cfg(test)]
use super::types::SourceCandidate;
use super::types::{AdapterOutput, Provider};

#[cfg(test)]
pub fn normalize_path(provider: Provider, path: &Path) -> Result<AdapterOutput, ImportError> {
    normalize_in_root(provider, path, None)
}

pub fn normalize_in_root(
    provider: Provider,
    path: &Path,
    root: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    normalize_dispatch(provider, path, root, None)
}

pub(crate) fn normalize_in_location(
    provider: Provider,
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<AdapterOutput, ImportError> {
    match location_scope {
        None => normalize_in_root(provider, path, root),
        Some(scope) => normalize_dispatch(provider, path, root, Some(scope)),
    }
}

fn normalize_dispatch(
    provider: Provider,
    path: &Path,
    root: Option<&Path>,
    location_scope: Option<&str>,
) -> Result<AdapterOutput, ImportError> {
    match provider {
        Provider::Codex => codex::normalize_file_in_root(path, root),
        Provider::ClaudeCode => claude::normalize_file(path),
        Provider::Cursor if path.extension().and_then(|s| s.to_str()) == Some("jsonl") => {
            cursor::normalize_transcript_in_location(path, root, location_scope)
        }
        Provider::Cursor if path.file_name().and_then(|s| s.to_str()) == Some("store.db") => {
            cursor::normalize_store_in_location(path, root, location_scope)
        }
        Provider::Cursor => cursor::normalize_transcript_file(path),
        Provider::GenericFile => generic::normalize_file_in_root(path, root),
    }
}

#[derive(Debug, Default)]
#[cfg(test)]
pub struct DiscoverOutcome {
    pub candidates: Vec<SourceCandidate>,
    pub issues: Vec<ImportError>,
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use crate::session_migration::types::{ImportOutcome, ImportedEvent, MessageRole};

    fn fx(rel: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/session-migration")
            .join(rel)
    }

    #[test]
    fn codex_g1_counts() {
        let out = normalize_path(Provider::Codex, &fx("codex/g1-main.jsonl")).unwrap();
        let msgs = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::Message { .. }))
            .count();
        let calls = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::ToolCall { .. }))
            .count();
        let results = out
            .graph
            .events
            .iter()
            .filter(|e| matches!(e, ImportedEvent::ToolResult { .. }))
            .count();
        assert_eq!(msgs, 3);
        assert_eq!(calls, 1);
        assert_eq!(results, 1);
        assert_eq!(out.outcome, ImportOutcome::Complete);
    }

    #[test]
    fn claude_tool_result_not_user() {
        let out = normalize_path(Provider::ClaudeCode, &fx("claude/g1-tools.jsonl")).unwrap();
        let user = out
            .graph
            .events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    ImportedEvent::Message {
                        role: MessageRole::User,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(user, 1);
        assert!(out
            .graph
            .events
            .iter()
            .any(|e| matches!(e, ImportedEvent::ToolResult { .. })));
    }

    #[test]
    fn generic_fenced_and_illegal() {
        let ok = normalize_path(Provider::GenericFile, &fx("generic/fenced.md")).unwrap();
        assert_eq!(
            ok.graph
                .events
                .iter()
                .filter(|e| matches!(e, ImportedEvent::Message { .. }))
                .count(),
            2
        );
        let err = normalize_path(Provider::GenericFile, &fx("generic/illegal.json")).unwrap_err();
        assert_eq!(
            err.code,
            crate::session_migration::errors::ErrorCode::FormatUnsupported
        );
    }

    #[test]
    fn cursor_transcript_two_messages() {
        let out = normalize_path(Provider::Cursor, &fx("cursor/transcript.jsonl")).unwrap();
        assert_eq!(out.graph.events.len(), 2);
    }
}
