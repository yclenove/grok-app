//! M4b-1 regressions. Each case is one public step of at most 100ms.
//!
//! Costs are charged into the shared fake clock. Nothing here sleeps.

use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptOutcome, CloneDeadline, CommandKind, CompletedStep, DeleteDeadline,
    DispatchState, GoldenImage, GroupDeadline, GuestCommand, HostClock, LedgerFact, LedgerOp,
    LoginDeadline, MonoInstant, NotRunReason, Phase, PhaseBudgets, ReapFault, ReplyState,
    RevokeDeadline, RunDeadline, RuntimeLoss, ScriptOutcome, SealDeadline, StageDeadline,
    StartDeadline, StepError, StepProgress, StopDeadline, TartCommand, VmIdentity,
};
use gcu_eval::disposable::CleanupDisposition;
use gcu_eval::disposable::Probe;

use super::faults::{expect_not_run, expect_quarantine};
use super::harness::{
    self, advance, cleanup_deadlines, drive_to, finish, id_clone, id_run, ok_body, push_clear,
    push_guest, push_isolate, push_present, query, reserve_default, scripts_to_reach, Pump, CFG,
    CLONE, CLONE_2, INC, RUN, RUN_2,
};

#[allow(
    clippy::type_complexity,
    reason = "one table of named checks; a type alias would not make the pointer simpler"
)]
pub(super) const TESTS: &[(&str, fn() -> Result<(), String>)] = &[
    (
        "slow_query_and_probes_one_slice_keep_original_ends",
        slow_query_and_probes_one_slice_keep_original_ends,
    ),
    (
        "stop_handle_pending_four_seconds_then_reap_stop_stays_latched",
        stop_handle_pending_four_seconds_then_reap_stop_stays_latched,
    ),
    (
        "r2_2_owner_end_during_destroy_and_reap_does_not_revive",
        r2_2_owner_end_during_destroy_and_reap_does_not_revive,
    ),
    (
        "r2_3_stop_during_lock_ledger_io_does_not_adopt_or_skip_delete",
        r2_3_stop_during_lock_ledger_io_does_not_adopt_or_skip_delete,
    ),
    (
        "r2_4_lock_yield_slow_ops_shared_step_begin_once",
        r2_4_lock_yield_slow_ops_shared_step_begin_once,
    ),
    (
        "r2_5_bootstrap_login_run_unknown_survives_cleanup",
        r2_5_bootstrap_login_run_unknown_survives_cleanup,
    ),
    (
        "r2_6_kill_reap_pending_frozen_end_and_strict_exit",
        r2_6_kill_reap_pending_frozen_end_and_strict_exit,
    ),
    (
        "d3_early_cancel_reaps_in_one_point_two_seconds",
        d3_early_cancel_reaps_in_one_point_two_seconds,
    ),
    (
        "d3_cancel_at_work_end_is_unresolved_or_within_op_end",
        d3_cancel_at_work_end_is_unresolved_or_within_op_end,
    ),
    (
        "d3_cleanup_cancel_stays_inside_phase_budgets",
        d3_cleanup_cancel_stays_inside_phase_budgets,
    ),
    (
        "d4_seal_and_recovery_deliver_once_via_poll",
        d4_seal_and_recovery_deliver_once_via_poll,
    ),
    (
        "d4_permit_begins_once_across_entry_and_poll",
        d4_permit_begins_once_across_entry_and_poll,
    ),
    (
        "d5_public_path_pending_accepted_then_cleanup",
        d5_public_path_pending_accepted_then_cleanup,
    ),
    (
        "r2_7_two_row_recovery_start_issued_does_not_inherit",
        r2_7_two_row_recovery_start_issued_does_not_inherit,
    ),
    (
        "cooperative_steps_within_one_hundred_ms",
        cooperative_steps_within_one_hundred_ms,
    ),
    (
        "injected_overrun_does_not_adopt",
        injected_overrun_does_not_adopt,
    ),
    (
        "host_stop_constants_unchanged",
        host_stop_constants_unchanged,
    ),
];

fn renew(io: &harness::Handles) -> Result<(), String> {
    io.owner
        .borrow_mut()
        .push_proof(RUN, io.clock.now())
        .map_err(|err| format!("proof: {err:?}"))
}

fn spans_ok(attempt: &harness::Machine<'_>) -> Result<(), String> {
    for span in attempt.step_spans() {
        if span.elapsed() > Duration::from_millis(100) {
            return Err(format!("step took {}ms", span.elapsed().as_millis()));
        }
    }
    Ok(())
}

fn pump(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
    caller: MonoInstant,
    first: Result<StepProgress<CompletedStep>, StepError>,
) -> Result<CompletedStep, String> {
    let mut progress = first.map_err(|err| format!("{err:?}"))?;
    for _ in 0..500 {
        match progress {
            StepProgress::Done(done) => return Ok(done),
            StepProgress::Pending => {
                renew(io)?;
                progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
            }
        }
    }
    Err("step limit".into())
}

fn runner_cost(io: &harness::Handles, millis: u64) {
    io.runner
        .borrow_mut()
        .set_io_cost(Duration::from_millis(millis));
}

fn guest_cost(io: &harness::Handles, millis: u64) {
    io.guest
        .borrow_mut()
        .set_io_cost(Duration::from_millis(millis));
}

fn kind_count(io: &harness::Handles, kind: CommandKind) -> usize {
    io.runner
        .borrow()
        .trace()
        .iter()
        .filter(|entry| matches_kind(&entry.command, kind))
        .count()
}

fn matches_kind(command: &TartCommand, kind: CommandKind) -> bool {
    matches!(
        (command, kind),
        (TartCommand::Clone { .. }, CommandKind::Clone)
            | (TartCommand::Start { .. }, CommandKind::Start)
            | (TartCommand::Stop { .. }, CommandKind::Stop)
            | (TartCommand::Delete { .. }, CommandKind::Delete)
            | (TartCommand::List, CommandKind::List)
            | (TartCommand::Status { .. }, CommandKind::Status)
    )
}

fn seal_count(io: &harness::Handles) -> usize {
    io.ledger
        .borrow()
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, LedgerOp::Seal))
        .count()
}

fn reach<'a>(
    world: &'a mut harness::World,
    io: &harness::Handles,
    phase: Phase,
) -> Result<harness::Machine<'a>, String> {
    scripts_to_reach(io, phase);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, io, phase)?;
    Ok(attempt)
}

fn deadlines_at(
    io: &harness::Handles,
) -> (gcu_eval::disposable::tart::CleanupDeadlines, MonoInstant) {
    let deadlines = cleanup_deadlines(&io.clock);
    let caller = deadlines.query.at();
    (deadlines, caller)
}

