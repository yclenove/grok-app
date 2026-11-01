//! Direct counterexamples from the S11a round-1 review.

use std::net::Ipv4Addr;
use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, BoundedOutput, CallError, CloneDeadline, CloneId, FakeClock, FakeGuestChannel,
    FakeLedger, FakeTartRunner, GoldenImage, GuestChannel, HostClock, PolicyError, QueryDeadline,
    RunId, ScriptOutcome, ScriptedReply, SoftnetFlags, TartRunner,
};

use super::{must_err, noted, reserve_dyn};

pub(super) fn clone_at_absolute_deadline_expires_without_the_script() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"queued-clone".to_vec())));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(1)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        ok!(RunId::new("run-bound")),
        ok!(CloneId::new("clone-bound")),
        absolute,
        &clock,
    ))?;
    let absolute_at = permit.absolute_deadline();
    ok!(clock.advance(Duration::from_secs(1)));
    if clock.now() != absolute_at {
        return Err("clock is not exactly on the absolute deadline".into());
    }
    let call = ok!(CloneDeadline::after(clock.now(), CloneDeadline::CAP));
    if call.at() <= absolute_at {
        return Err("call deadline did not outlast the absolute deadline".into());
    }
    {
        let runner: &mut dyn TartRunner = &mut runner;
        must_err(
            runner.clone_vm(permit, GoldenImage::MacosS2Eval, call),
            CallError::Expired,
            "boundary clone",
        )?;
    }
    let clone = ok!(CloneId::new("clone-bound"));
    let body = ok!(runner.status(
        &clone,
        ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
    ));
    if body.as_bytes() != b"queued-clone" {
        return Err("boundary expiry consumed the script".into());
    }
    let trace = runner.trace();
    if trace.len() != 2 {
        return Err(format!("boundary trace len {}", trace.len()));
    }
    if trace[0].outcome != ScriptOutcome::Expired || trace[0].deadline != absolute_at {
        return Err(format!(
            "boundary trace recorded {:?} at {:?}",
            trace[0].outcome, trace[0].deadline
        ));
    }
    if trace[0].deadline == call.at() {
        return Err("trace recorded the longer call deadline".into());
    }
    Ok(())
}

pub(super) fn clone_shorter_parent_deadline_is_recorded() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"cloned".to_vec())));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(1)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        ok!(RunId::new("run-short")),
        ok!(CloneId::new("clone-short")),
        absolute,
        &clock,
    ))?;
    let call = ok!(CloneDeadline::after(clock.now(), CloneDeadline::CAP));
    if permit.absolute_deadline() >= call.at() {
        return Err("parent deadline was not shorter than the call".into());
    }
    let absolute_at = permit.absolute_deadline();
    let body = ok!(runner.clone_vm(permit, GoldenImage::MacosS2Eval, call));
    if body.as_bytes() != b"cloned" {
        return Err("a live shorter parent deadline rejected the clone".into());
    }
    let entry = runner
        .trace()
        .first()
        .ok_or("shorter parent deadline left no trace")?;
    if entry.outcome != ScriptOutcome::Success || entry.deadline != absolute_at {
        return Err(format!(
            "effective deadline {:?} outcome {:?}",
            entry.deadline, entry.outcome
        ));
    }
    if entry.deadline == call.at() {
        return Err("trace kept the longer call deadline".into());
    }
    Ok(())
}

