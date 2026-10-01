//! Explicit native-fixture fault injection. Not compiled into product builds,
//! reachable from model arguments, or applied to ordinary App WebViews.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Phase {
    FrameReply,
    ContextEvent,
    CreateReply,
    ProcessIdentityReply,
    EvaluationReply,
}

pub(crate) struct Fault {
    pub phase: Phase,
    pub observed: Sender<()>,
}

pub(crate) fn dispatch(
    webview: ICoreWebView2,
    world: Arc<NativeWorld>,
    generation: u64,
    operation: ScriptOperation,
    current: impl Fn() -> bool + 'static,
    reply: Sender<Result<String, String>>,
    fault: Fault,
) {
    let job = Job::new(
        webview,
        world,
        generation,
        operation,
        current,
        "(() => { const el = document.getElementById('effects'); el.textContent = String(Number(el.textContent) + 1); return {ran: true}; })()".into(),
        reply,
    );
    *job.fault.borrow_mut() = Some(fault);
    job.begin();
}

pub(super) fn suppress(job: &Rc<Job>, phase: Phase) -> bool {
    let mut fault = job.fault.borrow_mut();
    if fault.as_ref().is_some_and(|fault| fault.phase == phase) {
        let fault = fault.take().expect("phase was matched");
        if phase == Phase::EvaluationReply {
            if let Some(witness) = job.world.probe_witness(job.ticket) {
                eprintln!(
                    "renderer-witness: held_before_crash {}",
                    witness.diagnostic()
                );
                // Read-only fault diagnostics. This observer cannot retire a
                // document, finish a ticket, replay an action or change a gate.
                let _ = std::thread::Builder::new()
                    .name("grok-cu-probe-exit".into())
                    .spawn(move || {
                        let started = std::time::Instant::now();
                        while !witness.exited()
                            && started.elapsed() < std::time::Duration::from_secs(20)
                        {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                        }
                        eprintln!(
                            "renderer-witness: independent_exit_observation elapsed_ms={} {}",
                            started.elapsed().as_millis(),
                            witness.diagnostic()
                        );
                    });
            }
        }
        let _ = fault.observed.send(());
        true
    } else {
        false
    }
}
