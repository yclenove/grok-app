//! One bounded sender, independent of the thread that must service LL hooks.
use super::*;
use hook_probe::{Dispatch, MouseProbeApi, ProbeIdle, ProbeRequest, RecentInput};
use windows::Win32::System::SystemInformation::GetTickCount64;

#[derive(Debug)]
struct Report {
    generation: u64,
    keyboard: Dispatch,
    mouse: Result<Dispatch, String>,
    mode: MouseProbe,
    dispatched: bool,
}
struct Job {
    request: ProbeRequest,
    generation: u64,
    action: bool,
}
pub(super) struct Sender {
    request: mpsc::SyncSender<Job>,
    reply: mpsc::Receiver<Report>,
    pending: bool,
    /// When the in-flight job was submitted, to trace a sender that does not come back.
    pending_since: Option<Instant>,
    last_pending_trace: Option<Instant>,
    last_decision_trace: Option<Instant>,
    report: Option<Report>,
    generation: u64,
    initial_finished: bool,
    attempted: std::collections::BTreeSet<MouseProbe>,
}
impl Sender {
    pub(super) fn start(events: Arc<GuardEvents>, idle: Arc<ProbeIdle>) -> Result<Self, String> {
        events.startup_step("hook_probe_sender", STARTUP_STEP_TIMEOUT);
        let (request, receive) = mpsc::sync_channel::<Job>(1);
        let (reply, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("gcu-hook-probes".into())
            .spawn(move || {
                while let Ok(job) = receive.recv() {
                    let request = job.request;
                    let mut api = NativeMouse {
                        events: &events,
                        tag: request.tag,
                        dpi: None,
                        dispatched: false,
                        generation: job.generation,
                        action: job.action,
                        coalesced: false,
                        packet: None,
                    };
                    let began = Instant::now();
                    let mouse = if job.action {
                        hook_probe::dispatch_for_action(&mut api, request.mode, &idle)
                    } else {
                        hook_probe::dispatch(&mut api, request.mode, &idle)
                    };
                    // Metadata only: a long cycle (gate wait + SendInput + hook callbacks) is the
                    // other way a lease can lapse while dispatches look regular.
                    let cycle = began.elapsed();
                    if cycle > std::time::Duration::from_millis(25) {
                        events
                            .probe_trace("probe_cycle", format!("cycle_ms={}", cycle.as_millis()));
                    }
                    let dispatched = api.dispatched;
                    // A refused or changed-cursor probe never submits extra input.
                    // The initial idle sample preceded all probe input, including
                    // our key-up. Subsequent own injections preserve that marker.
                    let keyboard = if api.coalesced {
                        api.keyboard_dispatch()
                    } else if mouse
                        .as_ref()
                        .is_ok_and(|r| r.0 == request.mode.requested())
                        && api.authorized()
                    {
                        send(
                            &events,
                            "hook_keyboard_dispatch",
                            &[keyboard_up(request.tag)],
                            "keyup",
                        )
                    } else {
                        (0, 0, 0)
                    };
                    if reply
                        .send(Report {
                            generation: job.generation,
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
            pending_since: None,
            last_pending_trace: None,
            last_decision_trace: None,
            report: None,
            attempted: Default::default(),
            generation: 0,
            initial_finished: false,
        })
    }
    pub(super) fn poll(&mut self) -> Result<(), String> {
        match self.reply.try_recv() {
            Ok(report) => {
                self.pending = false;
                self.pending_since = None;
                self.last_pending_trace = None;
                CONTEXT.with(|slot| {
                    if let Some(context) = slot.borrow().as_ref() {
                        context.events.probe_dispatch(false);
                        // Metadata only: why a probe cycle was refused (no coordinates or payload).
                        if let Err(reason) = &report.mouse {
                            context
                                .events
                                .probe_trace("hook_probe_refused", reason.clone());
                        }
                    }
                });
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
                if let (Ok(_), Err(reason)) = (&report.mouse, &success) {
                    CONTEXT.with(|slot| {
                        if let Some(context) = slot.borrow().as_ref() {
                            context.events.probe_trace(
                                "hook_probe_refused",
                                reason.chars().take(200).collect::<String>(),
                            );
                        }
                    });
                }
                let generation = report.generation;
                self.report = Some(report);
                CONTEXT.with(|slot| {
                    if let Some(context) = slot.borrow_mut().as_mut() {
                        let floor = context.events.accepted_cycle();
                        // A late initial report must not complete or refuse the action window.
                        if generation < floor {
                            self.initial_finished = true;
                        } else if context.events.probe_window().0 == generation {
                            if let Err(reason) = context.mouse_proof.complete(success) {
                                context.events.refuse_proof(reason);
                                self.initial_finished = true;
                            }
                        }
                    }
                    Ok::<_, String>(())
                })?;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Metadata only: a job that has not come back is the signature of a stuck SendInput.
                if let Some(since) = self.pending_since {
                    let waited = since.elapsed();
                    if waited > std::time::Duration::from_millis(30)
                        && self
                            .last_pending_trace
                            .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(30))
                    {
                        self.last_pending_trace = Some(Instant::now());
                        CONTEXT.with(|slot| {
                            if let Some(context) = slot.borrow().as_ref() {
                                context.events.probe_trace(
                                    "probe_pending_long",
                                    format!("pending_ms={}", waited.as_millis()),
                                );
                            }
                        });
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => return Err("hook probe sender exited".into()),
        }
        CONTEXT.with(|slot| {
            if let Some(context) = slot.borrow_mut().as_mut() {
                let (generation, _, floor) = context.events.admission_window();
                adopt(self, context, generation, floor);
                // The first read can precede begin_foreground. Adopt the generation it raised.
                let (generation, action, floor) = context.events.admission_window();
                adopt(self, context, generation, floor);
                let mouse = context.mouse_proof.proven();
                let keyboard = hook_probe::keyboard_stamp_for_mouse(
                    context.mouse_proof.proven_tag(),
                    context.keyboard,
                );
                // Stored stamps belong to self.generation. A retired cycle is not a removal.
                // A keyboard stamp from the next attempt is not paired with the previous mouse.
                if self.generation >= floor
                    && generation == self.generation
                    && (action || generation == 0)
                {
                    context.events.confirm_hook_proofs(
                        self.generation,
                        keyboard,
                        mouse,
                    )?;
                    if context.events.available() && !action {
                        self.initial_finished = true;
                    }
                }
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
                let dispatch = if action {
                    crate::input::hotkey::should_dispatch_action_probe(
                        self.pending,
                        mouse.is_some(),
                        context.events.injecting(),
                    )
                } else {
                    !self.pending && generation == 0 && !self.initial_finished
                };
                // Metadata only: why no new probe is going out while an injection is in flight.
                if context.events.injecting()
                    && !self.pending
                    && (!action || !dispatch || !context.events.probes_permitted())
                    && self
                        .last_decision_trace
                        .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(20))
                {
                    self.last_decision_trace = Some(Instant::now());
                    context.events.probe_trace(
                        "probe_not_dispatched",
                        format!(
                            "action={action}; dispatch={dispatch}; permitted={}; mouse_proven={}",
                            context.events.probes_permitted(),
                            mouse.is_some()
                        ),
                    );
                }
                if dispatch && context.events.probes_permitted() {
                    if let Some(request) = context.mouse_proof.next() {
                        context.keyboard = None;
                        if action && mouse.is_some() {
                            let age_ms = context
                                .events
                                .clock()
                                .lease_age()
                                .map(|age| age.as_millis())
                                .unwrap_or(0);
                            context
                                .events
                                .probe_trace("hook_probe_renew", format!("lease_age_ms={age_ms}"));
                        }
                        context.events.probe_dispatch(true);
                        self.request.try_send(Job { request, generation, action })
                            .map_err(|e| format!("hook probe submission: {e}"))?;
                        self.pending = true;
                        self.pending_since = Some(Instant::now());
                    } else if !action {
                        self.initial_finished = true;
                    } else if self
                        .last_decision_trace
                        .is_none_or(|at| at.elapsed() > std::time::Duration::from_millis(20))
                    {
                        self.last_decision_trace = Some(Instant::now());
                        context
                            .events
                            .probe_trace("probe_next_none", context.mouse_proof.debug_state());
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
    generation: u64,
    action: bool,
    /// Action probes carry the F24 key-up in the same SendInput as the mouse.
    coalesced: bool,
    packet: Option<(Dispatch, u32)>,
}
fn adopt(sender: &mut Sender, context: &mut Context, generation: u64, floor: u64) {
    if sender.pending || (sender.generation == generation && sender.generation >= floor) {
        return;
    }
    if sender.generation < floor {
        sender.initial_finished = true;
    }
    sender.generation = generation;
    context.mouse_proof.begin_cycle(true);
    context.keyboard = None;
}
fn keyboard_up(tag: usize) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_F24,
                dwFlags: KEYEVENTF_KEYUP,
                dwExtraInfo: tag,
                ..Default::default()
            },
        },
    }
}
impl NativeMouse<'_> {
    fn keyboard_dispatch(&self) -> Dispatch {
        let Some((result, mouse_count)) = self.packet else {
            return (0, 0, 0);
        };
        let sent = u32::from(result.0 > mouse_count);
        (sent, result.1, result.2)
    }
    fn authorized(&self) -> bool {
        let (generation, active, floor) = self.events.admission_window();
        generation == self.generation
            && self.generation >= floor
            && (!self.action || active)
            && self.events.probes_permitted()
    }
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
        if !self.authorized() {
            return Err("probe window ended before dispatch".into());
        }
        let mut inputs = match absolute {
            Some(at) => mouse::absolute(self.tag, at),
            None => mouse::inputs(mode, self.tag)?,
        };
        let mouse_count = inputs.len() as u32;
        // One packet: the hook pump delivers both callbacks before SendInput
        // returns, instead of sleeping another POLL_INTERVAL between them.
        if self.action && mouse_count == 1 && self.authorized() {
            inputs.push(keyboard_up(self.tag));
            self.coalesced = true;
        }
        self.dispatched = true;
        let packet = if self.coalesced {
            "mouse+keyup"
        } else {
            "mouse"
        };
        let result = send(self.events, "hook_mouse_dispatch", &inputs, packet);
        self.packet = Some((result, mouse_count));
        let result = (result.0.min(mouse_count), result.1, result.2);
        #[cfg(any(test, all(feature = "test-worker-faults", debug_assertions)))]
        if mode == MouseProbe::XButtonPair && result.0 == 1 {
            // Partial debug-only XDOWN needs exactly one matching XUP.
            if send(
                self.events,
                "hook_mouse_partial_release",
                &inputs[1..],
                "release",
            )
            .0 != 1
            {
                return Err("partial mouse probe button release failed".into());
            }
        }
        Ok(result)
    }
}
fn send(events: &GuardEvents, stage: &'static str, inputs: &[INPUT], packet: &str) -> Dispatch {
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
    // Visible after arm. Counts and duration only; no coordinates or key payload.
    events.probe_trace(
        stage,
        format!(
            "call_ms={elapsed}; sent={sent}/{}; packet={packet}",
            inputs.len()
        ),
    );
    (sent, code, elapsed)
}
