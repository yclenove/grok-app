//! One bounded sender, independent of the thread that must service LL hooks.
use super::*;
use hook_probe::{Dispatch, MouseProbeApi, ProbeIdle, ProbeRequest, RecentInput};
use windows::Win32::System::SystemInformation::GetTickCount64;

#[derive(Debug)]
struct Report {
    keyboard: Dispatch,
    mouse: Result<Dispatch, String>,
    mode: MouseProbe,
    dispatched: bool,
}
pub(super) struct Sender {
    request: mpsc::SyncSender<ProbeRequest>,
    reply: mpsc::Receiver<Report>,
    pending: bool,
    report: Option<Report>,
    attempted: std::collections::BTreeSet<MouseProbe>,
}
impl Sender {
    pub(super) fn start(events: Arc<GuardEvents>, idle: Arc<ProbeIdle>) -> Result<Self, String> {
        events.startup_step("hook_probe_sender", STARTUP_STEP_TIMEOUT);
        let (request, receive) = mpsc::sync_channel::<ProbeRequest>(1);
        let (reply, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-hook-probes".into())
            .spawn(move || {
                while let Ok(request) = receive.recv() {
                    let mut api = NativeMouse {
                        events: &events,
                        tag: request.tag,
                        dpi: None,
                        dispatched: false,
                    };
                    let mouse = hook_probe::dispatch(&mut api, request.mode, &idle);
                    let dispatched = api.dispatched;
                    // A refused or changed-cursor probe never submits extra input.
                    // The initial idle sample preceded all probe input, including
                    // our key-up. Subsequent own injections preserve that marker.
                    let keyboard = if mouse
                        .as_ref()
                        .is_ok_and(|r| r.0 == request.mode.requested())
                    {
                        send(
                            &events,
                            "hook_keyboard_dispatch",
                            &[INPUT {
                                r#type: INPUT_KEYBOARD,
                                Anonymous: INPUT_0 {
                                    ki: KEYBDINPUT {
                                        wVk: VK_F24,
                                        dwFlags: KEYEVENTF_KEYUP,
                                        dwExtraInfo: PROBE_TAG,
                                        ..Default::default()
                                    },
                                },
                            }],
                        )
                    } else {
                        (0, 0, 0)
                    };
                    if reply
                        .send(Report {
                            keyboard,
                            mouse,
                            mode: request.mode,
                            dispatched,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .map_err(|e| format!("hook probe thread: {e}"))?;
        Ok(Self {
            request,
            reply: result,
            pending: false,
            report: None,
            attempted: Default::default(),
        })
    }
    pub(super) fn poll(&mut self) -> Result<(), String> {
        match self.reply.try_recv() {
            Ok(report) => {
                self.pending = false;
                if report.dispatched {
                    self.attempted.insert(report.mode);
                }
                let success = match &report.mouse {
                    Err(reason) => Err(reason.clone()),
                    Ok(mouse) if report.keyboard.0 == 1 && mouse.0 == report.mode.requested() => {
                        Ok(())
                    }
                    Ok(_) => Err(format!(
                        "SendInput(hook probes): {report:?}; UIPI may suppress errors"
                    )),
                };
                self.report = Some(report);
                CONTEXT.with(|slot| {
                    if let Some(context) = slot.borrow_mut().as_mut() {
                        context.mouse_proof.complete(success)?;
                    }
                    Ok::<_, String>(())
                })?;
            }
            Err(mpsc::TryRecvError::Empty) => (),
            Err(mpsc::TryRecvError::Disconnected) => return Err("hook probe sender exited".into()),
        }
        CONTEXT.with(|slot| {
            if let Some(context) = slot.borrow_mut().as_mut() {
                let mouse = context.mouse_proof.proven();
                context.events.startup_progress("mouse_hook_proof", format!(
                    "{} (attempted {} probe kinds)",
                    if mouse.is_some() { "mouse hook liveness proven by tagged injected callback" }
                    else { "mouse hook liveness could not be proven" }, self.attempted.len()
                ));
                context.events.startup_progress("hook_SendInput", format!(
                    "hooks_install=success; dispatch={:?}; pending={}; keyboard_seen={}; mouse_seen={}; keyboard_probe_flags={:?}; mouse_probe_flags={:?}; last_keyboard_flags={:?}; last_mouse_flags={:?}; attempted_mouse_probe_kinds={}",
                    self.report, self.pending, context.keyboard.is_some(), mouse.is_some(),
                    context.keyboard_probe_flags, context.mouse_probe_flags,
                    context.last_keyboard_flags, context.last_mouse_flags, self.attempted.len(),
                ));
                if !self.pending {
                    if let Some(request) = context.mouse_proof.next() {
                        self.request.try_send(request)
                            .map_err(|e| format!("hook probe submission: {e}"))?;
                        self.pending = true;
                    }
                }
            }
            Ok(())
        })
    }
}

struct NativeMouse<'a> {
    events: &'a GuardEvents,
    tag: usize,
    dpi: Option<mouse::Dpi>,
    dispatched: bool,
}
impl MouseProbeApi for NativeMouse<'_> {
    fn recent_input(&mut self) -> Result<RecentInput, String> {
        interactive_desktop()?;
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: initialized writable sized structure; read-only session clock.
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return Err("absolute mouse probe not sent: GetLastInputInfo failed".into());
        }
        // SAFETY: scalar read-only system uptime. Full width detects tick rollover.
        Ok(RecentInput {
            now: unsafe { GetTickCount64() },
            last: info.dwTime,
        })
    }
    fn cursor(&mut self) -> Result<Cursor, String> {
        interactive_desktop()?;
        if self.dpi.is_none() {
            self.dpi = Some(mouse::Dpi::enter()?);
        }
        mouse::cursor()
    }
    fn desktop(&mut self) -> Result<Desktop, String> {
        Ok(mouse::desktop())
    }
    fn send(&mut self, mode: MouseProbe, absolute: Option<(i32, i32)>) -> Result<Dispatch, String> {
        let inputs = match absolute {
            Some(at) => mouse::absolute(self.tag, at),
            None => mouse::inputs(mode, self.tag)?,
        };
        self.dispatched = true;
        let result = send(self.events, "hook_mouse_dispatch", &inputs);
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        if mode == MouseProbe::XButtonPair && result.0 == 1 {
            // Partial debug-only XDOWN needs exactly one matching XUP.
            if send(self.events, "hook_mouse_partial_release", &inputs[1..]).0 != 1 {
                return Err("partial mouse probe button release failed".into());
            }
        }
        Ok(result)
    }
}
fn send(events: &GuardEvents, stage: &'static str, inputs: &[INPUT]) -> Dispatch {
    events.startup_progress(stage, "SendInput pending; Win32 result unobserved");
    let started = Instant::now();
    // SAFETY: initialized up/zero/checked absolute or opted-in debug records.
    let sent = unsafe {
        SetLastError(ERROR_SUCCESS);
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32)
    };
    // SAFETY: capture this exact SendInput's error, never infer UIPI.
    let code = unsafe { GetLastError() }.0;
    let elapsed = started.elapsed().as_millis();
    events.startup_progress(
        stage,
        format!(
            "sent={sent}/{}; Win32={code}; elapsed_ms={elapsed}; UIPI cause not inferred",
            inputs.len()
        ),
    );
    (sent, code, elapsed)
}
