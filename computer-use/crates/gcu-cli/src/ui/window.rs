//! macOS onboarding window. Compiled only with `macos-webview`, without wry's Linux WebKit feature.
//!
//! Dismiss returns to the helper. Restart spawns a waiter and asks the caller to
//! exit. The waiter `exec`s `open` only after this pid is gone, so a failed exit
//! cannot start a second helper.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::platform::run_return::EventLoopExtRunReturn;
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, PermissionResponse, WebViewBuilder};

use super::document::{navigation_allowed, onboarding_page, status_script};
use super::onboarding::{
    apply_ipc, live_snapshot, open_settings_allowed, relaunch_waiter, AgentStart, IpcEffect,
    OnboardingModel, PermissionSnapshot, RestartError, RestartHost, SettingsOpener,
};
use crate::exit::Failure;
use crate::helper::auth::current_bundle_id;

const POLL: Duration = Duration::from_millis(400);
const SETTINGS_TIMEOUT: Duration = Duration::from_secs(5);

struct LiveRestart {
    exited: bool,
}

impl RestartHost for LiveRestart {
    fn running(&self) -> bool {
        !self.exited
    }

    fn exit_helper(&mut self) -> Result<(), RestartError> {
        // The process exits after `run_return` when the effect is `Restarted`.
        // Recording it here lets `perform_restart` refuse to schedule if we are
        // still marked running. It does not signal this pid.
        self.exited = true;
        Ok(())
    }

    fn cancel_exit(&mut self) {
        self.exited = false;
    }

    fn schedule(&mut self, pid: u32, argv: &[String]) -> Result<(), RestartError> {
        let script = relaunch_waiter(pid, argv)?;
        match std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .spawn()
        {
            Ok(child) => {
                // The parent is about to exit. Waiting here deadlocks: the
                // waiter waits for this pid. Dropping the child does not lose
                // the process; init reaps it after we exit.
                drop(child);
                Ok(())
            }
            Err(_) => Err(RestartError::LaunchFailed),
        }
    }
}

struct SystemSettings;

impl SettingsOpener for SystemSettings {
    fn open(&mut self, url: &str) -> bool {
        open_settings(url)
    }
}

/// Run the window on the main thread and return when the user is done.
///
/// Blocking: until dismiss, close, an effective grant, or a scheduled restart.
/// Settings `open` is capped at five seconds. Permission probes do not prompt.
/// Cancellation: closing the window is dismiss and does not grant permission.
pub(crate) fn run(initial: PermissionSnapshot) -> Result<AgentStart, Failure> {
    let locale = gcu_mcp::active_locale();
    let mut model = OnboardingModel::new(initial);
    let queue = std::sync::Arc::new(std::sync::Mutex::new(
        std::collections::VecDeque::<String>::new(),
    ));
    let incoming = queue.clone();
    let event_loop = EventLoop::new();
    let title = gcu_mcp::text(locale, "onboarding.title");
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(440.0, 560.0))
        .with_resizable(false)
        .build(&event_loop)
        .map_err(|_| Failure::internal("Cannot open the onboarding window"))?;
    window.set_visible(true);
    window.set_focus();
    let html = onboarding_page(locale, &model.view());
    let webview = WebViewBuilder::new()
        .with_html(html)
        .with_background_color((13, 13, 13, 255))
        .with_ipc_handler(move |request| {
            if let Ok(mut guard) = incoming.lock() {
                guard.push_back(request.body().clone());
            }
        })
        .with_navigation_handler(|url| navigation_allowed(&url))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_permission_handler(|_| PermissionResponse::Deny)
        .with_download_started_handler(|_, _| false)
        .build(&window)
        .map_err(|_| Failure::internal("Cannot open the onboarding window"))?;

    let mut host = LiveRestart { exited: false };
    let mut settings = SystemSettings;
    let mut outcome = AgentStart::Serve;
    let bundle = current_bundle_id();
    let pid = std::process::id();
    let mut event_loop = event_loop;
    event_loop.run_return(|event, _, control| {
        *control = ControlFlow::WaitUntil(Instant::now() + POLL);
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = &event
        {
            outcome = AgentStart::Serve;
            *control = ControlFlow::Exit;
            return;
        }
        if !matches!(event, Event::MainEventsCleared) {
            return;
        }
        model.sample(live_snapshot());
        let mut messages = Vec::new();
        if let Ok(mut guard) = queue.lock() {
            messages.extend(guard.drain(..));
        }
        for message in messages {
            match apply_ipc(&mut model, &message, bundle, pid, &mut host, &mut settings) {
                IpcEffect::Dismiss => {
                    outcome = AgentStart::Serve;
                    *control = ControlFlow::Exit;
                    return;
                }
                IpcEffect::Restarted => {
                    outcome = AgentStart::Restarted;
                    *control = ControlFlow::Exit;
                    return;
                }
                IpcEffect::None | IpcEffect::Failed(_) | IpcEffect::OpenedSettings => {}
            }
        }
        if model.settled() && model.view().ready_to_close {
            outcome = AgentStart::Serve;
            *control = ControlFlow::Exit;
            return;
        }
        let script = status_script(locale, &model.view());
        let _ = webview.evaluate_script(&script);
    });
    Ok(outcome)
}

fn open_settings(url: &str) -> bool {
    if !open_settings_allowed(url) {
        return false;
    }
    let (sender, receiver) = mpsc::channel();
    let owned = url.to_string();
    let started = std::thread::Builder::new()
        .name("gcu-onboarding-settings".into())
        .spawn(move || {
            let ok = std::process::Command::new("/usr/bin/open")
                .arg(&owned)
                .status()
                .map(|status| status.success())
                .unwrap_or(false);
            let _ = sender.send(ok);
        });
    if started.is_err() {
        return false;
    }
    receiver.recv_timeout(SETTINGS_TIMEOUT).unwrap_or(false)
}