fn slow_query_and_probes_one_slice_keep_original_ends() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Booting);
    runner_cost(&io, 180);
    let mut attempt = world.attempt();
    let deadline = query(&io.clock);
    let caller = deadline.at();
    renew(&io)?;
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    let first = attempt.reserve(
        gcu_eval::disposable::tart::ReserveInput {
            run: id_run(RUN),
            clone_id: id_clone(CLONE),
            absolute,
            manifest: harness::manifest(),
            approved: harness::manifest(),
            stop_latched: false,
            owner_present: true,
        },
        deadline,
    );
    let _ = pump(&mut attempt, &io, caller, first)?;
    let lists: Vec<_> = io
        .runner
        .borrow()
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, TartCommand::List))
        .map(|entry| entry.deadline)
        .collect();
    if lists.len() != 1 {
        return Err(format!("list traces {lists:?}"));
    }
    spans_ok(&attempt)?;
    if io.clock.now().saturating_duration_since(MonoInstant::EPOCH) < Duration::from_millis(180) {
        return Err("list cost was not charged".into());
    }
    runner_cost(&io, 0);
    drive_to(&mut attempt, &io, Phase::Booting)?;
    guest_cost(&io, 30);
    push_guest(&io, ok_body("ok"));
    push_isolate(&io, "passed");
    io.owner.borrow_mut().push_priority_lease();
    renew(&io)?;
    let group = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
    attempt
        .isolate_wait(group)
        .map_err(|err| format!("isolate: {err:?}"))?;
    let guest = io.guest.borrow();
    let probes: Vec<_> = guest
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, GuestCommand::Probe(_)))
        .map(|entry| entry.deadline)
        .collect();
    let inventories: Vec<_> = guest
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, GuestCommand::Inventory))
        .map(|entry| entry.deadline)
        .collect();
    let leases = guest
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, GuestCommand::Lease { .. }))
        .count();
    drop(guest);
    // Each probe freezes its own end at begin: min(group caller, begin + probe cap).
    // A 30ms slice therefore steps the next end by 30ms. A later poll must not
    // extend an end, and no end may pass the isolate caller.
    let slice = Duration::from_millis(30);
    let stepped = probes.len() == Probe::ALL.len()
        && probes
            .windows(2)
            .all(|pair| pair[1].saturating_duration_since(pair[0]) == slice)
        && probes.iter().all(|deadline| *deadline <= group.at());
    if !stepped {
        return Err(format!("probe ends {probes:?}"));
    }
    if inventories.len() != 1 {
        return Err(format!("inventory traces {inventories:?}"));
    }
    if leases != 1 {
        return Err(format!("lease count {leases}"));
    }
    spans_ok(&attempt)?;
    if attempt.phase() != Phase::Isolated {
        return Err(format!("phase {:?}", attempt.phase()));
    }
    Ok(())
}

fn stop_handle_pending_four_seconds_then_reap_stop_stays_latched() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke_wait(revoke)
        .map_err(|err| format!("revoke: {err:?}"))?;
    push_present(&io);
    io.owner.borrow_mut().push_stop();
    let base_status = kind_count(&io, CommandKind::Status);
    let (deadlines, caller) = deadlines_at(&io);
    renew(&io)?;
    let mut progress = attempt
        .destroy(&deadlines)
        .map_err(|err| format!("destroy: {err:?}"))?;
    let mut armed = false;
    let mut stop_id = None;
    let mut seen = 0u32;
    for _ in 0..120 {
        if matches!(progress, StepProgress::Pending)
            && !armed
            && kind_count(&io, CommandKind::Status) == base_status + 1
            && attempt.active_command().is_none()
            && kind_count(&io, CommandKind::Stop) == 0
        {
            runner_cost(&io, 4_000);
            armed = true;
        }
        if kind_count(&io, CommandKind::Stop) > 0 {
            runner_cost(&io, 0);
        }
        if let Some(cmd) = attempt.active_command() {
            if cmd.handle.kind() == CommandKind::Stop {
                match stop_id {
                    None => stop_id = Some(cmd.handle.id()),
                    Some(id) if id == cmd.handle.id() => seen += 1,
                    Some(id) => return Err(format!("stop handle changed from {id}")),
                }
            }
        }
        if let StepProgress::Done(CompletedStep::Destroyed) = progress {
            break;
        }
        renew(&io)?;
        progress = attempt
            .poll(caller)
            .map_err(|err| format!("poll: {err:?}"))?;
    }
    if !matches!(progress, StepProgress::Done(CompletedStep::Destroyed)) {
        return Err(format!("destroy did not finish: {progress:?}"));
    }
    if !armed || seen < 30 || stop_id.is_none() {
        return Err(format!("stop observations {seen} armed {armed}"));
    }
    if !attempt.stop_latched() || kind_count(&io, CommandKind::Delete) != 1 {
        return Err("stop was not latched through delete".into());
    }
    let stop_deadline = io
        .runner
        .borrow()
        .trace()
        .iter()
        .find(|entry| matches!(entry.command, TartCommand::Stop { .. }))
        .map(|entry| entry.deadline);
    if stop_deadline
        != stop_id.map(|_| {
            io.runner
                .borrow()
                .trace()
                .iter()
                .find(|entry| matches!(entry.command, TartCommand::Stop { .. }))
                .map(|entry| entry.deadline)
                .unwrap_or(MonoInstant::EPOCH)
        })
    {
        return Err("stop deadline moved".into());
    }
    let _ = stop_deadline;
    spans_ok(&attempt)?;
    let seal = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    let sealed = attempt
        .seal_wait(seal)
        .map_err(|err| format!("seal: {err:?}"))?;
    if !attempt.stop_latched() || sealed.followups_allowed() {
        return Err("stop latch did not survive seal".into());
    }
    Ok(())
}

fn r2_2_owner_end_during_destroy_and_reap_does_not_revive() -> Result<(), String> {
    death_during_destroy()?;
    death_during_reap()?;
    Ok(())
}

fn death_during_destroy() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke_wait(revoke)
        .map_err(|err| format!("revoke: {err:?}"))?;
    io.owner
        .borrow_mut()
        .push_proof(RUN, io.clock.now())
        .map_err(|err| format!("{err:?}"))?;
    advance(&io.clock, Duration::from_secs(3))?;
    push_present(&io);
    let err = attempt.destroy_wait(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::OwnerDead)) || attempt.phase() != Phase::Suspended {
        return Err(format!("destroy {err:?} phase {:?}", attempt.phase()));
    }
    if kind_count(&io, CommandKind::Delete) != 0 {
        return Err("owner death deleted".into());
    }
    io.owner
        .borrow_mut()
        .push_proof(RUN, io.clock.now())
        .map_err(|err| format!("{err:?}"))?;
    let again = attempt.sync_owner(query(&io.clock).at());
    if !matches!(again, Err(StepError::OwnerDead)) {
        return Err(format!("revived {again:?}"));
    }
    Ok(())
}

