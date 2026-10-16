//! One bounded sender, independent of the thread that must service LL hooks.
use super::*;
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
    override_mode: Option<MouseProbe>,
    attempted: std::collections::BTreeSet<MouseProbe>,
}
impl Sender {
    pub(super) fn start(events: Arc<GuardEvents>) -> Result<Self, String> {
        events.startup_step("hook_probe_sender", STARTUP_STEP_TIMEOUT);
        let override_mode = configured_mode()?;
        let (request, receive) = mpsc::sync_channel(1);
        let (reply, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-hook-probes".into())
            .spawn(move || {
                while let Ok(mode) = receive.recv() {
                    let keyboard = send(
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
                    );
                    let mouse = match super::mouse::inputs(mode, PROBE_TAG) {
                        Ok(inputs) => {
                            let result = send(&events, "hook_mouse_dispatch", &inputs);
                            if mode == MouseProbe::XButtonPair && result.0 == 1 {
                                // Partial XDOWN must have one matching XUP even
                                // on this explicitly selected disposable CI path.
                                let released =
                                    send(&events, "hook_mouse_partial_release", &inputs[1..]);
                                if released.0 != 1 {
                                    events.fail("partial mouse probe button release failed");
                                }
                            }
                            result
                        }
                        Err(reason) => {
                            events.fail(reason);
                            break;
                        }
                    };
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
            override_mode,
            attempted: Default::default(),
        })
    }
    pub(super) fn poll(&mut self) -> Result<(), String> {
        match self.reply.try_recv() {
            Ok(report) => {
                self.pending = false;
                let success = report.keyboard.0 == 1 && report.mouse.0 == report.mode.requested();
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
                context.events.startup_progress("mouse_hook_proof", format!(
                    "{} (attempted {} probe kinds)",
                    if context.mouse.is_some() { "mouse hook liveness proven by tagged injected callback" }
                    else { "mouse hook liveness could not be proven" }, self.attempted.len()
                ));
                context.events.startup_progress("hook_SendInput", format!(
                    "hooks_install=success; dispatch={:?}; pending={}; keyboard_seen={}; mouse_seen={}; keyboard_probe_flags={:?}; mouse_probe_flags={:?}; last_keyboard_flags={:?}; last_mouse_flags={:?}; mouse_proof={}; attempted_mouse_probe_kinds={}",
                    self.report.as_ref().map(|r| (r.mode, r.keyboard, r.mouse)), self.pending, context.keyboard.is_some(), context.mouse.is_some(),
                    context.keyboard_probe_flags, context.mouse_probe_flags,
                    context.last_keyboard_flags, context.last_mouse_flags,
                    if context.mouse.is_some() { "injected tagged callback observed" }
                    else if context.mouse_probe_flags.is_some() { "tagged callback observed but injected flags missing" }
                    else if context.last_mouse_flags.is_some() { "hook received other events; tagged probe unobserved (filter/UIPI not proven)" }
                    else { "mouse hook liveness could not be proven; no mouse callback observed; dispatch/pump/filter cause unproven" },
                    self.attempted.len(),
                ));
                // Defaults never move the cursor or press a mouse button. Native
                // acceptance is not a proof: only the tagged injected callback is.
                if context.mouse.is_none() {
                    self.mode = self.override_mode.unwrap_or_else(|| mode_at(self.started.elapsed()));
                }
            }
        });
        if !self.pending {
            self.request
                .try_send(self.mode)
                .map_err(|e| format!("hook probe submission: {e}"))?;
            self.attempted.insert(self.mode);
            self.pending = true;
        }
        Ok(())
    }
}
fn send(events: &GuardEvents, stage: &'static str, inputs: &[INPUT]) -> (u32, u32, u128) {
    events.startup_progress(stage, "SendInput pending; Win32 result unobserved");
    let started = Instant::now();
    // SAFETY: initialized up/zero-motion or explicitly opted-in mouse records.
    let sent = unsafe {
        SetLastError(ERROR_SUCCESS);
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32)
    };
    // SAFETY: immediately capture this exact SendInput's error, never infer UIPI.
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

fn configured_mode() -> Result<Option<MouseProbe>, String> {
    // Release/default builds never select motion/buttons. The contract harness
    // selects a candidate only after its same-process empirical matrix saw a
    // callback. Selection never replaces the tagged injected liveness proof.
    #[cfg(all(feature = "test-worker-faults", debug_assertions))]
    if let Some(mode) = std::env::var_os("GCU_HOOK_MOUSE_PROBE") {
        return match mode.to_str() {
            Some("zero_move") => Ok(Some(MouseProbe::ZeroMove)),
            Some("zero_wheel") => Ok(Some(MouseProbe::ZeroWheel)),
            Some("zero_hwheel") => Ok(Some(MouseProbe::ZeroHWheel)),
            Some("absolute_same_position") => Ok(Some(MouseProbe::AbsoluteSamePosition)),
            Some("relative_pair") => Ok(Some(MouseProbe::RelativePair)),
            Some("xbutton_pair") => Ok(Some(MouseProbe::XButtonPair)),
            _ => Err("unsupported debug hook mouse probe; foreground disabled".into()),
        };
    }
    Ok(None)
}
