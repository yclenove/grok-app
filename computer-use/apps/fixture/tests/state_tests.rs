//! Unit and integration tests for `gcu-fixture` state logic, persistence, and pure UI helpers.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gcu_fixture::state::{
    compute_grid_cell, compute_virtual_list_rows, hit_test_rect, write_state_atomically,
    EventSource, FixtureState, CANVAS_GRID_COLS, CANVAS_GRID_ROWS, MAX_EVENT_LOG_ENTRIES,
    VIRTUAL_LIST_TOTAL_ROWS,
};
use std::fs;
use std::sync::Arc;
use std::thread;

#[test]
fn atomic_write_creates_valid_state_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir creation");
    let state_path = temp_dir.path().join("fixture_state.json");

    let mut state = FixtureState::default();
    state.set_text("Alice".to_string(), EventSource::Keyboard);
    state.set_checkbox(true, EventSource::Mouse);
    state.increment_counter(EventSource::Accessibility);

    let res = write_state_atomically(&state, &state_path);
    assert!(res.is_ok(), "atomic write should succeed: {:?}", res.err());

    assert!(state_path.exists(), "target state file must exist");

    // Ensure no temporary files are left over in the directory
    let entries: Vec<_> = fs::read_dir(temp_dir.path())
        .expect("read_dir")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1, "only the target state file should remain");

    // Read back and deserialize
    let raw_content = fs::read_to_string(&state_path).expect("read state file");
    let loaded_state: FixtureState = serde_json::from_str(&raw_content).expect("deserialize state");

    assert_eq!(loaded_state.text, "Alice");
    assert!(loaded_state.checkbox);
    assert!(loaded_state.checked);
    assert_eq!(loaded_state.counter, 1);
    assert_eq!(loaded_state.form.name, "Alice");
    assert!(loaded_state.form.agree);
}

#[test]
fn concurrent_writes_produce_valid_final_state() {
    let temp_dir = tempfile::tempdir().expect("tempdir creation");
    let state_path = Arc::new(temp_dir.path().join("concurrent_state.json"));

    let num_threads = 8;
    let iterations_per_thread = 15;
    let mut handles = Vec::new();

    for thread_idx in 0..num_threads {
        let path_clone = Arc::clone(&state_path);
        handles.push(thread::spawn(move || {
            for iter in 0..iterations_per_thread {
                let mut state = FixtureState {
                    counter: (thread_idx * 100 + iter) as u64,
                    ..Default::default()
                };
                state.set_text(format!("Thread-{thread_idx}-{iter}"), EventSource::Keyboard);
                let write_res = write_state_atomically(&state, &path_clone);
                assert!(write_res.is_ok(), "concurrent write must succeed");
            }
        }));
    }

    for handle in handles {
        handle.join().expect("thread join");
    }

    // Final file must be valid, uncorrupted JSON
    let raw_content = fs::read_to_string(&*state_path).expect("read final state");
    let final_state: FixtureState =
        serde_json::from_str(&raw_content).expect("final state must be valid JSON");
    assert!(!final_state.text.is_empty());

    // Verify temp files are cleaned up
    let temp_files: Vec<_> = fs::read_dir(temp_dir.path())
        .expect("read_dir")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".concurrent_state.tmp.")
        })
        .collect();
    assert!(
        temp_files.is_empty(),
        "no temporary files should be orphaned"
    );
}

#[test]
fn counter_increments_exactly_once() {
    let mut state = FixtureState::default();
    assert_eq!(state.counter, 0);

    state.increment_counter(EventSource::Mouse);
    assert_eq!(state.counter, 1);

    state.increment_counter(EventSource::Accessibility);
    assert_eq!(state.counter, 2);

    assert_eq!(state.event_log.len(), 2);
    assert_eq!(state.event_log[0].source, EventSource::Mouse);
    assert_eq!(state.event_log[0].action, "increment_counter");
    assert_eq!(state.event_log[1].source, EventSource::Accessibility);
    assert_eq!(state.event_log[1].action, "increment_counter");
}

#[test]
fn password_not_written_to_state_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let state_path = temp_dir.path().join("secure_state.json");

    let secret_plain = "SuperSecret_Sensitive_Password_987#";
    let mut state = FixtureState::default();
    state.set_password(secret_plain, EventSource::Keyboard);

    assert_eq!(state.password_secret, secret_plain);
    assert_eq!(state.password_len, secret_plain.chars().count());
    assert!(state.is_password);

    write_state_atomically(&state, &state_path).expect("atomic write");

    let file_text = fs::read_to_string(&state_path).expect("read state file");

    // The plaintext MUST NOT appear anywhere in the saved file
    assert!(
        !file_text.contains(secret_plain),
        "PLAINTEXT PASSWORD LEAKED in state file: {file_text}"
    );

    // Required metadata must be present
    assert!(file_text.contains("\"is_password\": true"));
    assert!(file_text.contains(&format!("\"password_len\": {}", secret_plain.len())));

    // Deserializing does not restore plaintext
    let restored: FixtureState = serde_json::from_str(&file_text).expect("deserialization");
    assert!(restored.is_password);
    assert_eq!(restored.password_len, secret_plain.len());
    assert!(restored.password_secret.is_empty());
}