fn death_during_reap() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    io.runner
        .borrow_mut()
        .set_reap_cost(Duration::from_millis(500));
    let caller = query(&io.clock).at();
    renew(&io)?;
    let first = attempt.cancel(query(&io.clock));
    if !matches!(first, Ok(StepProgress::Pending)) {
        return Err(format!("cancel {first:?}"));
    }
    io.owner
        .borrow_mut()
        .push_proof(RUN, io.clock.now())
        .map_err(|err| format!("{err:?}"))?;
    advance(&io.clock, Duration::from_secs(3))?;
    let polled = attempt.poll(caller);
    if !matches!(polled, Err(StepError::OwnerDead)) || attempt.phase() != Phase::Suspended {
        return Err(format!("reap {polled:?} phase {:?}", attempt.phase()));
    }
    if io.clock.now() >= end || kind_count(&io, CommandKind::Delete) != 0 {
        return Err("reap crossed the clone end or deleted".into());
    }
    let seal = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    if !matches!(attempt.seal(seal), Err(StepError::Terminal)) {
        return Err("seal revived the suspended attempt".into());
    }
    Ok(())
}

fn hold_clone(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
) -> Result<MonoInstant, String> {
    let lists = kind_count(io, CommandKind::List);
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(io)?;
    let mut progress = attempt
        .clone_vm(GoldenImage::MacosS2Eval, deadline)
        .map_err(|err| format!("clone: {err:?}"))?;
    for _ in 0..30 {
        if kind_count(io, CommandKind::List) > lists
            && attempt.active_command().is_none()
            && attempt.phase() == Phase::Reserved
        {
            runner_cost(io, 10_000);
        }
        if let Some(cmd) = attempt.active_command() {
            if cmd.handle.kind() == CommandKind::Clone {
                return Ok(cmd.handle.end());
            }
        }
        if !matches!(progress, StepProgress::Pending) {
            return Err(format!("clone finished early {progress:?}"));
        }
        renew(io)?;
        progress = attempt
            .poll(caller)
            .map_err(|err| format!("poll: {err:?}"))?;
    }
    Err("clone handle did not open".into())
}

fn r2_3_stop_during_lock_ledger_io_does_not_adopt_or_skip_delete() -> Result<(), String> {
    stop_during_lock()?;
    stop_during_ledger()?;
    stop_during_clone()?;
    stop_before_delete_does_not_skip()?;
    Ok(())
}

fn stop_during_lock() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Reserved);
    io.lock
        .borrow_mut()
        .advance_on_acquire(Duration::from_millis(500));
    let mut attempt = world.attempt();
    let deadline = query(&io.clock);
    let caller = deadline.at();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    renew(&io)?;
    let first = attempt.reserve(reserve_input(absolute), deadline);
    if !matches!(first, Ok(StepProgress::Pending)) {
        return Err(format!("lock {first:?}"));
    }
    io.owner.borrow_mut().push_stop();
    let err = pump(&mut attempt, &io, caller, Ok(StepProgress::Pending));
    if !matches!(err, Err(ref message) if message.contains("StopLatched")) {
        return Err(format!("lock stop {err:?}"));
    }
    if attempt.phase() == Phase::Reserved || kind_count(&io, CommandKind::Delete) != 0 {
        return Err("lock stop adopted a reserve or deleted".into());
    }
    spans_ok(&attempt)?;
    Ok(())
}

fn stop_during_ledger() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Reserved);
    io.ledger
        .borrow_mut()
        .set_io_cost(Duration::from_millis(250));
    let mut attempt = world.attempt();
    let deadline = query(&io.clock);
    let caller = deadline.at();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    renew(&io)?;
    let mut progress = attempt
        .reserve(reserve_input(absolute), deadline)
        .map_err(|err| format!("{err:?}"))?;
    let mut armed = false;
    for _ in 0..20 {
        if matches!(progress, StepProgress::Pending)
            && io.clock.now() > MonoInstant::EPOCH
            && !armed
        {
            io.owner.borrow_mut().push_stop();
            armed = true;
        }
        if armed {
            break;
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    let err = pump(&mut attempt, &io, caller, Ok(progress));
    if !armed || !matches!(err, Err(ref message) if message.contains("StopLatched")) {
        return Err(format!("ledger stop {err:?} armed {armed}"));
    }
    if attempt.phase() == Phase::Reserved {
        return Err("ledger stop adopted the reserve".into());
    }
    Ok(())
}

fn reserve_input(absolute: AttemptDeadline) -> gcu_eval::disposable::tart::ReserveInput {
    gcu_eval::disposable::tart::ReserveInput {
        run: id_run(RUN),
        clone_id: id_clone(CLONE),
        absolute,
        manifest: harness::manifest(),
        approved: harness::manifest(),
        stop_latched: false,
        owner_present: true,
    }
}

fn stop_during_clone() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let _end = hold_clone(&mut attempt, &io)?;
    io.owner.borrow_mut().push_stop();
    runner_cost(&io, 0);
    io.runner.borrow_mut().set_reap_cost(Duration::ZERO);
    let caller = query(&io.clock).at();
    renew(&io)?;
    let polled = attempt.poll(caller);
    let done = pump(&mut attempt, &io, caller, polled)?;
    if !matches!(done, CompletedStep::Cancelled) || attempt.phase() == Phase::Cloned {
        return Err(format!("clone stop {done:?} {:?}", attempt.phase()));
    }
    if kind_count(&io, CommandKind::Start) != 0 || !attempt.stop_latched() {
        return Err("stop during clone started or cleared the latch".into());
    }
    if io.runner.borrow().kill_count() != 1 {
        return Err("kill did not begin once".into());
    }
    Ok(())
}

fn stop_before_delete_does_not_skip() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Revoking)?;
    push_present(&io);
    let base = kind_count(&io, CommandKind::Status);
    let (deadlines, caller) = deadlines_at(&io);
    renew(&io)?;
    let mut progress = attempt
        .destroy(&deadlines)
        .map_err(|err| format!("{err:?}"))?;
    for _ in 0..40 {
        if !matches!(progress, StepProgress::Pending) {
            return Err(format!("destroy finished before delete {progress:?}"));
        }
        if kind_count(&io, CommandKind::Status) >= base + 2 && attempt.active_command().is_none() {
            break;
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    if !matches!(progress, StepProgress::Pending) {
        return Err(format!("destroy finished before delete {progress:?}"));
    }
    let deletes = kind_count(&io, CommandKind::Delete);
    io.owner.borrow_mut().push_stop();
    renew(&io)?;
    progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    if kind_count(&io, CommandKind::Delete) != deletes || !matches!(progress, StepProgress::Pending)
    {
        return Err("delete began in the stop step".into());
    }
    renew(&io)?;
    let _ = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    if kind_count(&io, CommandKind::Delete) != deletes + 1 {
        return Err("delete did not begin on the next step".into());
    }
    if !attempt.stop_latched() {
        return Err("stop did not latch".into());
    }
    Ok(())
}

fn r2_4_lock_yield_slow_ops_shared_step_begin_once() -> Result<(), String> {
    lock_yields_five_hundred()?;
    slow_clone_begins_once()?;
    shared_lease_and_inventory()?;
    slow_seal_begins_once()?;
    Ok(())
}

fn lock_yields_five_hundred() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Reserved);
    io.lock
        .borrow_mut()
        .advance_on_acquire(Duration::from_millis(500));
    let mut attempt = world.attempt();
    let start = io.clock.now();
    reserve_default(&mut attempt, &io)?;
    let moved = io.clock.now().saturating_duration_since(start);
    if moved != Duration::from_millis(500) || attempt.phase() != Phase::Reserved {
        return Err(format!("lock move {moved:?} {:?}", attempt.phase()));
    }
    spans_ok(&attempt)?;
    if attempt.step_spans().len() < 5 {
        return Err("500ms lock fit in one step".into());
    }
    Ok(())
}

