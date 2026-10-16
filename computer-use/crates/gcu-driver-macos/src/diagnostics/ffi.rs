//! Native probe boundary. Events are constructed/read back, never posted here.

use crate::{
    ax::{AccessibilityToggles, ToggleName, ToggleStore},
    MacosDriver,
};
use gcu_core::driver::{Driver, DriverError, Result};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventSource, CGEventSourceStateID, CGEventType, CGMouseButton,
};
use serde_json::{json, Value};
use std::{ffi::CStr, sync::Arc};

/// Inspect the installed backend and non-prompting native prerequisites.
/// CG/preflight checks run on a blocking worker; no input is posted. Field-51
/// readback tests event construction, not delivery to the target application.
pub async fn runtime_report(driver: &MacosDriver) -> Result<Value> {
    let probe = driver.event_api().map(|api| api.probe());
    let caps = driver.capabilities().clone();
    let guards = driver.input_guards().map(|g| {
        json!({
            "stopped": g.is_stopped(), "stop_available": g.stop_available(),
            "ms_since_physical_input": g.ms_since_physical_input(),
            "foreground_ready": g.foreground_ready(),
        })
    });
    let native = tokio::task::spawn_blocking(|| {
        let source = CGEventSource::new(CGEventSourceStateID::Private);
        let event = CGEvent::new(None); // Same implicit source as production.
        let mouse = CGEvent::new_mouse_event(
            None,
            CGEventType::LeftMouseDown,
            CGPoint { x: 0.0, y: 0.0 },
            CGMouseButton::Left,
        );
        let key = CGEvent::new_keyboard_event(None, 0, true);
        let readback = event.as_deref().map(field_51_readback);
        json!({
            "ax_trusted": super::ax_call("AXIsProcessTrusted", 0, "prompt=false", || {
                // SAFETY: non-prompting trust preflight has no pointer arguments.
                unsafe { accessibility_sys::AXIsProcessTrusted() }
            }),
            "screen_recording": objc2_core_graphics::CGPreflightScreenCaptureAccess(),
            "sck_screenshot_manager_class": symbol(c"OBJC_CLASS_$_SCScreenshotManager"),
            "cg_window_location_symbol": symbol(c"CGEventSetWindowLocation"),
            "cg_post_to_pid_symbol": symbol(c"CGEventPostToPid"),
            "event_source_private_created": source.is_some(),
            "event_implicit_source_created": event.is_some(),
            "mouse_event_created": mouse.is_some(),
            "mouse_field_51_readback": mouse.as_deref().map(field_51_readback),
            "keyboard_event_created": key.is_some(),
            "keyboard_field_51_readback": key.as_deref().map(field_51_readback),
            "field_51_written": 1234, "field_51_readback": readback,
            "field_51_roundtrip": readback == Some(1234),
            "arrival_tested": false,
            "arrival_reason": "no events posted; roundtrip does not prove target receipt",
            "skylight_symbols": skylight_symbols(),
        })
    })
    .await
    .map_err(|_| DriverError::Crashed {
        maybe_executed: false,
    })?;
    let probe_facts = probe.as_ref().map(|p| {
        json!({
            "window_location_symbol": p.window_location_symbol, "field_51": p.field_51,
            "arrival_verified": p.arrival_verified, "os_major": p.os_major,
            "appkit_ready": p.appkit_ready, "accessibility": p.accessibility,
            "secure_prompt": p.secure_prompt, "foreground": p.foreground,
            "skylight": p.skylight, "background_ready": p.background_ready(),
            "field_51_rule": "symbol && !arrival_failed && os_major >= 27; not a readback probe",
        })
    });
    let mut checks = Vec::new();
    if let Some(p) = &probe {
        for (name, passed) in [
            ("window_location_symbol", p.window_location_symbol),
            ("field_51", p.field_51),
            ("appkit_ready", p.appkit_ready),
            ("ax_trusted", p.accessibility),
            ("no_secure_prompt", !p.secure_prompt),
            (
                "os_major>=27 || arrival_verified",
                p.os_major >= 27 || p.arrival_verified,
            ),
        ] {
            checks.push(json!({"check": name, "passed": passed}));
        }
    }
    let reasons = json!({
        "observe.tree": {"declared": caps.observe.tree, "reason": "static implementation flag; AX permission checked per read", "ax_trusted": native["ax_trusted"]},
        "observe.screenshot": {"declared": caps.observe.screenshot, "reason": "static implementation flag; SCK/permission checked per capture", "screen_recording": native["screen_recording"], "sck_class": native["sck_screenshot_manager_class"]},
        "observe.events": {"declared": caps.observe.events, "reason": "static AXObserver support; registration not tested until subscribe", "ax_trusted": native["ax_trusted"]},
        "delivery.accessibility": {"declared": caps.delivery.accessibility, "reason": "static AX action support; Stop/permission/identity checked per action", "guards": guards},
        "delivery.targeted_pointer": {"declared": caps.delivery.targeted_pointer, "backend_installed": probe.is_some(), "checks": checks, "reason": "all Probe::background_ready checks required at backend injection; Chromium excluded per target"},
        "delivery.targeted_keys": {"declared": caps.delivery.targeted_keys, "backend_installed": probe.is_some(), "checks": checks, "reason": "same background_ready predicate as targeted_pointer"},
        "delivery.foreground": {"declared": caps.delivery.foreground, "probe_allows_hid": probe.as_ref().is_some_and(|p| p.foreground && p.appkit_ready && p.accessibility && !p.secure_prompt), "guards": guards, "reason": "probe permits HID plus healthy Stop/activity and unlatched Host; action additionally requires idle and route permission"},
        "overlay_excluded_from_capture": {"declared": caps.overlay_excluded_from_capture, "reason": "overlay not installed in this driver (P4-04); C21 skip expected"},
    });
    Ok(
        json!({"kind": "CAPABILITIES", "capabilities": caps, "reasons": reasons,
            "installed_probe": probe_facts, "native_checks": native, "guards": guards,
            "timeouts": {"ax_messaging_secs": crate::real::MESSAGING_TIMEOUT_SECS,
                "ax_job_wait_ms": crate::real::AX_CALL_TIMEOUT.as_millis(),
                "tree_walk_ms": crate::ax::TREE_BUILD_TIMEOUT.as_millis(),
                "driver_action_ms": gcu_core::driver::DRIVER_DEADLINE.as_millis(),
                "messaging_getter": "none; AX_CALL shows actual successful setter and unconfigured references"},
        }),
    )
}

