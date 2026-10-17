//! Permission poll and helper restart. No windowing toolkit.
//!
//! Screen Recording applies only to processes that start after the grant.
//! Accessibility applies immediately. This module never prompts TCC and never
//! opens Settings except through [`apply_ipc`].

use std::ffi::{OsStr, OsString};

use crate::exit::Failure;
use crate::helper::auth::{launch_command, launch_command_at, own_app_path};

/// How the helper should continue after the onboarding window returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentStart {
    /// Permissions are already usable, or the user dismissed the window.
    Serve,
    /// A waiter was spawned. The process must exit so the waiter can `exec`.
    ///
    /// Only the macOS window constructs this. Other builds still match it.
    #[cfg_attr(
        not(all(target_os = "macos", feature = "macos-webview")),
        allow(dead_code)
    )]
    Restarted,
}

/// One TCC fact. `false` means missing or the probe failed closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionSnapshot {
    /// `AXIsProcessTrusted` without a prompt.
    pub accessibility: bool,
    /// `CGPreflightScreenCaptureAccess` only. Never a request API.
    pub screen_recording: bool,
}

/// Card shown for one permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardStatus {
    /// Not granted in this process.
    Missing,
    /// Granted and already effective.
    Granted,
    /// Screen Recording was granted after this process started.
    NeedsRestart,
}

/// Which Settings pane a click may open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionKind {
    /// Privacy & Security → Accessibility.
    Accessibility,
    /// Privacy & Security → Screen Recording.
    ScreenRecording,
}

/// Rendered onboarding state. Restart is offered only for [`CardStatus::NeedsRestart`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OnboardingView {
    /// Accessibility card.
    pub accessibility: CardStatus,
    /// Screen Recording card.
    pub screen_recording: CardStatus,
    /// True only when Screen Recording needs a helper restart.
    pub restart_enabled: bool,
    /// Set when a restart was attempted and did not finish.
    pub restart_error: Option<RestartError>,
    /// Both permissions are effective. Closing does not grant anything.
    pub ready_to_close: bool,
}

/// Why a helper restart did not launch a new process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartError {
    /// Bundle id is not safe to pass to `open`.
    BadBundle,
    /// Pid 0 is not a real helper.
    BadPid,
    /// The running helper refused to exit.
    ExitFailed,
    /// Exit was accepted but the process is still alive.
    StillRunning,
    /// The waiter or `open` could not be spawned.
    LaunchFailed,
    /// The waiter gave up because the pid stayed alive.
    TimedOut,
    /// Screen Recording does not need a restart.
    NotNeeded,
}

/// What one IPC token did. Unknown tokens are [`IpcEffect::None`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpcEffect {
    /// Token ignored. No settings open and no restart.
    None,
    /// User dismissed. No grant, no settings, no restart.
    Dismiss,
    /// Waiter spawned after the helper accepted exit.
    Restarted,
    /// Restart failed. The window stays open.
    Failed(RestartError),
    /// Settings was opened from an explicit click.
    OpenedSettings,
}

/// Exit then schedule. A failed exit must not start a second helper.
///
/// Blocking, timeout, and cancellation are defined by the implementor.
/// [`perform_restart`] calls `exit_helper` before `schedule`, and `cancel_exit`
/// only when `schedule` returns an error.
pub trait RestartHost {
    /// Whether the helper process is still alive.
    fn running(&self) -> bool;
    /// Ask the helper to exit. `Ok` means the request was accepted.
    fn exit_helper(&mut self) -> Result<(), RestartError>;
    /// Undo a recorded exit after `schedule` fails.
    fn cancel_exit(&mut self);
    /// Spawn the waiter. Must not wait for this process to die.
    fn schedule(&mut self, pid: u32, argv: &[String]) -> Result<(), RestartError>;
}

/// Opens one allowlisted Settings URL. Return false when the open did not start.
pub trait SettingsOpener {
    /// Open `url`. Blocking and timeout are defined by the implementor.
    fn open(&mut self, url: &str) -> bool;
}

/// Accessibility pane. macOS 13+ Settings URL. Not an http URL.
pub const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility";

/// Screen Recording pane.
pub const SCREEN_RECORDING_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ScreenCapture";

/// Settings URL for `kind`. Pure. Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn settings_url(kind: PermissionKind) -> &'static str {
    match kind {
        PermissionKind::Accessibility => ACCESSIBILITY_SETTINGS_URL,
        PermissionKind::ScreenRecording => SCREEN_RECORDING_SETTINGS_URL,
    }
}