fn slow_clone_begins_once() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    runner_cost(&io, 250);
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(&io)?;
    let first = attempt.clone_vm(GoldenImage::MacosS2Eval, deadline);
    let done = pump(&mut attempt, &io, caller, first)?;
    if !matches!(done, CompletedStep::Cloned) || kind_count(&io, CommandKind::Clone) != 1 {
        return Err(format!(
            "clone begins {}",
            kind_count(&io, CommandKind::Clone)
        ));
    }
    let ends: Vec<_> = io
        .runner
        .borrow()
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, TartCommand::Clone { .. }))
        .map(|entry| entry.deadline)
        .collect();
    if ends.len() != 1 {
        return Err(format!("clone deadlines {ends:?}"));
    }
    spans_ok(&attempt)?;
    runner_cost(&io, 250);
    // Only the start scripts. scripts_to_reach(Booting) would queue another
    // clear list first, and start treats a missing name as an identity conflict.
    harness::push_named(&io, CLONE);
    harness::push_status(&io, "running", RUN, "up", "up");
    harness::push_runner(&io, ok_body("ok"));
    let start = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
    let start_at = start.at();
    renew(&io)?;
    let starting = attempt.start(&harness::flags(), start);
    let started = pump(&mut attempt, &io, start_at, starting)?;
    if !matches!(started, CompletedStep::Started) || kind_count(&io, CommandKind::Start) != 1 {
        return Err("start began more than once".into());
    }
    spans_ok(&attempt)
}

fn shared_lease_and_inventory() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Booting)?;
    guest_cost(&io, 60);
    push_guest(&io, ok_body("ok"));
    push_isolate(&io, "passed");
    io.owner.borrow_mut().push_priority_lease();
    renew(&io)?;
    let group = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
    let caller = group.at();
    let first = attempt
        .isolate(group)
        .map_err(|err| format!("isolate: {err:?}"))?;
    if !matches!(first, StepProgress::Pending) {
        return Err(format!("lease step {first:?}"));
    }
    let commands: Vec<_> = io
        .guest
        .borrow()
        .trace()
        .iter()
        .map(|entry| entry.command.clone())
        .collect();
    if commands.len() != 1 || !matches!(commands[0], GuestCommand::Lease { .. }) {
        return Err(format!("shared step ran {commands:?}"));
    }
    let span = attempt.step_spans().last().copied().ok_or("no span")?;
    if span.elapsed() != Duration::from_millis(60) || span.elapsed() > Duration::from_millis(100) {
        return Err(format!("lease span {:?}", span.elapsed()));
    }
    // The lease occupied the entry. The next tick may be the isolate lock;
    // inventory is the following guest slice, still one begin.
    renew(&io)?;
    let mut inventories = 0;
    for _ in 0..3 {
        let step = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
        if !matches!(step, StepProgress::Pending) {
            return Err(format!("inventory step {step:?}"));
        }
        inventories = io
            .guest
            .borrow()
            .trace()
            .iter()
            .filter(|entry| matches!(entry.command, GuestCommand::Inventory))
            .count();
        if inventories > 1 {
            return Err(format!("inventory count {inventories}"));
        }
        if inventories == 1 {
            break;
        }
        renew(&io)?;
    }
    if inventories != 1 {
        return Err(format!("inventory count {inventories}"));
    }
    spans_ok(&attempt)
}

fn slow_seal_begins_once() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke_wait(revoke)
        .map_err(|err| format!("{err:?}"))?;
    push_present(&io);
    attempt
        .destroy_wait(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    io.ledger
        .borrow_mut()
        .set_io_cost(Duration::from_millis(250));
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(&io)?;
    let sealing = attempt.seal(deadline);
    let done = pump(&mut attempt, &io, caller, sealing)?;
    if !matches!(done, CompletedStep::Sealed(_)) || seal_count(&io) != 1 {
        return Err(format!("seal begins {}", seal_count(&io)));
    }
    spans_ok(&attempt)
}

fn r2_5_bootstrap_login_run_unknown_survives_cleanup() -> Result<(), String> {
    for op in [GuestOp::Bootstrap, GuestOp::Login, GuestOp::Run] {
        cover(op, Fault::Lost)?;
        cover(op, Fault::AtEnd)?;
        cover(op, Fault::Parse)?;
        cover(op, Fault::Provider)?;
        cover(op, Fault::Runtime)?;
    }
    known_first_fault()
}

#[derive(Clone, Copy)]
enum GuestOp {
    Bootstrap,
    Login,
    Run,
}

#[derive(Clone, Copy)]
enum Fault {
    Lost,
    AtEnd,
    Parse,
    Provider,
    Runtime,
}

