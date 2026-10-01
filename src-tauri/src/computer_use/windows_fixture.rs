//! Self-built Win32 fixture window for native observe/act/verify tests.

#![cfg(target_os = "windows")]

mod choice_loop;
mod clipboard;
mod focus;
mod identity;
mod native_stop;
mod p8;
mod scroll;
mod security;
mod uia;
mod window;

pub use choice_loop::{
    run_choice_captured_set_value, run_choice_click_text, run_choice_stop_and_dead,
};
pub use clipboard::run_clipboard_restore;
pub use focus::{run_focus_drift_pauses, run_user_takeover_pauses};
pub use identity::run_identity_host_protect;
pub use native_stop::run_native_stop;
pub use p8::{hold_p8_ipc, run_p8_native_loop};
pub use scroll::run_scroll_direction;
pub use security::run_security_and_cancel;
pub use uia::{run_geometry_capture, run_uia_element_tree, run_uia_full_actions};
pub use window::FixtureWindow;
pub(crate) use window::{force_foreground, oracle_path, write_list_oracle};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::computer_use::adapter::ComputerUseAdapter;

    #[test]
    #[ignore = "native Windows desktop; run explicitly with --ignored"]
    fn fixture_window_is_enumerable() {
        let title = format!("GrokCuFixture-{}", std::process::id());
        let fx = FixtureWindow::spawn(&title).expect("native fixture must actually start");
        let adapter = crate::computer_use::windows_adapter::WindowsAdapter::new();
        let listed = adapter.list_targets().expect("native target enumeration");
        let target = listed
            .iter()
            .find(|t| {
                t.title == title
                    && crate::computer_use::windows_identity::parse_target_id(&t.target_id)
                        .is_some_and(|(pid, hwnd, _)| {
                            pid == std::process::id() && hwnd == fx.hwnd()
                        })
            })
            .expect("the exact owned fixture window must be enumerated");
        assert!(adapter.target_alive(&target.target_id));
        fx.close();
        assert!(
            !adapter.target_alive(&target.target_id),
            "closed fixture must retire its identity"
        );
    }
}
