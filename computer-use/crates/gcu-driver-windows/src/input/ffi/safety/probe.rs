//! One bounded sender, independent of the thread that must service LL hooks.
use super::*;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
enum MouseProbe {
    ZeroMove,
    ZeroWheel,
}
#[derive(Debug)]
struct Report {
    keyboard: (u32, u32, u128),
    mouse: (u32, u32, u128),
    mode: MouseProbe,
}
pub(super) struct Sender {
    request: mpsc::SyncSender<MouseProbe>,
    reply: mpsc::Receiver<Report>,
    pending: bool,
    started: Instant,
    mode: MouseProbe,
    report: Option<Report>,
}
impl Sender {
    pub(super) fn start(events: Arc<GuardEvents>) -> Result<Self, String> {
        events.startup_step("hook_probe_sender", STARTUP_STEP_TIMEOUT);
        let (request, receive) = mpsc::sync_channel(1);
        let (reply, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-hook-probes".into())
            .spawn(move || {
                while let Ok(mode) = receive.recv() {
                    let keyboard = send(
                        &events,
                        "hook_keyboard_dispatch",
                        INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 {
                                ki: KEYBDINPUT {
                                    wVk: VK_F24,
                                    dwFlags: KEYEVENTF_KEYUP,
                                    dwExtraInfo: PROBE_TAG,
                                    ..Default::default()
                                },
                            },
                        },
                    );
                    let mouse = send(
                        &events,
                        "hook_mouse_dispatch",
                        INPUT {
                            r#type: INPUT_MOUSE,
                            Anonymous: INPUT_0 {
                                mi: MOUSEINPUT {
                                    dwFlags: match mode {
                                        MouseProbe::ZeroMove => {
                                            MOUSEEVENTF_MOVE | MOUSEEVENTF_MOVE_NOCOALESCE
                                        }
                                        MouseProbe::ZeroWheel => MOUSEEVENTF_WHEEL,
                                    },
                                    dwExtraInfo: PROBE_TAG,
                                    ..Default::default()
                                },
                            },
                        },
                    );
                    if reply
                        .send(Report {
                            keyboard,
                            mouse,
                            mode,
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
            started: Instant::now(),
            mode: MouseProbe::ZeroMove,
            report: None,
        })
    }
    pub(super) fn poll(&mut self) -> Result<(), String> {
        match self.reply.try_recv() {
            Ok(report) => {
                self.pending = false;
                let success = report.keyboard.0 == 1 && report.mouse.0 == 1;
                self.report = Some(report);
                if !success {
                    return Err(format!(
                        "SendInput(hook probes): {:?}; UIPI may suppress errors",
                        self.report
                    ));
                }
            }
            Err(mpsc::TryRecvError::Empty) => (),
            Err(mpsc::TryRecvError::Disconnected) => return Err("hook probe sender exited".into()),
        }
        CONTEXT.with(|slot| {
            if let Some(context) = slot.borrow().as_ref() {
                context.events.startup_progress("hook_SendInput", format!(
                    "hooks_install=success; dispatch={:?}; pending={}; keyboard_seen={}; mouse_seen={}; keyboard_probe_flags={:?}; mouse_probe_flags={:?}; last_keyboard_flags={:?}; last_mouse_flags={:?}; mouse_proof={}",
                    self.report.as_ref().map(|r| (r.mode, r.keyboard, r.mouse)), self.pending, context.keyboard.is_some(), context.mouse.is_some(),
                    context.keyboard_probe_flags, context.mouse_probe_flags,
                    context.last_keyboard_flags, context.last_mouse_flags,
                    if context.mouse.is_some() { "injected tagged callback observed" }
                    else if context.mouse_probe_flags.is_some() { "tagged callback observed but injected flags missing" }
                    else if context.last_mouse_flags.is_some() { "hook received other events; tagged probe unobserved (filter/UIPI not proven)" }
                    else { "no mouse callback observed; dispatch/pump/filter cause unproven" },
                ));
                // Some desktops suppress a zero-displacement move before the LL
                // hook. Switch to a zero-delta wheel during setup; neither probe
                // moves the cursor, scrolls content, or activates a window.
                if context.mouse.is_none() && self.started.elapsed() >= Duration::from_millis(250) {
                    self.mode = MouseProbe::ZeroWheel;
                }
            }
        });
        if !self.pending {
            self.request
                .try_send(self.mode)
                .map_err(|e| format!("hook probe submission: {e}"))?;
            self.pending = true;
        }
        Ok(())
    }
}
fn send(events: &GuardEvents, stage: &'static str, input: INPUT) -> (u32, u32, u128) {
    events.startup_progress(stage, "SendInput pending; Win32 result unobserved");
    let started = Instant::now();
    // SAFETY: initialized single up/zero-motion record, no borrowed pointers.
    let sent = unsafe {
        SetLastError(ERROR_SUCCESS);
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32)
    };
    // SAFETY: immediately capture this exact SendInput's error, never infer UIPI.
    let code = unsafe { GetLastError() }.0;
    let elapsed = started.elapsed().as_millis();
    events.startup_progress(
        stage,
        format!("sent={sent}/1; Win32={code}; elapsed_ms={elapsed}; UIPI cause not inferred"),
    );
    (sent, code, elapsed)
}