fn cover(op: GuestOp, fault: Fault) -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let phase = match op {
        GuestOp::Bootstrap => Phase::Isolated,
        GuestOp::Login => Phase::Bootstrapped,
        GuestOp::Run => Phase::Ready,
    };
    let mut attempt = reach(&mut world, &io, phase)?;
    let before = attempt.phase();
    match fault {
        Fault::Lost => {
            guest_cost(&io, 40);
            io.guest.borrow_mut().push_state(ReplyState::Pending);
        }
        Fault::AtEnd => guest_cost(&io, 1_000),
        Fault::Parse => {
            guest_cost(&io, 40);
            io.guest.borrow_mut().push_state(ReplyState::Pending);
            push_guest(&io, ok_body("nope"));
        }
        Fault::Provider | Fault::Runtime => {
            guest_cost(&io, 40);
            io.guest.borrow_mut().push_state(ReplyState::Accepted);
        }
    }
    let started = io.clock.now();
    let (caller, mut progress) = begin_guest(&mut attempt, &io, op, fault)?;
    let mut failed = None;
    for _ in 0..12 {
        let charged =
            io.clock.now().saturating_duration_since(started) >= Duration::from_millis(40);
        if charged && matches!(progress, StepProgress::Pending) && !matches!(fault, Fault::AtEnd) {
            break;
        }
        match progress {
            StepProgress::Pending => {
                renew(&io)?;
                match attempt.poll(caller) {
                    Ok(next) => progress = next,
                    Err(err) => {
                        failed = Some(err);
                        break;
                    }
                }
            }
            other => return Err(format!("left pending early {other:?}")),
        }
    }
    if attempt.phase() != before && !matches!(fault, Fault::AtEnd) {
        return Err(format!(
            "phase moved before the fault {:?}",
            attempt.phase()
        ));
    }
    match fault {
        Fault::Provider => {
            let noted = attempt
                .provider_wait(query(&io.clock))
                .map_err(|err| format!("provider: {err:?}"))?;
            if !matches!(noted, CompletedStep::ProviderCrashNoted) {
                return Err(format!("provider {noted:?}"));
            }
        }
        Fault::Runtime => {
            let noted = attempt
                .runtime_wait(RuntimeLoss::GuestCrash, query(&io.clock))
                .map_err(|err| format!("runtime: {err:?}"))?;
            if !matches!(noted, CompletedStep::RuntimeLossNoted) {
                return Err(format!("runtime {noted:?}"));
            }
        }
        Fault::Lost | Fault::AtEnd | Fault::Parse => {
            if failed.is_none() {
                renew(&io)?;
                match attempt.poll(caller) {
                    Err(err) => failed = Some(err),
                    Ok(progress) => return Err(format!("fault was adopted {progress:?}")),
                }
            }
            if failed.is_none() {
                return Err("issued fault was adopted".into());
            }
        }
    }
    if !attempt.effect_unknown() {
        return Err("issued fault stayed known".into());
    }
    guest_cost(&io, 0);
    push_present(&io);
    let receipt = finish(&mut attempt, &io, true)?;
    if !matches!(receipt.outcome(), AttemptOutcome::Quarantined { .. })
        || receipt.followups_allowed()
        || io.guest.borrow().consume_count() != 0
    {
        return Err(format!(
            "cleanup cleared unknown {:?} consumes {}",
            receipt.outcome(),
            io.guest.borrow().consume_count()
        ));
    }
    Ok(())
}

fn begin_guest(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
    op: GuestOp,
    fault: Fault,
) -> Result<(MonoInstant, StepProgress<CompletedStep>), String> {
    renew(io)?;
    let limit = if matches!(fault, Fault::AtEnd) {
        Duration::from_millis(200)
    } else {
        Duration::from_secs(5)
    };
    let now = io.clock.now();
    match op {
        GuestOp::Bootstrap => {
            let deadline = StageDeadline::after(now, limit.min(StageDeadline::CAP)).unwrap();
            let caller = deadline.at();
            let progress = attempt
                .bootstrap(deadline)
                .map_err(|err| format!("bootstrap: {err:?}"))?;
            Ok((caller, progress))
        }
        GuestOp::Login => {
            let deadline = LoginDeadline::after(now, limit.min(LoginDeadline::CAP)).unwrap();
            let caller = deadline.at();
            let progress = attempt
                .login(deadline)
                .map_err(|err| format!("login: {err:?}"))?;
            Ok((caller, progress))
        }
        GuestOp::Run => {
            let deadline = RunDeadline::after(now, limit.min(RunDeadline::CAP)).unwrap();
            let caller = deadline.at();
            let progress = attempt
                .run(deadline)
                .map_err(|err| format!("run: {err:?}"))?;
            Ok((caller, progress))
        }
    }
}

fn known_first_fault() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Isolated)?;
    push_guest(&io, ok_body("nope"));
    let deadline = StageDeadline::after(io.clock.now(), StageDeadline::CAP).unwrap();
    let err = attempt.bootstrap_wait(deadline);
    if err.is_ok() || attempt.effect_unknown() {
        return Err(format!(
            "first fault {err:?} unknown {}",
            attempt.effect_unknown()
        ));
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::MissingProof, false, true)
}

fn r2_6_kill_reap_pending_frozen_end_and_strict_exit() -> Result<(), String> {
    strict_reap_with_stop()?;
    reap_slices()?;
    for fault in [
        ReapFault::Empty,
        ReapFault::Malformed,
        ReapFault::Unknown,
        ReapFault::WrongHandle,
        ReapFault::Late,
    ] {
        bad_reap(fault)?;
    }
    pending_reap_stops_at_frozen_end()?;
    Ok(())
}

fn strict_reap_with_stop() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    runner_cost(&io, 0);
    io.runner.borrow_mut().set_reap_cost(Duration::ZERO);
    let caller = query(&io.clock).at();
    io.owner.borrow_mut().push_stop();
    renew(&io)?;
    let cancelling = attempt.cancel(query(&io.clock));
    let done = pump(&mut attempt, &io, caller, cancelling)?;
    if !matches!(done, CompletedStep::Cancelled) {
        return Err(format!("strict {done:?}"));
    }
    if io.runner.borrow().kill_count() != 1 || !attempt.stop_latched() || io.clock.now() >= end {
        return Err("strict reap missed the frozen end or the latch".into());
    }
    spans_ok(&attempt)
}

fn reap_slices() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    runner_cost(&io, 0);
    io.runner
        .borrow_mut()
        .set_reap_cost(Duration::from_millis(350));
    let caller = query(&io.clock).at();
    let start = io.clock.now();
    renew(&io)?;
    let cancelling = attempt.cancel(query(&io.clock));
    let done = pump(&mut attempt, &io, caller, cancelling)?;
    let moved = io.clock.now().saturating_duration_since(start);
    if !matches!(done, CompletedStep::Cancelled) || moved != Duration::from_millis(350) {
        return Err(format!("slices {done:?} {moved:?}"));
    }
    if io.runner.borrow().kill_count() != 1 || io.clock.now() >= end {
        return Err("sliced reap began twice or passed the end".into());
    }
    spans_ok(&attempt)
}

fn bad_reap(fault: ReapFault) -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let _end = hold_clone(&mut attempt, &io)?;
    runner_cost(&io, 0);
    io.runner.borrow_mut().set_reap_fault(Some(fault));
    let caller = query(&io.clock).at();
    renew(&io)?;
    let cancelling = attempt.cancel(query(&io.clock));
    let err = pump(&mut attempt, &io, caller, cancelling);
    if err.is_ok() || !attempt.effect_unknown() || io.runner.borrow().kill_count() != 1 {
        return Err(format!("fault {fault:?} {err:?}"));
    }
    if attempt.phase() == Phase::Cloned {
        return Err("bad reap adopted the clone".into());
    }
    Ok(())
}

