//! Fixture-instance identity for one Windows evaluation scope.
//!
//! The inputs are a process id, the creation time read from that process's
//! handle, a boot identity, an HWND, and its creation generation. A path, image
//! name, window title, class, or rectangle is not an input. A failed proof is
//! [`DriverError::ScopeDenied`] and never selects another window.
//!
//! Memory only for [`prove_process`], [`prove_window`], and [`prove_element`].
//! [`live_process`] and [`live_window`] read metadata through [`WinApi`] and do
//! not capture pixels or send input.

use crate::{api::WinApi, desktop::window_process};
use gcu_core::{
    driver::{DriverError, Result, WindowRef},
    scope::{BootIdentity, OsStartTime, ProcessInstance, WindowBinding},
};

/// One process the scope is willing to name. Zero pid or creation time, or an
/// empty boot identity, is not evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProcessClaim {
    pub(crate) pid: u32,
    pub(crate) created: u64,
    pub(crate) boot: String,
}

/// One top-level window owned by [`ProcessClaim`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowClaim {
    pub(crate) hwnd: u64,
    pub(crate) incarnation: u64,
    pub(crate) owner: ProcessClaim,
}

/// Process and top-level window of one UIA fragment. No name, value, or pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FragmentFact {
    /// `CurrentProcessId` of the fragment, not the requested window's pid.
    pub(crate) owner_pid: u32,
    /// Top-level HWND that contains the fragment.
    pub(crate) top_hwnd: u64,
}

/// Scope copied once for one job. Later pin clears do not change this value.
///
/// [`JobScope::Missing`] is a scoped session with no binding. It is never
/// treated as [`JobScope::Unscoped`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JobScope {
    /// Ordinary work. A foreign owner or a missing probe stays permitted.
    Unscoped,
    /// Admitted window, copied by value. Later lookups do not replace it.
    Scoped(WindowClaim),
    /// Scoped session whose binding is gone. The job must refuse.
    Missing,
}

impl JobScope {
    /// Work that was not admitted under a scope.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn unscoped() -> Self {
        Self::Unscoped
    }

    /// `Ok(None)` is unscoped. `Ok(Some)` is the admitted window.
    /// `Err` is a scoped job with no binding. The pin slot is not read.
    ///
    /// Memory only. No blocking, timeout, or cancellation.
    pub(crate) fn claim(&self) -> Result<Option<&WindowClaim>> {
        match self {
            Self::Unscoped => Ok(None),
            Self::Scoped(claim) => Ok(Some(claim)),
            Self::Missing => Err(DriverError::ScopeDenied),
        }
    }
}

/// The child that will receive input must be the admitted process instance.
///
/// Memory only. No blocking, timeout, or cancellation. A different creation
/// time is a different process, including the same pid.
pub(crate) fn admit_recipient(
    admitted: &WindowClaim,
    recipient_pid: u32,
    recipient_created: u64,
) -> Result<()> {
    if !proved_process(&admitted.owner) || admitted.hwnd == 0 || admitted.incarnation == 0 {
        return Err(DriverError::ScopeDenied);
    }
    if recipient_pid == 0
        || recipient_created == 0
        || recipient_pid != admitted.owner.pid
        || recipient_created != admitted.owner.created
    {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// The fragment must belong to the admitted window. `None` is unproved.
///
/// Memory only. No blocking, timeout, or cancellation. The requested window's
/// owner is not a substitute for a missing fact.
pub(crate) fn admit_fragment(admitted: &WindowClaim, fact: Option<FragmentFact>) -> Result<()> {
    if !proved_process(&admitted.owner) || admitted.hwnd == 0 || admitted.incarnation == 0 {
        return Err(DriverError::ScopeDenied);
    }
    let Some(fact) = fact else {
        return Err(DriverError::ScopeDenied);
    };
    if fact.owner_pid == 0
        || fact.top_hwnd == 0
        || fact.owner_pid != admitted.owner.pid
        || fact.top_hwnd != admitted.hwnd
    {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// Boot identity is an OS boot-session id. The wall-clock origin is not one.
///
/// A missing, empty, oversized, or purely numeric session fails closed. The
/// numeric form is the rejected uptime estimate and is never promoted into
/// [`BootIdentity`]. Memory only. No blocking, timeout, or cancellation.
pub(crate) fn boot_token(session: Option<&str>, wall_clock_origin: Option<u64>) -> Result<String> {
    let _ = wall_clock_origin;
    let Some(session) = session.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(DriverError::ScopeDenied);
    };
    if session.len() > 256 || session.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DriverError::ScopeDenied);
    }
    Ok(session.to_owned())
}

/// Fields parsed from a discovery stamp. Class text is checked only so a
/// non-stamp id does not parse; it is not compared as authorization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ParsedStamp {
    pub(crate) hwnd: u64,
    pub(crate) pid: u32,
    pub(crate) process_created: u64,
    pub(crate) created: u64,
}