fn field_51_readback(event: &CGEvent) -> i64 {
    CGEvent::set_integer_value_field(Some(event), CGEventField(51), 1234);
    CGEvent::integer_value_field(Some(event), CGEventField(51))
}

fn symbol(name: &CStr) -> bool {
    // SAFETY: name is NUL-terminated; returned addresses are only tested for null.
    !unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }.is_null()
}

fn skylight_symbols() -> Value {
    // SAFETY: constant system path; dlopen's reference is released below.
    let handle = unsafe {
        libc::dlopen(
            c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        )
    };
    let mut out = json!({"framework_loaded": !handle.is_null(),
        "feature": cfg!(feature = "macos-private-spi"),
        "disabled_env": std::env::var_os("GCU_DISABLE_SKYLIGHT").is_some()});
    for name in [c"SLEventPostToPid", c"SLPSPostEventRecordTo"] {
        // SAFETY: nonnull handle is owned here; no symbol is called.
        let found = !handle.is_null() && !unsafe { libc::dlsym(handle, name.as_ptr()) }.is_null();
        out[name.to_string_lossy().as_ref()] = json!(found);
    }
    if !handle.is_null() {
        // SAFETY: release only the dlopen reference acquired above.
        unsafe { libc::dlclose(handle) };
    }
    out
}

/// Scoped comparison of AccessKit activation switches on one fixture process.
/// Writes and restoration use the driver's AX worker, with 1s messaging/1.5s
/// waiter deadlines. Missing originals restore to false, matching ToggleStore.
/// Drop restores on ordinary error/unwind; process termination cannot restore.
pub struct FixtureToggles {
    inner: Arc<AccessibilityToggles<crate::real::AxToggleStore>>,
    pid: u32,
}

impl FixtureToggles {
    /// Bind only to the PID of the fixture resolved by the probe's path argument.
    pub fn new(driver: &MacosDriver, pid: u32) -> Result<Self> {
        let thread = driver
            .ax
            .clone()
            .ok_or(DriverError::CapabilityUnavailable)?;
        Ok(Self {
            inner: Arc::new(AccessibilityToggles::new(crate::real::AxToggleStore::new(
                thread,
            ))),
            pid,
        })
    }

    /// Record originals and enable both switches on the dedicated AX worker.
    pub async fn enable(&self) -> Result<()> {
        let inner = self.inner.clone();
        let pid = self.pid;
        tokio::task::spawn_blocking(move || {
            for name in [ToggleName::ManualAccessibility, ToggleName::EnhancedUserInterface] {
                let original = inner.store().get(pid, name)?;
                println!("{}", json!({"kind": "AX_TOGGLE", "phase": "original", "pid": pid, "attribute": name.as_str(), "value": original}));
            }
            inner.enable(pid)?;
            for name in [ToggleName::ManualAccessibility, ToggleName::EnhancedUserInterface] {
                let value = inner.store().get(pid, name)?;
                println!("{}", json!({"kind": "AX_TOGGLE", "phase": "enabled_readback", "pid": pid, "attribute": name.as_str(), "value": value}));
            }
            Ok(())
        }).await.map_err(|_| DriverError::Crashed { maybe_executed: false })?
    }

    /// Restore originals and report readback, before the final tree comparison.
    pub async fn restore(&self) -> Result<()> {
        let inner = self.inner.clone();
        let pid = self.pid;
        tokio::task::spawn_blocking(move || {
            inner.restore();
            for name in [ToggleName::ManualAccessibility, ToggleName::EnhancedUserInterface] {
                let value = inner.store().get(pid, name)?;
                println!("{}", json!({"kind": "AX_TOGGLE", "phase": "restored_readback", "pid": pid, "attribute": name.as_str(), "value": value}));
            }
            Ok(())
        }).await.map_err(|_| DriverError::Crashed { maybe_executed: false })?
    }
}
