//! Portable regression tests for the native probe's window selection.

#[path = "../examples/driver_probe/select.rs"]
mod select;

use gcu_protocol::{Rect, WindowInfo};

fn window(id: &str, width: f64) -> WindowInfo {
    WindowInfo {
        id: id.into(),
        title: "Fixture".into(),
        frame: Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: 100.0,
        },
        scale: 2.0,
        on_current_desktop: None,
    }
}

#[test]
fn window_selection_no_key_window_uses_largest_listed_fixture_window() {
    let windows = [window("cg:8:42", 100.0), window("cg:7:42", 200.0)];
    assert_eq!(
        select::window(&windows, None, 42).map(|w| w.id),
        Some("cg:7:42".into())
    );
}

#[test]
fn window_selection_valid_key_window_preserves_main_window_choice() {
    let windows = [window("cg:8:42", 100.0), window("cg:7:42", 200.0)];
    assert_eq!(
        select::window(&windows, Some(&windows[0]), 42).map(|w| w.id),
        Some("cg:8:42".into())
    );
}

#[test]
fn window_selection_other_process_or_stale_key_is_never_substituted() {
    let wrong_pid = window("cg:8:99", 500.0);
    let stale = window("cg:9:42", 300.0);
    let listed = window("cg:7:42", 200.0);
    assert!(select::window(&[wrong_pid], Some(&stale), 42).is_none());
    assert_eq!(
        select::window(&[listed], Some(&stale), 42).map(|w| w.id),
        Some("cg:7:42".into())
    );
}

#[test]
fn window_selection_malformed_or_zero_cg_id_is_rejected() {
    for id in ["cg:0:42", "cg:bad:42", "cg:7:42:99", "ax:7:42"] {
        assert!(select::window(&[window(id, 200.0)], None, 42).is_none());
    }
}
