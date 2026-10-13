//! Real Win32 group witnesses on the private noninteractive station.
//! Host choices are synthetic. This station cannot admit visible-window
//! metadata/capture/input, so these are rejection and retirement checks.
use super::*;
use crate::computer_use::adapter::{ComputerUseAdapter, SurfaceKind};
use crate::computer_use::windows_identity::{clear_stamp, format_target_id, stamp_window};
use grok_computer_use_core::broker::{
    AccessApprovalReady, BrokerOptions, ComputerUseBroker, StopState,
};
use serde_json::json;
use std::sync::Arc;

struct OwnedBroker(Arc<ComputerUseBroker>);
impl OwnedBroker {
    fn finish(&self) -> Result<(), String> {
        self.0.fence_task_consent("native-group-probe");
        for ticket in self.0.take_cleanup_tickets() {
            if self
                .0
                .finish_stop_cleanup(&ticket)
                .map_err(|e| e.to_string())?
                != StopState::Stopped
            {
                return Err("original native scope cleanup is not terminal".into());
            }
        }
        Ok(())
    }
}
impl Drop for OwnedBroker {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            eprintln!("original native group cleanup failed: {error}");
        }
    }
}

fn id(edit: &Edit) -> String {
    format_target_id(std::process::id(), edit.hwnd, stamp_window(edit.hwnd))
}

fn group(retire_inactive: bool) -> Result<(), String> {
    // WS_VISIBLE does not make a private window station interactive. Keep
    // production IsWindowVisible/OpenInputDesktop checks; never use WinSta0.
    let a = Edit::with_visibility("Owned native scope A", None, true)?;
    let b = Edit::with_visibility("Owned native scope B", None, true)?;
    let outside = Edit::with_visibility("Unselected native scope C", None, true)?;
    let (a_id, b_id) = (id(&a), id(&b));
    for edit in [&a, &b, &outside] {
        edit.state.reads.set(0);
        edit.state.effects.set(0);
    }
    let adapter = Arc::new(super::super::WindowsAdapter::new());
    if adapter.input_available() || unsafe { IsWindowVisible(a.hwnd) }.as_bool() {
        return Err("this rejection probe requires the private noninteractive station".into());
    }
    if !retire_inactive
        && adapter
            .scope_target_info("native-private-metadata", &a_id)
            .as_ref()
            .err()
            .map(String::as_str)
            != Some("selected Windows window is hidden or minimized")
    {
        return Err("hidden selected native metadata did not report its actual limitation".into());
    }
    // Use the same Host ownership wrapper as the App, retaining the native
    // producer rather than validating only an unwrapped fixture adapter.
    let wrapped = Arc::new(grok_computer_use_core::owned::HostOwnedAdapter::new(
        adapter,
    ));
    if wrapped.worker_attached() {
        return Err("scope descriptor unexpectedly started a driver".into());
    }
    let owned = OwnedBroker(Arc::new(ComputerUseBroker::new(
        wrapped,
        BrokerOptions {
            feature_enabled: false,
            lease_path: std::env::temp_dir()
                .join(format!("cu-native-group-{}.lease", uuid::Uuid::new_v4())),
            ..BrokerOptions::default()
        },
    )));
    let broker = &owned.0;
    let token = broker
        .issue_transport_credential("native-group-probe")
        .map_err(|e| e.to_string())?;
    broker
        .bind_agent_session("native-group-probe", "synthetic-native-group-agent")
        .map_err(|e| e.to_string())?;
    let pending = broker.submit_access(token.token(), json!({"action":"request",
        "purpose":"synthetic Host chooses two isolated Win32 roots", "scopeIntent":"owned A and B only",
        "surfaces":["desktop"], "capabilities":["observe","input"]})).map_err(|e| e.to_string())?;
    let request = pending["requestId"]
        .as_str()
        .ok_or("native group request missing")?;
    let selected = broker
        .capture_task_scope_group(
            token.token(),
            request,
            1,
            &[(SurfaceKind::Desktop, &a_id), (SurfaceKind::Desktop, &b_id)],
        )
        .map_err(|e| e.to_string())?;
    if selected.members().len() != 2
        || broker.feature_enabled()
        || a.state.reads.get() + b.state.reads.get() + outside.state.reads.get() != 0
    {
        return Err("native selection read content or enabled execution".into());
    }
    if retire_inactive {
        clear_stamp(b.hwnd);
        stamp_window(b.hwnd);
        if broker
            .reserve_selected_access_approval(token.token(), selected)
            .is_ok()
        {
            return Err("retired inactive Win32 root published a group reservation".into());
        }
    } else {
        let AccessApprovalReady::Prepare(mut prep) = broker
            .reserve_selected_access_approval(token.token(), selected)
            .map_err(|e| e.to_string())?
        else {
            return Err("native group unexpectedly already approved".into());
        };
        prep.publish_feature().map_err(|e| e.to_string())?;
        let error = prep
            .prepare_execution()
            .err()
            .ok_or("private hidden group prepared an execution binding")?;
        if !error.to_string().contains("hidden or minimized") {
            return Err("private native preparation failed for an unrelated reason".into());
        }
        prep.abort();
    }
    if broker.feature_enabled()
        || broker.executable_grant_count("native-group-probe") != 0
        || broker.continuation_count("native-group-probe") != 0
        || !broker.scope_memories("native-group-probe").is_empty()
        || a.state.reads.get() + b.state.reads.get() + outside.state.reads.get() != 0
        || a.state.effects.get() + b.state.effects.get() + outside.state.effects.get() != 0
    {
        return Err("rejected native group leaked authority, ambient reads or input".into());
    }
    a.assert_value("Owned native scope A")?;
    b.assert_value("Owned native scope B")?;
    outside.assert_value("Unselected native scope C")?;
    owned.finish()
}

type GateRunner<'a> = dyn FnMut(&str, &dyn Fn() -> Result<(), String>) + 'a;

pub(super) fn run(run: &mut GateRunner<'_>) {
    run(
        "private hidden native group rejects preparation without ambient reads or input",
        &|| group(false),
    );
    run(
        "retired inactive native root rejects the whole group reservation",
        &|| group(true),
    );
}
