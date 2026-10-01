//! Real Win32 listbox readback for both UIA Scroll and directed WM_MOUSEWHEEL.
//! This is an owned test window, never a user's application or full release proof.
use super::{force_foreground, FixtureWindow};
use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::windows_adapter::WindowsAdapter;
use std::cmp::Ordering;
use std::time::Duration;

pub fn run_scroll_direction() -> Result<(), String> {
    for semantic in [true, false] {
        let title = format!("GrokCuFixture-scroll-{}-{semantic}", std::process::id());
        let fixture = FixtureWindow::spawn(&title)?;
        let adapter = WindowsAdapter::new();
        let target = adapter
            .list_targets()?
            .into_iter()
            .find(|target| {
                target.title == title
                    && crate::computer_use::windows_identity::parse_target_id(&target.target_id)
                        .is_some_and(|(pid, hwnd, _)| {
                            pid == std::process::id() && hwnd == fixture.hwnd()
                        })
            })
            .ok_or("exact owned scroll fixture missing")?;
        force_foreground(fixture.hwnd());
        if !adapter.foreground_input_available(&target.target_id) {
            return Err(
                "owned scroll fixture did not acquire foreground; no input attempted".into(),
            );
        }
        for (delta, expected) in [
            (240, Ordering::Greater),
            (0, Ordering::Equal),
            (-240, Ordering::Less),
        ] {
            let obs = adapter.observe(&target.target_id)?;
            let lists: Vec<_> = obs
                .nodes
                .iter()
                .filter(|n| n.role == "list" && !n.truncated)
                .collect();
            if lists.len() != 1 {
                return Err("owned fixture must expose one unambiguous list".into());
            }
            let list = lists[0];
            let action_target = if semantic {
                ActionTarget::Element {
                    element_ref: list.node_ref.clone(),
                }
            } else {
                let (Some(x), Some(y), Some(w), Some(h)) =
                    (list.x, list.y, list.width, list.height)
                else {
                    return Err("list geometry unavailable".into());
                };
                ActionTarget::Coord {
                    x: x + w / 2.0,
                    y: y + h / 2.0,
                }
            };
            let before = fixture.list_top_index();
            if before < 0 {
                return Err("list oracle unavailable".into());
            }
            let result = adapter.act(&DispatchRequest {
                managed_request: None,
                cancellation: Default::default(),
                run_id: "scroll-fixture".into(),
                action_id: format!("scroll-{semantic}-{delta}"),
                generation: 1,
                target_id: target.target_id.clone(),
                target_generation: 1,
                snapshot_id: obs.snapshot_id,
                geometry_revision: obs.geometry_revision,
                action: ActionKind::Scroll,
                target: action_target,
                parameters: serde_json::json!({"delta":delta}),
                scope: ActionScope::Directed,
            })?;
            std::thread::sleep(Duration::from_millis(100));
            let after = fixture.list_top_index();
            if after < 0 || after.cmp(&before) != expected || result.applied != (delta != 0) {
                return Err(format!("scroll direction/no-op failed semantic={semantic} delta={delta} top={before}->{after} applied={}",result.applied));
            }
            if semantic && delta != 0 && !result.detail.contains("uia scroll") {
                return Err("semantic scroll took an unintended fallback".into());
            }
            println!("PASS native Windows scroll semantic={semantic} delta={delta} top={before}->{after} applied={}",result.applied);
        }
        fixture.close();
    }
    println!("gate: windows_scroll_direction positive_down negative_up zero_noop UIA_and_WM_MOUSEWHEEL=true");
    Ok(())
}