#[test]
fn event_log_bounded_at_fifty() {
    let mut state = FixtureState::default();

    for i in 0..75 {
        state.log_event(EventSource::Keyboard, format!("action_{i}"), None);
    }

    assert_eq!(state.event_log.len(), MAX_EVENT_LOG_ENTRIES);
    assert_eq!(state.event_log[0].action, "action_25");
    assert_eq!(state.event_log[49].action, "action_74");
}

#[test]
fn invalid_path_returns_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let dir_path = temp_dir.path();

    let state = FixtureState::default();
    let res = write_state_atomically(&state, dir_path);
    assert!(res.is_err(), "writing to an existing directory must fail");
}

#[test]
fn sync_mirrors_and_form_state_consistency() {
    let mut state = FixtureState::default();
    state.set_text("Zhang San".to_string(), EventSource::Keyboard);
    state.set_checkbox(true, EventSource::Mouse);
    state.set_slider(75.5, EventSource::Mouse);
    state.set_combo_selected(2, EventSource::Mouse);
    state.set_drag_drop_ok(true, EventSource::Mouse);
    state.submit_form(EventSource::Mouse);
    state.set_list_visible_range((10, 25));

    // Check mirrors
    assert!(state.checked);
    assert_eq!(state.selected, 2);
    assert_eq!(state.value, 75.5);
    assert!(state.drop_ok);
    assert_eq!(state.visible_range, (10, 25));

    // Check form oracle sub-object
    assert_eq!(state.form.name, "Zhang San");
    assert!(state.form.agree);
    assert_eq!(state.form.submitted_count, 1);
}

#[test]
fn modal_open_and_close_lifecycle() {
    let mut state = FixtureState::default();
    assert!(!state.modal_open);
    assert!(!state.modal_closed);
    assert_eq!(state.modal_closed_count, 0);

    // Open modal
    state.open_modal(EventSource::Mouse);
    assert!(state.modal_open);
    assert_eq!(state.modal_open_count, 1);
    assert!(!state.modal_closed);

    // Cancel modal
    state.close_modal(false, EventSource::Mouse);
    assert!(!state.modal_open);
    assert!(!state.modal_closed);
    assert_eq!(state.modal_closed_count, 0);

    // Reopen and confirm
    state.open_modal(EventSource::Mouse);
    assert!(state.modal_open);
    assert_eq!(state.modal_open_count, 2);

    state.close_modal(true, EventSource::Mouse);
    assert!(!state.modal_open);
    assert!(state.modal_closed);
    assert_eq!(state.modal_closed_count, 1);
}

#[test]
fn sensitive_send_button_increments_count() {
    let mut state = FixtureState::default();
    assert_eq!(state.send_clicked, 0);

    state.click_send(EventSource::Mouse);
    assert_eq!(state.send_clicked, 1);

    state.click_send(EventSource::Keyboard);
    assert_eq!(state.send_clicked, 2);

    assert_eq!(state.event_log.len(), 2);
    assert_eq!(state.event_log[0].action, "click_send");
    assert_eq!(state.event_log[1].action, "click_send");
}

#[test]
fn compute_grid_cell_inside_and_outside_bounds() {
    let origin_x = 100.0;
    let origin_y = 50.0;
    let cell_w = 40.0;
    let cell_h = 40.0;

    // Inside cells
    assert_eq!(
        compute_grid_cell(
            (105.0, 55.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        Some((0, 0))
    );
    assert_eq!(
        compute_grid_cell(
            (415.0, 285.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        Some((7, 5))
    );
    assert_eq!(
        compute_grid_cell(
            (225.0, 135.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        Some((3, 2))
    );

    // Outside bounds
    assert_eq!(
        compute_grid_cell(
            (99.0, 55.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        None
    );
    assert_eq!(
        compute_grid_cell(
            (105.0, 49.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        None
    );
    assert_eq!(
        compute_grid_cell(
            (500.0, 55.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        None
    );
    assert_eq!(
        compute_grid_cell(
            (105.0, 400.0),
            (origin_x, origin_y),
            (cell_w, cell_h),
            (CANVAS_GRID_COLS, CANVAS_GRID_ROWS)
        ),
        None
    );
}

#[test]
fn compute_virtual_list_rows_page_scrolling() {
    let viewport_h = 180.0;
    let row_h = 20.0;
    let total = VIRTUAL_LIST_TOTAL_ROWS;

    let range_initial = compute_virtual_list_rows(0.0, viewport_h, row_h, total);
    assert_eq!(range_initial, (0, 9));

    let range_scrolled = compute_virtual_list_rows(100.0, viewport_h, row_h, total);
    assert_eq!(range_scrolled, (5, 14));

    let range_end = compute_virtual_list_rows(50000.0, viewport_h, row_h, total);
    assert_eq!(range_end, (2000, 2000));
}

#[test]
fn hit_test_rect_boundary_conditions() {
    let min = (10.0, 20.0);
    let max = (100.0, 80.0);

    assert!(hit_test_rect((50.0, 50.0), min, max));
    assert!(hit_test_rect((10.0, 20.0), min, max));
    assert!(hit_test_rect((100.0, 80.0), min, max));

    assert!(!hit_test_rect((9.9, 50.0), min, max));
    assert!(!hit_test_rect((100.1, 50.0), min, max));
    assert!(!hit_test_rect((50.0, 19.9), min, max));
    assert!(!hit_test_rect((50.0, 80.1), min, max));
}