fn pending_reap_stops_at_frozen_end() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    runner_cost(&io, 0);
    io.runner
        .borrow_mut()
        .set_reap_fault(Some(ReapFault::PendingForever));
    let caller = query(&io.clock).at();
    renew(&io)?;
    let mut progress = attempt
        .cancel(query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    for _ in 0..3 {
        if !matches!(progress, StepProgress::Pending) {
            return Err(format!("pending reap stopped early {progress:?}"));
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    let kills = io.runner.borrow().kill_count();
    let left = end.saturating_duration_since(io.clock.now());
    harness::advance_live(&mut attempt, &io, left)?;
    let err = attempt.poll(caller);
    if err.is_ok() || !attempt.effect_unknown() || io.runner.borrow().kill_count() != kills {
        return Err(format!("frozen {err:?} kills {kills}"));
    }
    if io.clock.now() < end {
        return Err("frozen reap returned before the end".into());
    }
    Ok(())
}

fn d3_early_cancel_reaps_in_one_point_two_seconds() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    runner_cost(&io, 0);
    io.runner
        .borrow_mut()
        .set_reap_cost(Duration::from_millis(1_200));
    let caller = query(&io.clock).at();
    let start = io.clock.now();
    renew(&io)?;
    let cancelling = attempt.cancel(query(&io.clock));
    let done = pump(&mut attempt, &io, caller, cancelling)?;
    let moved = io.clock.now().saturating_duration_since(start);
    if !matches!(done, CompletedStep::Cancelled) || moved != Duration::from_millis(1_200) {
        return Err(format!("early {done:?} {moved:?}"));
    }
    if io.clock.now() >= end || attempt.effect_unknown() || io.runner.borrow().kill_count() != 1 {
        return Err("early cancel missed the clone end or the one kill".into());
    }
    if kind_count(&io, CommandKind::Clone) != 0 {
        return Err("clone body was traced as success".into());
    }
    spans_ok(&attempt)
}

fn d3_cancel_at_work_end_is_unresolved_or_within_op_end() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let end = hold_clone(&mut attempt, &io)?;
    let share = Duration::from_millis(1_200);
    let left = end.saturating_duration_since(io.clock.now());
    if left <= share {
        return Err(format!("no work window before {end:?}"));
    }
    runner_cost(&io, 0);
    io.runner.borrow_mut().set_reap_cost(share);
    harness::advance_live(&mut attempt, &io, left - share)?;
    let caller = query(&io.clock).at();
    renew(&io)?;
    let polled = attempt.poll(caller);
    let err = pump(&mut attempt, &io, caller, polled);
    if err.is_ok() || !attempt.effect_unknown() {
        return Err(format!("work_end adopted {err:?}"));
    }
    if io.clock.now() > end || attempt.phase() == Phase::Cloned {
        return Err("unresolved reap passed the original end".into());
    }
    if io.runner.borrow().kill_count() != 1 {
        return Err("work_end kill began more than once".into());
    }
    Ok(())
}

fn d3_cleanup_cancel_stays_inside_phase_budgets() -> Result<(), String> {
    if RevokeDeadline::CAP != Duration::from_secs(10)
        || StopDeadline::CAP != Duration::from_secs(20)
        || DeleteDeadline::CAP != Duration::from_secs(20)
        || SealDeadline::CAP != Duration::from_secs(10)
        || CloneDeadline::CAP != Duration::from_secs(60)
    {
        return Err("phase caps changed".into());
    }
    revoke_cancel_is_prompt()?;
    stop_cancel_stops_at_twenty_seconds()?;
    Ok(())
}

fn revoke_cancel_is_prompt() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Running)?;
    guest_cost(&io, 5_000);
    let deadline = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(&io)?;
    let first = attempt.revoke(deadline).map_err(|err| format!("{err:?}"))?;
    if !matches!(first, StepProgress::Pending) {
        return Err(format!("revoke {first:?}"));
    }
    let start = io.clock.now();
    let cancelling = attempt.cancel(query(&io.clock));
    let done = pump(&mut attempt, &io, caller, cancelling)?;
    let moved = io.clock.now().saturating_duration_since(start);
    if !matches!(done, CompletedStep::Cancelled) || moved > Duration::from_secs(1) {
        return Err(format!("revoke cancel {done:?} {moved:?}"));
    }
    if io.runner.borrow().kill_count() != 0 {
        return Err("revoke cancel started a runner kill".into());
    }
    Ok(())
}