/// True only for the two Settings URLs this window may open.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn open_settings_allowed(url: &str) -> bool {
    url == ACCESSIBILITY_SETTINGS_URL || url == SCREEN_RECORDING_SETTINGS_URL
}

/// Open the window only on macOS when either permission is missing.
///
/// A process that already has Screen Recording does not need a restart.
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn should_open_onboarding(macos: bool, snapshot: PermissionSnapshot) -> bool {
    macos && (!snapshot.accessibility || !snapshot.screen_recording)
}

/// Poll model. The first sample counts as one. Two identical samples settle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnboardingModel {
    initial_screen: bool,
    current: PermissionSnapshot,
    stable: u32,
    restart_error: Option<RestartError>,
}

impl OnboardingModel {
    /// Start from the permissions this process had when it launched.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: the
    /// value is not shared.
    #[must_use]
    pub fn new(initial: PermissionSnapshot) -> Self {
        Self {
            initial_screen: initial.screen_recording,
            current: initial,
            stable: 1,
            restart_error: None,
        }
    }

    /// Record one probe. Does not open Settings or restart the helper.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn sample(&mut self, next: PermissionSnapshot) {
        if next == self.current {
            self.stable = self.stable.saturating_add(1);
        } else {
            self.current = next;
            self.stable = 1;
        }
    }

    /// True after two identical samples, including the initial one plus one repeat.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared borrow.
    #[must_use]
    pub fn settled(&self) -> bool {
        self.stable >= 2
    }

    /// Cards derived from the latest sample.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared borrow.
    #[must_use]
    pub fn view(&self) -> OnboardingView {
        let accessibility = if self.current.accessibility {
            CardStatus::Granted
        } else {
            CardStatus::Missing
        };
        let screen_recording = if !self.initial_screen && self.current.screen_recording {
            CardStatus::NeedsRestart
        } else if self.current.screen_recording {
            CardStatus::Granted
        } else {
            CardStatus::Missing
        };
        OnboardingView {
            accessibility,
            screen_recording,
            restart_enabled: screen_recording == CardStatus::NeedsRestart,
            restart_error: self.restart_error,
            ready_to_close: accessibility == CardStatus::Granted
                && screen_recording == CardStatus::Granted,
        }
    }

    /// Restart when the screen card says so. Stores the error for the page.
    ///
    /// Does not call `host` when a restart is not needed, the bundle is unsafe,
    /// or the pid is 0. Blocking, timeout, and cancellation follow `host`.
    pub fn restart(
        &mut self,
        bundle: &str,
        pid: u32,
        host: &mut dyn RestartHost,
    ) -> Result<(), RestartError> {
        if !self.view().restart_enabled {
            self.restart_error = Some(RestartError::NotNeeded);
            return Err(RestartError::NotNeeded);
        }
        match perform_restart(bundle, pid, host) {
            Ok(()) => {
                self.restart_error = None;
                Ok(())
            }
            Err(error) => {
                self.restart_error = Some(error);
                Err(error)
            }
        }
    }
}

/// Exit the helper, then schedule `open` with [`launch_command`].
///
/// `BadBundle` and `BadPid` do not touch `host`. A failed `schedule` clears the
/// exit so a dead waiter cannot be the only outcome while this process lives.
/// Blocking, timeout, and cancellation follow `host`.
pub fn perform_restart(
    bundle: &str,
    pid: u32,
    host: &mut dyn RestartHost,
) -> Result<(), RestartError> {
    let by_bundle = launch_command(bundle).ok_or(RestartError::BadBundle)?;
    // Relaunch this exact app: a bundle id may resolve to another registered copy.
    let argv = own_app_path()
        .and_then(|app| launch_command_at(&app))
        .unwrap_or(by_bundle);
    if pid == 0 {
        return Err(RestartError::BadPid);
    }
    host.exit_helper()?;
    if host.running() {
        return Err(RestartError::StillRunning);
    }
    match host.schedule(pid, &argv) {
        Ok(()) => Ok(()),
        Err(error) => {
            host.cancel_exit();
            Err(error)
        }
    }
}

/// Shell script that `exec`s `argv` only after `pid` has exited.
///
/// The script exits 1 if the pid is still alive after about five seconds, so a
/// failed exit cannot start a second helper. Each `argv` entry is POSIX-quoted;
/// empty entries and control characters are refused. Blocking: none. Timeout: none. Cancellation: none.
pub fn relaunch_waiter(pid: u32, argv: &[String]) -> Result<String, RestartError> {
    if pid == 0 {
        return Err(RestartError::BadPid);
    }
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| arg.is_empty() || arg.chars().any(char::is_control))
    {
        return Err(RestartError::LaunchFailed);
    }
    let joined = argv
        .iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!(
        "i=0; while [ \"$i\" -lt 50 ]; do if ! kill -0 {pid} 2>/dev/null; then exec {joined}; fi; i=$((i+1)); sleep 0.1; done; exit 1"
    ))
}

