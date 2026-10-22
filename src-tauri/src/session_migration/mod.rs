//! Session and workspace import domain.
//!
//! Imported history lives in `{app_data}/imports/` and is never written as a
//! native `messages.json` / `sessions_index.json` row. Native session commands
//! reject `imp:<uuid>` with `IMPORTED_HISTORY_READ_ONLY`.

pub mod adapters;
pub mod assets;
pub mod budget;
pub mod capability;
pub mod catalog;
mod catalog_indexes;
mod clock;
#[cfg(test)]
mod codec_profile;
pub mod composer_discovery;
pub mod continuation;
#[cfg(test)]
mod continuation_attempt_tests;
#[cfg(test)]
mod continuation_claim_tests;
mod continuation_create;
#[cfg(test)]
mod continuation_integrity_tests;
mod continuation_intent;
pub mod source_comparison;
pub mod source_comparison_fields;
pub(crate) use continuation_intent::record_published_before_removal;
mod continuation_lifecycle;
mod continuation_scope;
mod continuation_state;
pub(crate) mod continuation_target;
pub mod continuation_view;
pub mod cursor;
pub mod detect;
pub mod discovery;
pub mod engine;
pub mod errors;
pub mod graph;
pub mod group_selection;
pub mod hash;
pub mod identity_association;
pub mod ids;
mod image_catalog;
pub mod image_reader;
pub mod job_actions;
pub mod job_control;
#[cfg(test)]
mod job_control_tests;
pub mod job_history;
mod job_input_association;
#[cfg(test)]
mod job_poll_tests;
mod job_progress;
mod job_title_projection;
pub mod jobs;
#[cfg(test)]
mod jobs_projection_measure_tests;
pub mod jsonl;
pub(crate) mod mapping_scope;
pub mod mcp_stdio;
#[cfg(all(debug_assertions, feature = "native-import-audit"))]
mod native_fault;
mod plan_snapshot;
pub mod prepare;
mod prepare_mapping;
mod prepare_plan;
mod prepare_query;
mod prepare_selection;
#[cfg(test)]
mod prepare_tests;
pub mod prepared_association;
pub(crate) mod prepared_builder;
mod preview_display;
pub(crate) mod preview_gate;
mod preview_lease_io;
pub(crate) mod preview_lifecycle;
#[cfg(test)]
mod preview_lifecycle_tests;
pub(crate) mod preview_store;
pub mod project_link;
mod project_references;
mod publication_crypto;
mod publication_outcome;
pub(crate) use publication_outcome::ProjectCreation as ImportProjectCreation;
mod publication_protocol;
mod publication_write_fence;
#[cfg(test)]
mod publish_timing;
mod published_verification;
pub mod reader;
pub mod reader_health;
mod registration_provenance;
mod registration_provenance_store;
pub mod revision_history;
pub mod scan;
mod scan_group_store;
pub mod scan_groups;
pub mod scan_selection;
pub mod search;
pub mod selection;
#[cfg(test)]
pub mod source_copy_guard;
mod source_location_store;
pub mod source_locations;
pub mod source_member;
pub mod sources;
#[cfg(all(test, windows))]
mod sqlite_long_path_tests;
mod sqlite_open;
/// Test fixtures outside the import domain use the same long-path VFS and locks.
#[cfg(test)]
pub(crate) fn open_fixture_sqlite(
    path: impl AsRef<std::path::Path>,
) -> rusqlite::Result<rusqlite::Connection> {
    sqlite_open::open(path)
}
pub mod sqlite_ro;
#[cfg(test)]
pub mod test_home;
pub mod types;
pub mod workspace;
pub mod workspace_apply;
pub mod workspace_association;
mod workspace_association_store;
pub mod workspace_catalog;
pub mod workspace_edit;
mod workspace_identity;
pub mod workspace_import;
pub mod workspace_relocation;

pub use ids::reject_imported_history_id;

/// CLI flag handled before Tauri / single-instance.
pub const IMPORT_HISTORY_MCP_FLAG: &str = "--import-history-mcp";

/// True when this process should become the import-history MCP stdio server.
pub fn wants_import_history_mcp() -> bool {
    std::env::args().any(|a| a == IMPORT_HISTORY_MCP_FLAG)
}

/// Run the stdio MCP server and return a process exit code.
pub fn run_import_history_mcp() -> i32 {
    mcp_stdio::run_stdio()
}