fn stop_cancel_stops_at_twenty_seconds() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Revoking)?;
    push_present(&io);
    let base = kind_count(&io, CommandKind::Status);
    let (deadlines, caller) = deadlines_at(&io);
    renew(&io)?;
    let mut progress = attempt
        .destroy(&deadlines)
        .map_err(|err| format!("{err:?}"))?;
    for _ in 0..40 {
        if !matches!(progress, StepProgress::Pending) {
            return Err(format!("destroy left the stop window {progress:?}"));
        }
        if kind_count(&io, CommandKind::Status) == base + 1
            && attempt.active_command().is_none()
            && kind_count(&io, CommandKind::Stop) == 0
        {
            runner_cost(&io, 60_000);
            break;
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    if !matches!(progress, StepProgress::Pending) {
        return Err(format!("destroy left the stop window {progress:?}"));
    }
    renew(&io)?;
    progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    let end = attempt
        .active_command()
        .filter(|cmd| cmd.handle.kind() == CommandKind::Stop)
        .map(|cmd| cmd.handle.end())
        .ok_or(format!("stop did not stay open {progress:?}"))?;
    io.runner
        .borrow_mut()
        .set_reap_cost(Duration::from_secs(30));
    let start = io.clock.now();
    let cancelling = attempt.cancel(query(&io.clock));
    // The destroy query caller is 5s. Reap must be allowed to run until the
    // saved stop end, which is the 20s stop cap, not that shorter query.
    let err = pump(&mut attempt, &io, end, cancelling);
    let moved = io.clock.now().saturating_duration_since(start);
    if err.is_ok() || !attempt.effect_unknown() {
        return Err(format!("stop cancel adopted {err:?}"));
    }
    if io.clock.now() > end || moved >= Duration::from_secs(30) || moved < Duration::from_secs(19) {
        return Err(format!(
            "stop cancel moved {moved:?} end {end:?} now {:?}",
            io.clock.now()
        ));
    }
    Ok(())
}

fn d4_seal_and_recovery_deliver_once_via_poll() -> Result<(), String> {
    seal_once()?;
    recover_once()?;
    Ok(())
}

fn seal_once() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = reach(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke_wait(revoke)
        .map_err(|err| format!("{err:?}"))?;
    push_present(&io);
    attempt
        .destroy_wait(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    let caller = deadline.at();
    let entry = attempt
        .seal(deadline)
        .map_err(|err| format!("seal: {err:?}"))?;
    if !matches!(entry, StepProgress::Pending) {
        return Err(format!("seal entry {entry:?}"));
    }
    let mut progress = entry;
    let mut delivered = None;
    for _ in 0..20 {
        match progress {
            StepProgress::Pending => {
                progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
            }
            StepProgress::Done(CompletedStep::Sealed(receipt)) => {
                delivered = Some(receipt);
                break;
            }
            other => return Err(format!("seal poll {other:?}")),
        }
    }
    let receipt = delivered.ok_or("seal was not delivered")?;
    if !matches!(attempt.poll(caller), Err(StepError::NoActiveStep)) {
        return Err("second seal poll repeated the receipt".into());
    }
    if seal_count(&io) != 1 || attempt.receipt().is_none() {
        return Err(format!("seal writes {} receipt missing", seal_count(&io)));
    }
    if receipt.outcome().score().is_none() {
        return Err(format!("seal outcome {:?}", receipt.outcome()));
    }
    Ok(())
}

fn recover_once() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    plant_row(&io, RUN, CLONE, AttemptDeadline::CAP, false)?;
    plant_row(&io, RUN_2, CLONE_2, AttemptDeadline::CAP, false)?;
    push_clear(&io);
    push_clear(&io);
    let mut attempt = world.attempt();
    let (deadlines, caller) = deadlines_at(&io);
    let entry = attempt
        .recover(&deadlines)
        .map_err(|err| format!("recover: {err:?}"))?;
    if !matches!(entry, StepProgress::Pending) {
        return Err(format!("recover entry {entry:?}"));
    }
    let mut progress = entry;
    let mut delivered = None;
    for _ in 0..40 {
        progress = match progress {
            StepProgress::Pending => attempt.poll(caller).map_err(|err| format!("{err:?}"))?,
            StepProgress::Done(CompletedStep::Recovered(rows)) => {
                delivered = Some(rows);
                break;
            }
            other => return Err(format!("recover poll {other:?}")),
        };
    }
    let rows = delivered.ok_or("recovery was not delivered")?;
    if rows.len() != 2 {
        return Err(format!("recovered {}", rows.len()));
    }
    if !matches!(attempt.poll(caller), Err(StepError::NoActiveStep)) {
        return Err("second recover poll repeated the rows".into());
    }
    if seal_count(&io) != 2 || attempt.phase() != Phase::Idle {
        return Err(format!(
            "seals {} phase {:?}",
            seal_count(&io),
            attempt.phase()
        ));
    }
    Ok(())
}

fn plant_row(
    io: &harness::Handles,
    run: &str,
    clone: &str,
    limit: Duration,
    started: bool,
) -> Result<(), String> {
    let absolute = AttemptDeadline::after(io.clock.now(), limit).unwrap();
    let call = query(&io.clock);
    let clone_id = id_clone(clone);
    let mut ledger = io.ledger.borrow_mut();
    let permit = ledger
        .reserve(id_run(run), clone_id.clone(), absolute, call)
        .map_err(|err| format!("plant: {err:?}"))?;
    drop(permit);
    let identity = VmIdentity::from_parts(INC.to_owned(), CFG.to_owned());
    ledger
        .note_identity(&clone_id, &identity, call)
        .map_err(|err| format!("identity: {err:?}"))?;
    ledger
        .append(&clone_id, LedgerFact::CloneIssued, call)
        .map_err(|err| format!("clone fact: {err:?}"))?;
    if started {
        ledger
            .append(&clone_id, LedgerFact::StartIssued, call)
            .map_err(|err| format!("start fact: {err:?}"))?;
    }
    Ok(())
}

fn d4_permit_begins_once_across_entry_and_poll() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    runner_cost(&io, 200);
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(&io)?;
    let entry = attempt
        .clone_vm(GoldenImage::MacosS2Eval, deadline)
        .map_err(|err| format!("{err:?}"))?;
    if !matches!(entry, StepProgress::Pending) {
        return Err(format!("clone entry {entry:?}"));
    }
    let begins_at_entry = io.runner.borrow().begin_count();
    let done = pump(&mut attempt, &io, caller, Ok(entry))?;
    if !matches!(done, CompletedStep::Cloned) || kind_count(&io, CommandKind::Clone) != 1 {
        return Err("permit moved more than once".into());
    }
    if io.runner.borrow().begin_count() <= begins_at_entry
        && kind_count(&io, CommandKind::List) == 0
    {
        return Err("clone never began".into());
    }
    spans_ok(&attempt)
}

fn d5_public_path_pending_accepted_then_cleanup() -> Result<(), String> {
    guest_progress(GuestOp::Bootstrap, D5::Lost)?;
    guest_progress(GuestOp::Login, D5::Stop)?;
    guest_progress(GuestOp::Run, D5::Owner)?;
    Ok(())
}

#[derive(Clone, Copy)]
enum D5 {
    Lost,
    Stop,
    Owner,
}

