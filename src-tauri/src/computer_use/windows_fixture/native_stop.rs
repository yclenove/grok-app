//! Owned HWND acceptance: pause inside the target's real synchronous down
//! handler, stop from another thread, and independently count native effects.
use super::{force_foreground, FixtureWindow};
use crate::computer_use::adapter::{ActionScope, ComputerUseAdapter, DispatchRequest};
use crate::computer_use::protocol::{ActionKind, ActionTarget};
use crate::computer_use::windows_adapter::WindowsAdapter;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct DispatchPause {
    state: Mutex<(bool, bool, bool)>, // entered, released, expired
    changed: Condvar,
}

impl DispatchPause {
    pub(super) fn enter(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        let (mut state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |state| !state.1)
            .unwrap();
        state.2 = timeout.timed_out() && !state.1;
    }

    fn await_entered(&self) -> bool {
        let (state, _) = self
            .changed
            .wait_timeout_while(
                self.state.lock().unwrap(),
                Duration::from_secs(3),
                |state| !state.0,
            )
            .unwrap();
        state.0
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
}

fn round(
    adapter: &Arc<WindowsAdapter>,
    fx: &FixtureWindow,
    req: DispatchRequest,
    stop: bool,
) -> Result<(), String> {
    let pause = fx.pause_next_drag();
    let executing = adapter.clone();
    let dispatched = req.clone();
    let worker = std::thread::spawn(move || executing.act(&dispatched));
    let checked: Result<(), String> = (|| {
        if !pause.await_entered() {
            return Err("native down handler was not entered".into());
        }
        if adapter.is_idle(&req.run_id) || !fx.is_dragging() {
            return Err("native work incorrectly reported idle".into());
        }
        adapter.abort("other-run", u64::MAX)?;
        adapter.abort(&req.run_id, req.generation)?;
        req.cancellation
            .check()
            .map_err(|_| "late/unrelated Stop canceled the current action")?;
        if stop {
            let started = Instant::now();
            adapter.abort(&req.run_id, req.generation + 1)?;
            if started.elapsed() > Duration::from_millis(500) {
                return Err("Stop waited for the blocked native handler".into());
            }
            if req.cancellation.check().is_ok() || adapter.is_idle(&req.run_id) {
                return Err("Stop failed to revoke input or released occupancy early".into());
            }
        }
        let mut next = req.clone();
        next.generation += 2;
        next.cancellation = Default::default();
        if !adapter.act(&next).is_err_and(|e| e.contains("occupied")) {
            return Err("a second native action entered before the first returned".into());
        }
        Ok(())
    })();
    // Cleanup runs even when an assertion fails. The target's own handler also
    // has a bounded fail-safe, which must never be counted as a passed check.
    pause.release();
    let native = worker
        .join()
        .map_err(|_| "native dispatch thread panicked")?;
    checked?;
    if pause.state.lock().unwrap().2 {
        return Err("native test hold expired before explicit release".into());
    }
    if native.is_ok() == stop {
        return Err(format!("unexpected native outcome: {native:?}"));
    }
    if !adapter.is_idle(&req.run_id) || fx.is_dragging() {
        return Err("native return did not finish the owned down/up cleanup".into());
    }
    Ok(())
}

pub fn run_native_stop() -> Result<(), String> {
    let title = format!("GrokCuFixture-native-stop-{}", uuid::Uuid::new_v4());
    let fx = FixtureWindow::spawn(&title)?;
    let adapter = Arc::new(WindowsAdapter::new());
    let target = adapter
        .list_targets()?
        .into_iter()
        .find(|t| t.title == title)
        .ok_or("owned native Stop fixture was not enumerated")?;
    force_foreground(fx.hwnd());
    let deadline = Instant::now() + Duration::from_secs(2);
    while !adapter.foreground_input_available(&target.target_id) {
        if Instant::now() >= deadline {
            return Err("owned fixture did not get foreground".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let req = DispatchRequest {
        managed_request: None,
        cancellation: Default::default(),
        run_id: "native-stop".into(),
        action_id: "old-drag".into(),
        generation: 1,
        target_id: target.target_id.clone(),
        target_generation: 1,
        snapshot_id: "owned-stop-fixture".into(),
        geometry_revision: adapter.current_geometry_revision_for(&target.target_id),
        action: ActionKind::Drag,
        target: ActionTarget::Coord { x: 260.0, y: 165.0 },
        parameters: serde_json::json!({"x1": 395.0, "y1": 165.0}),
        scope: ActionScope::Directed,
    };
    round(&adapter, &fx, req.clone(), true)?;
    if fx.drags() != 0 {
        return Err("canceled native drag reached its destination".into());
    }
    println!("PASS native blocked down: Stop stays nonblocking, retains occupancy and releases only its directed down");
    println!(
        "PASS canceled continuation: no native drop; another dispatch rejected while occupied"
    );
    let mut resumed = req;
    resumed.action_id = "new-drag".into();
    resumed.generation = 3;
    resumed.cancellation = Default::default();
    round(&adapter, &fx, resumed, false)?;
    if fx.drags() != 1 {
        return Err(format!(
            "resumed native drag count {}, expected 1",
            fx.drags()
        ));
    }
    println!("PASS late/unrelated Stop cannot cancel the new generation; resumed native drop exactly once");
    println!(
        "Windows native Stop acceptance: PASS (owned HWND, not installed App or all desktop input)"
    );
    Ok(())
}
