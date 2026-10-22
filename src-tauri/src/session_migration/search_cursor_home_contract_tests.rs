//! Every signed search/read cursor fixture must own its installation secret.
//! The inherited sandbox sentinel must remain byte-identical and be the only
//! parent-home file after each fixture, including when the fixture creates pages.
use super::*;
use crate::session_migration::test_home::fixture_home_contract_tests;

fixture_home_contract_tests!(
    cjk_query_hits_without_spaces,
    model_search_and_read_exclude_reasoning_and_system_fields,
    selection_search_and_read_hide_denied_reasoning_and_tool_input,
    literal_search_preserves_phrase_path_and_cursor_scope,
    search_and_read_cursors_reject_bare_tamper_cross_owner_and_stale,
    bounded_read_reassembles_unicode_and_rejects_foreign_cursor,
    read_indexed_event_text_cursor_offset_is_bytes_not_chars,
    missing_index_rebuilds_from_archive,
    corrupt_existing_index_fails_closed,
    corrupt_archive_is_not_an_empty_page,
    corrupt_event_json_is_not_skipped,
    old_index_schema_rebuilds,
    page_cursor_rejects_another_revision,
    event_page_stops_at_100_and_does_not_repeat,
    event_page_byte_cap_keeps_unicode_and_oversized_first,
    read_with_selection_reassembles_unicode_by_byte_offset,
);