pub(super) fn clone_shorter_call_deadline_is_recorded() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"cloned".to_vec())));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(90)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        ok!(RunId::new("run-call-short")),
        ok!(CloneId::new("clone-call-short")),
        absolute,
        &clock,
    ))?;
    let permit_at = permit.absolute_deadline();
    let call = ok!(CloneDeadline::after(clock.now(), Duration::from_secs(30)));
    if call.at() >= permit_at {
        return Err("call deadline was not shorter than the permit".into());
    }
    let call_at = call.at();
    let body = ok!(runner.clone_vm(permit, GoldenImage::MacosS2Eval, call));
    if body.as_bytes() != b"cloned" {
        return Err("a live shorter call deadline rejected the clone".into());
    }
    let entry = runner
        .trace()
        .first()
        .ok_or("shorter call deadline left no trace")?;
    if entry.outcome != ScriptOutcome::Success || entry.deadline != call_at {
        return Err(format!(
            "effective deadline {:?} outcome {:?}",
            entry.deadline, entry.outcome
        ));
    }
    if entry.deadline == permit_at {
        return Err("trace kept the longer permit deadline".into());
    }
    Ok(())
}

pub(super) fn clone_at_call_deadline_expires_while_permit_is_live() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut ledger = FakeLedger::new(clock.clone());
    let mut runner = FakeTartRunner::new(clock.clone());
    runner.push(ok!(ScriptedReply::success(b"queued-call".to_vec())));
    let absolute = ok!(AttemptDeadline::after(clock.now(), Duration::from_secs(90)));
    let permit = noted(reserve_dyn(
        &mut ledger,
        ok!(RunId::new("run-call-edge")),
        ok!(CloneId::new("clone-call-edge")),
        absolute,
        &clock,
    ))?;
    let permit_at = permit.absolute_deadline();
    let call = ok!(CloneDeadline::after(clock.now(), Duration::from_secs(1)));
    let call_at = call.at();
    if call_at >= permit_at {
        return Err("call deadline did not stay inside the permit".into());
    }
    ok!(clock.advance(Duration::from_secs(1)));
    if clock.now() != call_at {
        return Err("clock is not exactly on the call deadline".into());
    }
    if clock.now() >= permit_at {
        return Err("permit was not still live at the call deadline".into());
    }
    {
        let runner: &mut dyn TartRunner = &mut runner;
        must_err(
            runner.clone_vm(permit, GoldenImage::MacosS2Eval, call),
            CallError::Expired,
            "call boundary",
        )?;
    }
    let clone = ok!(CloneId::new("clone-call-edge"));
    let body = ok!(runner.status(
        &clone,
        ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP)),
    ));
    if body.as_bytes() != b"queued-call" {
        return Err("call-boundary expiry consumed the queued reply".into());
    }
    let trace = runner.trace();
    if trace.len() != 2 {
        return Err(format!("call-boundary trace len {}", trace.len()));
    }
    if trace[0].outcome != ScriptOutcome::Expired || trace[0].deadline != call_at {
        return Err(format!(
            "call-boundary trace recorded {:?} at {:?}",
            trace[0].outcome, trace[0].deadline
        ));
    }
    if trace[0].deadline == permit_at {
        return Err("trace recorded the longer permit deadline".into());
    }
    Ok(())
}

pub(super) fn expired_guest_call_keeps_the_queued_reply() -> Result<(), String> {
    let clock = FakeClock::new();
    let mut guest = FakeGuestChannel::new(clock.clone());
    guest.push(ok!(ScriptedReply::success(b"guest-kept".to_vec())));
    let stale = ok!(QueryDeadline::after(clock.now(), Duration::from_secs(1)));
    ok!(clock.advance(Duration::from_secs(1)));
    if clock.now() != stale.at() {
        return Err("guest clock is not exactly on the deadline".into());
    }
    {
        let guest: &mut dyn GuestChannel = &mut guest;
        must_err(guest.inventory(stale), CallError::Expired, "guest expired")?;
    }
    let body = ok!(guest.inventory(ok!(QueryDeadline::after(clock.now(), QueryDeadline::CAP))));
    if body.as_bytes() != b"guest-kept" {
        return Err("expired guest call consumed the queued reply".into());
    }
    let outcomes: Vec<ScriptOutcome> = guest.trace().iter().map(|entry| entry.outcome).collect();
    if outcomes != [ScriptOutcome::Expired, ScriptOutcome::Success] {
        return Err(format!("guest expiry trace: {outcomes:?}"));
    }
    Ok(())
}