/// POSIX single-quote `arg`; safe for any text without control characters.
fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/'))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// Apply one IPC token. Unknown tokens have no side effects.
///
/// `dismiss` does not open Settings or restart. Settings tokens open only the
/// matching allowlisted URL. Blocking, timeout, and cancellation follow `host`
/// and `settings`.
pub fn apply_ipc(
    model: &mut OnboardingModel,
    token: &str,
    bundle: &str,
    pid: u32,
    host: &mut dyn RestartHost,
    settings: &mut dyn SettingsOpener,
) -> IpcEffect {
    match token {
        "dismiss" => IpcEffect::Dismiss,
        "restart" => match model.restart(bundle, pid, host) {
            Ok(()) => IpcEffect::Restarted,
            Err(error) => IpcEffect::Failed(error),
        },
        "settings:accessibility" => open_kind(settings, PermissionKind::Accessibility),
        "settings:screen_recording" => open_kind(settings, PermissionKind::ScreenRecording),
        _ => IpcEffect::None,
    }
}

fn open_kind(settings: &mut dyn SettingsOpener, kind: PermissionKind) -> IpcEffect {
    let url = settings_url(kind);
    if open_settings_allowed(url) && settings.open(url) {
        IpcEffect::OpenedSettings
    } else {
        IpcEffect::None
    }
}

/// True when argv is the real `agent` subcommand, not `--driver fake`.
///
/// `--scene agent` is not the subcommand. Pure. Blocking: none. Timeout: none.
/// Cancellation: none. This does not grant TCC.
#[must_use]
pub fn argv_requests_onboarding(args: &[OsString]) -> bool {
    !driver_is_fake(args) && first_positional(args).is_some_and(|arg| arg == "agent")
}

fn driver_is_fake(args: &[OsString]) -> bool {
    let mut expect = false;
    for arg in args {
        if expect {
            return arg.eq_ignore_ascii_case("fake");
        }
        if arg == "--driver" {
            expect = true;
            continue;
        }
        if let Some(value) = arg.to_str().and_then(|text| text.strip_prefix("--driver=")) {
            if value.eq_ignore_ascii_case("fake") {
                return true;
            }
        }
    }
    false
}

fn first_positional(args: &[OsString]) -> Option<&OsStr> {
    let mut expect_value = false;
    for arg in args {
        if expect_value {
            expect_value = false;
            continue;
        }
        let Some(text) = arg.to_str() else {
            continue;
        };
        if text == "--driver" || text == "--scene" {
            expect_value = true;
            continue;
        }
        if text.starts_with("--driver=") || text.starts_with('-') {
            continue;
        }
        return Some(arg.as_os_str());
    }
    None
}

/// Probe this process. Failure is "not granted". macOS only.
///
/// Blocking: the TCC preflight calls return immediately; they do not prompt.
/// Timeout: none beyond those calls. Cancellation: none. Thread safety: the
/// probes are main-thread AppKit calls.
#[cfg(all(target_os = "macos", feature = "macos-webview"))]
#[must_use]
pub(crate) fn live_snapshot() -> PermissionSnapshot {
    use gcu_driver_macos::api::MacApi;
    let api = gcu_driver_macos::RealMacApi;
    PermissionSnapshot {
        accessibility: api.accessibility_trusted().unwrap_or(false),
        screen_recording: api.screen_recording_ok().unwrap_or(false),
    }
}

/// Show onboarding before the helper serves, when this process is missing a permission.
///
/// On macOS, with `macos-webview`, the wry loop runs on the calling (main)
/// thread and returns. Dismiss continues to serve. A successful restart has
/// already spawned the waiter; the caller must exit without serving. Without
/// that feature, and on other systems, this returns [`AgentStart::Serve`] and
/// does not open a window. Blocking: the window loop waits for the user.
/// Timeout: permission probes and Settings opens are capped inside the window.
/// Cancellation: closing the window is dismiss.
pub(crate) fn prepare_agent_onboarding() -> Result<AgentStart, Failure> {
    #[cfg(all(target_os = "macos", feature = "macos-webview"))]
    {
        let snapshot = live_snapshot();
        if !should_open_onboarding(true, snapshot) {
            return Ok(AgentStart::Serve);
        }
        super::window::run(snapshot)
    }
    #[cfg(not(all(target_os = "macos", feature = "macos-webview")))]
    {
        Ok(AgentStart::Serve)
    }
}