fn guest_progress(op: GuestOp, inject: D5) -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let phase = match op {
        GuestOp::Bootstrap => Phase::Isolated,
        GuestOp::Login => Phase::Bootstrapped,
        GuestOp::Run => Phase::Ready,
    };
    let mut attempt = reach(&mut world, &io, phase)?;
    guest_cost(&io, 40);
    io.guest.borrow_mut().push_state(ReplyState::Pending);
    io.guest.borrow_mut().push_state(ReplyState::Accepted);
    let start = io.clock.now();
    let (caller, mut progress) = begin_guest(&mut attempt, &io, op, Fault::Lost)?;
    for _ in 0..8 {
        if io.clock.now().saturating_duration_since(start) >= Duration::from_millis(80)
            && matches!(progress, StepProgress::Pending)
        {
            break;
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    if attempt.phase() != phase || !matches!(progress, StepProgress::Pending) {
        return Err(format!("progress {:?} {:?}", attempt.phase(), progress));
    }
    match inject {
        D5::Lost => {
            renew(&io)?;
            let err = attempt.poll(caller);
            if err.is_ok() || !attempt.effect_unknown() {
                return Err(format!("lost {err:?}"));
            }
            guest_cost(&io, 0);
            push_present(&io);
            let receipt = finish(&mut attempt, &io, true)?;
            expect_quarantine(&receipt, NotRunReason::ReplyLost)?;
        }
        D5::Stop => {
            io.owner.borrow_mut().push_stop();
            renew(&io)?;
            let err = attempt.poll(caller);
            if !matches!(err, Err(StepError::StopLatched)) || !attempt.stop_latched() {
                return Err(format!("stop {err:?}"));
            }
            guest_cost(&io, 0);
            push_present(&io);
            let receipt = finish(&mut attempt, &io, true)?;
            if receipt.followups_allowed() || kind_count(&io, CommandKind::Delete) != 1 {
                return Err("stop cleanup did not finish under the latch".into());
            }
        }
        D5::Owner => {
            io.owner
                .borrow_mut()
                .push_proof(RUN, io.clock.now())
                .map_err(|err| format!("{err:?}"))?;
            advance(&io.clock, Duration::from_secs(3))?;
            let err = attempt.poll(caller);
            if !matches!(err, Err(StepError::OwnerDead))
                || kind_count(&io, CommandKind::Delete) != 0
            {
                return Err(format!("owner {err:?}"));
            }
            if attempt.phase() != Phase::Suspended {
                return Err(format!("phase {:?}", attempt.phase()));
            }
        }
    }
    if io.guest.borrow().consume_count() != 0 {
        return Err("guest op was consumed".into());
    }
    let issued = attempt
        .effects()
        .iter()
        .any(|row| row.dispatch == DispatchState::Issued);
    if !issued {
        return Err("no dispatch record".into());
    }
    Ok(())
}

fn r2_7_two_row_recovery_start_issued_does_not_inherit() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    plant_row(&io, RUN, CLONE, AttemptDeadline::CAP, false)?;
    plant_row(&io, RUN_2, CLONE_2, Duration::from_secs(1), true)?;
    advance(&io.clock, Duration::from_secs(2))?;
    push_clear(&io);
    push_clear(&io);
    let mut attempt = world.attempt();
    let (deadlines, caller) = deadlines_at(&io);
    let mut progress = attempt
        .recover(&deadlines)
        .map_err(|err| format!("{err:?}"))?;
    let mut saw_b = false;
    let mut saw_a_after_b = false;
    for _ in 0..40 {
        if let Some(key) = attempt.reclaim_row_key() {
            if key.clone_id.as_str() == CLONE_2 {
                saw_b = true;
            } else if saw_b && key.clone_id.as_str() == CLONE {
                saw_a_after_b = true;
            }
        }
        match progress {
            StepProgress::Pending => match attempt.poll(caller) {
                Ok(next) => progress = next,
                Err(err) => {
                    if saw_b || attempt.phase() == Phase::Quarantined {
                        break;
                    }
                    return Err(format!("recovery: {err:?}"));
                }
            },
            other => return Err(format!("row b should not finish recovery {other:?}")),
        }
        if saw_b && !matches!(progress, StepProgress::Pending) {
            break;
        }
        if attempt.phase() == Phase::Quarantined {
            break;
        }
    }
    if !saw_b || saw_a_after_b {
        return Err(format!(
            "row inheritance saw_b {saw_b} back {saw_a_after_b}"
        ));
    }
    let row_b = io
        .ledger
        .borrow()
        .query(&id_clone(CLONE_2), query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    let row_a = io
        .ledger
        .borrow()
        .query(&id_clone(CLONE), query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    if row_b.is_sealed() || !row_a.is_sealed() {
        return Err(format!(
            "sealed A {} B {}",
            row_a.is_sealed(),
            row_b.is_sealed()
        ));
    }
    if kind_count(&io, CommandKind::Delete) != 0 || kind_count(&io, CommandKind::Stop) != 0 {
        return Err("missing started name was deleted".into());
    }
    let listed = io.runner.borrow().trace().iter().any(|entry| {
        matches!(entry.command, TartCommand::List)
            && entry.deadline.nanos() == row_b.absolute_deadline().nanos()
    });
    if listed {
        return Err("recovery reused the expired row absolute".into());
    }
    drop(attempt);
    let fresh = world.attempt();
    if fresh.phase() != Phase::Idle || fresh.reclaim_stage().is_some() {
        return Err("new attempt inherited the recovery cursor".into());
    }
    Ok(())
}

fn cooperative_steps_within_one_hundred_ms() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Booting);
    runner_cost(&io, 30);
    let mut attempt = world.attempt();
    drive_paid(&mut attempt, &io, Phase::Booting)?;
    spans_ok(&attempt)?;
    if attempt
        .step_spans()
        .iter()
        .any(|span| span.elapsed() > Duration::from_millis(100))
    {
        return Err("a cooperative step exceeded 100ms".into());
    }
    Ok(())
}

fn drive_paid(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
    target: Phase,
) -> Result<(), String> {
    for _ in 0..12 {
        if attempt.phase() == target {
            return Ok(());
        }
        renew(io)?;
        match attempt.phase() {
            Phase::Idle => reserve_default(attempt, io)?,
            Phase::Reserved => {
                let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
                attempt
                    .clone_wait(GoldenImage::MacosS2Eval, deadline)
                    .map_err(|err| format!("clone: {err:?}"))?;
            }
            Phase::Cloned => {
                let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
                attempt
                    .start_wait(&harness::flags(), deadline)
                    .map_err(|err| format!("start: {err:?}"))?;
            }
            other => return Err(format!("drive stuck at {other:?}")),
        }
    }
    Err(format!("did not reach {target:?}"))
}

fn injected_overrun_does_not_adopt() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io)?;
    let lists = kind_count(&io, CommandKind::List);
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    let caller = deadline.at();
    renew(&io)?;
    let mut progress = attempt
        .clone_vm(GoldenImage::MacosS2Eval, deadline)
        .map_err(|err| format!("{err:?}"))?;
    let mut armed = false;
    for _ in 0..20 {
        if kind_count(&io, CommandKind::List) > lists
            && attempt.active_command().is_none()
            && attempt.phase() == Phase::Reserved
            && !armed
        {
            io.runner
                .borrow_mut()
                .set_overrun(Some(Duration::from_millis(500)));
            armed = true;
        }
        if attempt
            .step_spans()
            .iter()
            .any(|span| span.elapsed() > Duration::from_millis(100))
        {
            break;
        }
        if !matches!(progress, StepProgress::Pending) {
            return Err(format!("overrun adopted early {progress:?}"));
        }
        renew(&io)?;
        progress = attempt.poll(caller).map_err(|err| format!("{err:?}"))?;
    }
    if !armed || attempt.phase() == Phase::Cloned {
        return Err(format!("overrun phase {:?}", attempt.phase()));
    }
    let begins = kind_count(&io, CommandKind::Clone);
    let late = attempt
        .step_spans()
        .iter()
        .any(|span| span.elapsed() > Duration::from_millis(100));
    if !late || begins != 1 {
        return Err(format!("late {late} begins {begins}"));
    }
    let after = match progress {
        StepProgress::Pending => attempt.poll(caller),
        StepProgress::Done(_) => return Err("overrun step returned Done".into()),
    };
    if after.is_ok() || attempt.phase() == Phase::Cloned || kind_count(&io, CommandKind::Clone) != 1
    {
        return Err(format!("later poll adopted {after:?}"));
    }
    if !attempt.effect_unknown() {
        return Err("late clone write stayed a known success".into());
    }
    Ok(())
}

fn host_stop_constants_unchanged() -> Result<(), String> {
    if gcu_core::session::STOP_LATCH_BUDGET != Duration::from_millis(100) {
        return Err("stop latch budget changed".into());
    }
    if gcu_core::session::RELEASE_INPUT_BUDGET != Duration::from_millis(500) {
        return Err("release budget changed".into());
    }
    if gcu_core::driver::NATIVE_TIMEOUT != Duration::from_secs(5) {
        return Err("native timeout changed".into());
    }
    if gcu_core::driver::DRIVER_DEADLINE != Duration::from_millis(4_750) {
        return Err("driver deadline changed".into());
    }
    if gcu_eval::disposable::tart::LEASE_WINDOW_MS != 5_000 {
        return Err("guest lease window changed".into());
    }
    let _ = ScriptOutcome::Success;
    let _ = CleanupDisposition::Unknown;
    Ok(())
}