pub(super) fn bounded_output_debug_hides_text_and_bytes() -> Result<(), String> {
    let text = "tart-debug-canary-7f3a9c";
    let bytes = text.as_bytes();
    let output = ok!(BoundedOutput::try_new(bytes.to_vec()));
    let rendered = format!("{output:?}");
    let failed = format!("{:?}", CallError::Failed(output.clone()));
    let byte_debug = format!("{bytes:?}");
    let prefix_debug = format!("{:?}", &bytes[..8]);
    for label in [&rendered, &failed] {
        if label.contains(text) {
            return Err(format!("debug contained payload text: {label}"));
        }
        if label.contains(&byte_debug) || label.contains(&prefix_debug) {
            return Err(format!("debug contained payload bytes: {label}"));
        }
    }
    if !rendered.contains(&output.len().to_string()) {
        return Err(format!("debug dropped the length: {rendered}"));
    }
    Ok(())
}

pub(super) fn softnet_rejects_special_use_at_prefix_boundaries() -> Result<(), String> {
    let rejected: &[[u8; 4]] = &[
        [0, 0, 0, 0],
        [0, 255, 255, 255],
        [10, 0, 0, 0],
        [10, 255, 255, 255],
        [100, 64, 0, 0],
        [100, 127, 255, 255],
        [127, 0, 0, 0],
        [127, 255, 255, 255],
        [169, 254, 0, 0],
        [169, 254, 255, 255],
        [172, 16, 0, 0],
        [172, 31, 255, 255],
        [192, 0, 0, 0],
        [192, 0, 0, 1],
        [192, 0, 0, 8],
        [192, 0, 0, 170],
        [192, 0, 0, 171],
        [192, 0, 0, 255],
        [192, 0, 2, 0],
        [192, 0, 2, 255],
        [192, 88, 99, 0],
        [192, 88, 99, 255],
        [192, 168, 0, 0],
        [192, 168, 255, 255],
        [198, 18, 0, 0],
        [198, 19, 255, 255],
        [198, 51, 100, 0],
        [198, 51, 100, 255],
        [203, 0, 113, 0],
        [203, 0, 113, 255],
        [224, 0, 0, 0],
        [239, 255, 255, 255],
        [240, 0, 0, 0],
        [248, 0, 0, 0],
        [255, 255, 255, 254],
        [255, 255, 255, 255],
    ];
    for octets in rejected {
        let addr = Ipv4Addr::from(*octets);
        must_err(
            SoftnetFlags::try_new(&[addr]),
            PolicyError::NotPublic { addr },
            "special-use",
        )?;
    }
    let accepted: &[[u8; 4]] = &[
        [1, 0, 0, 0],
        [9, 255, 255, 255],
        [11, 0, 0, 0],
        [100, 63, 255, 255],
        [100, 128, 0, 0],
        [126, 255, 255, 255],
        [128, 0, 0, 0],
        [169, 253, 255, 255],
        [169, 255, 0, 0],
        [172, 15, 255, 255],
        [172, 32, 0, 0],
        [191, 255, 255, 255],
        [192, 0, 1, 0],
        [192, 0, 1, 255],
        [192, 0, 3, 0],
        [192, 88, 98, 255],
        [192, 88, 100, 0],
        [192, 167, 255, 255],
        [192, 169, 0, 0],
        [198, 17, 255, 255],
        [198, 20, 0, 0],
        [198, 51, 99, 255],
        [198, 51, 101, 0],
        [203, 0, 112, 255],
        [203, 0, 114, 0],
        [223, 255, 255, 255],
    ];
    for octets in accepted {
        let addr = Ipv4Addr::from(*octets);
        let flags = ok!(SoftnetFlags::try_new(&[addr]));
        if flags.allow() != [addr] {
            return Err(format!("boundary {addr} was not stored"));
        }
    }
    Ok(())
}