fn proved_process(claim: &ProcessClaim) -> bool {
    claim.pid != 0 && claim.created != 0 && !claim.boot.is_empty() && claim.boot.len() <= 256
}

/// Same pid and creation time with a different boot is a different boot, not a match.
pub(crate) fn prove_process(stored: &ProcessClaim, live: &ProcessClaim) -> Result<()> {
    if !proved_process(stored) || !proved_process(live) {
        return Err(DriverError::ScopeDenied);
    }
    if stored.pid != live.pid || stored.created != live.created || stored.boot != live.boot {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// HWND reuse is the same handle with a new creation generation. Owner pid or
/// owner creation-time changes are a different owner, including the same pid.
pub(crate) fn prove_window(stored: &WindowClaim, live: &WindowClaim) -> Result<()> {
    prove_process(&stored.owner, &live.owner)?;
    if stored.hwnd == 0 || stored.incarnation == 0 || live.hwnd == 0 || live.incarnation == 0 {
        return Err(DriverError::ScopeDenied);
    }
    if stored.hwnd != live.hwnd || stored.incarnation != live.incarnation {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// Element identity is the admitted window plus the element's owner pid.
/// `owner_pid == None` is unproved. A different HWND or window id is a handle
/// that crossed windows, including another top-level window in the same process.
pub(crate) fn prove_element(
    admitted: &WindowClaim,
    admitted_id: &str,
    element_hwnd: u64,
    element_window_id: &str,
    owner_pid: Option<u32>,
) -> Result<()> {
    if !proved_process(&admitted.owner)
        || admitted.hwnd == 0
        || admitted.incarnation == 0
        || element_hwnd == 0
        || element_hwnd != admitted.hwnd
        || element_window_id != admitted_id
    {
        return Err(DriverError::ScopeDenied);
    }
    match owner_pid {
        Some(pid) if pid != 0 && pid == admitted.owner.pid => Ok(()),
        _ => Err(DriverError::ScopeDenied),
    }
}

/// Delivery-time element check. An unset owner probe (`None`) does not by
/// itself deny, because the portable UIA fake has no `CurrentProcessId`.
/// A reported pid, a zero pid, a zero HWND, or a stamp for a different HWND does.
/// Real UIA returns `Ok(Some(_))` or `Err`, never `Ok(None)`.
pub(crate) fn admit_delivery_element(
    admitted_hwnd: u64,
    element_hwnd: u64,
    element_window_id: &str,
    owner_pid: Option<u32>,
    admitted_pid: Option<u32>,
) -> Result<()> {
    if admitted_hwnd == 0 || element_hwnd == 0 || element_hwnd != admitted_hwnd {
        return Err(DriverError::ScopeDenied);
    }
    if let Some(stamp) = parse_stamp(element_window_id) {
        if stamp.hwnd != admitted_hwnd {
            return Err(DriverError::ScopeDenied);
        }
    }
    match owner_pid {
        None => Ok(()),
        Some(0) => Err(DriverError::ScopeDenied),
        Some(owner) => match admitted_pid {
            Some(pid) if pid == owner => Ok(()),
            _ => Err(DriverError::ScopeDenied),
        },
    }
}

/// Scalars that must be known before a packet is handed to the input API.
/// This does not open a process handle and does not run inside the pointer gate.
pub(crate) fn require_delivery_scalars(
    window_hwnd: u64,
    pid: u32,
    process_created: u64,
    incarnation: u64,
    recipient_hwnd: u64,
) -> Result<()> {
    if window_hwnd == 0
        || pid == 0
        || process_created == 0
        || incarnation == 0
        || recipient_hwnd == 0
    {
        return Err(DriverError::ScopeDenied);
    }
    Ok(())
}

/// Parse a discovery stamp. Any other id, including a bare HWND, is `None`.
pub(crate) fn parse_stamp(id: &str) -> Option<ParsedStamp> {
    let mut parts = id.splitn(5, ':');
    let hwnd = u64::from_str_radix(parts.next()?, 16).ok()?;
    let pid = parts.next()?.parse().ok()?;
    let process_created = u64::from_str_radix(parts.next()?, 16).ok()?;
    let created = u64::from_str_radix(parts.next()?, 16).ok()?;
    let class = decode_class(parts.next()?)?;
    if hwnd == 0 || pid == 0 || process_created == 0 || created == 0 || class.is_empty() {
        return None;
    }
    Some(ParsedStamp {
        hwnd,
        pid,
        process_created,
        created,
    })
}

fn decode_class(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

/// Live process identity. The image path is not read.
pub(crate) fn live_process(api: &dyn WinApi, pid: u32) -> Option<ProcessInstance> {
    if pid == 0 {
        return None;
    }
    let created = api.process_security(pid).ok()?.created;
    let boot = api.boot_identity().ok()?;
    ProcessInstance::new(
        pid,
        OsStartTime::new(created).ok()?,
        BootIdentity::new(boot).ok()?,
    )
    .ok()
}

/// Live window identity from the stamp and a fresh enumeration.
///
/// The stamp's pid and handle creation time must still match that process.
/// ApplicationFrameHost unwraps to the child pid; that child's handle creation
/// time is the owner. A path match is not a proof. `None` does not name a
/// substitute window.
pub(crate) fn live_window(api: &dyn WinApi, window: &WindowRef) -> Option<WindowBinding> {
    let stamp = parse_stamp(&window.id)?;
    let boot = api.boot_identity().ok()?;
    let current = api
        .enum_windows()
        .ok()?
        .into_iter()
        .find(|item| item.hwnd == stamp.hwnd)?;
    if current.hwnd != stamp.hwnd || current.created != stamp.created {
        return None;
    }
    let host_created = api.process_security(stamp.pid).ok()?.created;
    if host_created == 0 || host_created != stamp.process_created {
        return None;
    }
    let owner = window_process(api, &current).ok()?;
    let owner_created = if owner == stamp.pid {
        host_created
    } else {
        api.process_security(owner).ok()?.created
    };
    if owner == 0 || owner_created == 0 {
        return None;
    }
    if window.app.info.pid.is_some_and(|pid| pid != owner) {
        return None;
    }
    let stored_owner = ProcessClaim {
        pid: owner,
        created: owner_created,
        boot: boot.clone(),
    };
    let live_owner = stored_owner.clone();
    prove_process(&stored_owner, &live_owner).ok()?;
    prove_window(
        &WindowClaim {
            hwnd: stamp.hwnd,
            incarnation: stamp.created,
            owner: stored_owner,
        },
        &WindowClaim {
            hwnd: current.hwnd,
            incarnation: current.created,
            owner: live_owner,
        },
    )
    .ok()?;
    let instance = ProcessInstance::new(
        owner,
        OsStartTime::new(owner_created).ok()?,
        BootIdentity::new(boot).ok()?,
    )
    .ok()?;
    WindowBinding::new(window.id.clone(), current.created, instance).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        desktop::HwndStamp,
        testing::{medium_security, window, FakeWinApi},
    };
    use gcu_core::{
        driver::{AppRef, WindowRef},
        scope::SCOPE_DENIED_MESSAGE,
    };
    use gcu_protocol::AppInfo;

    fn process(pid: u32, created: u64, boot: &str) -> ProcessClaim {
        ProcessClaim {
            pid,
            created,
            boot: boot.into(),
        }
    }

    fn claims() -> (WindowClaim, WindowClaim) {
        let owner = process(7, 42, "boot-1");
        let window = WindowClaim {
            hwnd: 8,
            incarnation: 99,
            owner,
        };
        (window.clone(), window)
    }

    fn denied(err: Result<()>) {
        let err = err.expect_err("unproved");
        assert_eq!(err, DriverError::ScopeDenied);
        assert_eq!(err.to_string(), SCOPE_DENIED_MESSAGE);
        assert!(!err.to_string().to_lowercase().contains("editor"));
        assert!(!err.maybe_executed());
    }

    #[test]
    fn matching_process_window_and_element_are_proved_without_a_path() {
        let (stored, live) = claims();
        prove_process(&stored.owner, &live.owner).unwrap();
        prove_window(&stored, &live).unwrap();
        prove_element(&stored, "stamp", 8, "stamp", Some(7)).unwrap();
    }

    #[test]
    fn pid_reuse_hwnd_reuse_owner_change_and_crossed_handle_are_scope_denied() {
        let (stored, mut live) = claims();
        live.owner.created += 1;
        denied(prove_process(&stored.owner, &live.owner));
        denied(prove_window(&stored, &live));

        let (_, mut reused) = claims();
        reused.incarnation += 1;
        denied(prove_window(&stored, &reused));

        let (_, mut owner) = claims();
        owner.owner.pid = 8;
        denied(prove_window(&stored, &owner));

        denied(prove_element(&stored, "stamp-a", 8, "stamp-b", Some(7)));
        denied(prove_element(&stored, "stamp-a", 9, "stamp-a", Some(7)));
        denied(prove_element(&stored, "stamp-a", 8, "stamp-a", Some(8)));
        denied(prove_element(&stored, "stamp-a", 8, "stamp-a", None));
        denied(prove_element(&stored, "stamp-a", 8, "stamp-a", Some(0)));
    }

    #[test]
    fn equal_zeros_and_empty_boot_are_unproved() {
        let mut stored = process(7, 0, "boot-1");
        let live = stored.clone();
        denied(prove_process(&stored, &live));
        stored.created = 42;
        stored.boot.clear();
        denied(prove_process(&stored, &stored));
        stored.boot = "boot-1".into();
        stored.pid = 0;
        denied(prove_process(&stored, &stored));

        let (mut window, _) = claims();
        window.incarnation = 0;
        denied(prove_window(&window, &window));
        window.incarnation = 99;
        window.hwnd = 0;
        denied(prove_window(&window, &window));
        assert!(require_delivery_scalars(8, 7, 42, 99, 9).is_ok());
        denied(require_delivery_scalars(8, 7, 0, 99, 9));
        denied(require_delivery_scalars(0, 7, 42, 99, 9));
        denied(require_delivery_scalars(8, 0, 42, 99, 9));
        denied(require_delivery_scalars(8, 7, 42, 0, 9));
        denied(require_delivery_scalars(8, 7, 42, 99, 0));
    }

    #[test]
    fn different_boot_is_not_the_same_instance() {
        let stored = process(7, 42, "boot-1");
        let live = process(7, 42, "boot-2");
        denied(prove_process(&stored, &live));
    }

    #[test]
    fn boot_token_uses_the_session_and_ignores_the_wall_clock() {
        let wall = 1_700_000_000_u64;
        let first = boot_token(Some("boot-session-a"), Some(wall)).expect("session");
        let second = boot_token(Some("boot-session-b"), Some(wall)).expect("other boot");
        assert_ne!(first, second);
        assert_eq!(
            first,
            boot_token(Some("boot-session-a"), Some(wall.saturating_add(50))).expect("same boot")
        );
        let numeric = wall.to_string();
        for session in [
            None,
            Some(""),
            Some("   "),
            Some(numeric.as_str()),
            Some("1700000000"),
        ] {
            let err = boot_token(session, Some(wall)).expect_err("unproved boot");
            assert_eq!(err, DriverError::ScopeDenied);
            assert!(!err.maybe_executed());
        }
    }

    #[test]
    fn delivery_allows_an_unset_owner_and_rejects_a_crossed_or_foreign_owner() {
        admit_delivery_element(8, 8, "8", None, Some(7)).unwrap();
        denied(admit_delivery_element(8, 9, "8", Some(7), Some(7)));
        denied(admit_delivery_element(8, 8, "8", Some(99), Some(7)));
        denied(admit_delivery_element(8, 8, "8", Some(0), Some(7)));
        denied(admit_delivery_element(0, 0, "8", None, Some(7)));
        let other = HwndStamp::from(&window(9, 7)).id();
        denied(admit_delivery_element(8, 8, &other, Some(7), Some(7)));
    }

    #[test]
    fn bare_hwnd_is_not_a_stamp_and_a_real_stamp_roundtrips() {
        assert!(parse_stamp("8").is_none());
        assert!(parse_stamp("").is_none());
        let native = window(8, 7);
        let id = HwndStamp::from(&native).id();
        let parsed = parse_stamp(&id).unwrap();
        assert_eq!(parsed.hwnd, 8);
        assert_eq!(parsed.pid, 7);
        assert_eq!(parsed.process_created, native.process_created);
        assert_eq!(parsed.created, native.created);
    }

    fn app(pid: u32, id: &str) -> WindowRef {
        WindowRef {
            app: AppRef {
                info: AppInfo {
                    id: "C:\\Editor.exe".into(),
                    name: "Editor".into(),
                    pid: Some(pid),
                    identity: None,
                },
            },
            id: id.into(),
        }
    }

    #[test]
    fn live_window_ignores_path_and_rejects_reuse_owner_change_and_a_forged_id() {
        let api = FakeWinApi::default();
        api.update(|state| {
            state.paths.insert(7, "C:\\Editor.exe".into());
            state.processes.insert(7, medium_security());
            state.windows.push(window(8, 7));
            state.windows.push(window(9, 7));
        });
        let id = HwndStamp::from(&window(8, 7)).id();
        let win = app(7, &id);
        let first = live_window(&api, &win).expect("proved");
        assert_eq!(first.owner().pid(), 7);
        assert_eq!(first.incarnation(), 99);
        api.update(|state| {
            state.paths.insert(7, "C:\\Impostor.exe".into());
        });
        assert_eq!(live_window(&api, &win), Some(first.clone()));
        api.update(|state| state.processes.get_mut(&7).unwrap().created += 1);
        assert!(live_window(&api, &win).is_none());
        api.update(|state| state.processes.get_mut(&7).unwrap().created -= 1);
        api.update(|state| {
            if let Some(item) = state.windows.iter_mut().find(|item| item.hwnd == 8) {
                item.created += 1;
            }
        });
        assert!(live_window(&api, &win).is_none());
        api.update(|state| {
            if let Some(item) = state.windows.iter_mut().find(|item| item.hwnd == 8) {
                item.created -= 1;
                item.pid = 11;
            }
            state.paths.insert(11, "C:\\Other.exe".into());
            state.processes.insert(11, medium_security());
        });
        assert!(live_window(&api, &win).is_none());
        assert!(live_window(&api, &app(7, "8")).is_none());
        api.update(|state| state.boot.clear());
        assert!(live_process(&api, 7).is_none());
    }
}
