//! Orphan recovery, cleanup confirmation, lock, and budget faults.

use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptLedger, BudgetError, BudgetField, CloneDeadline, GroupDeadline,
    GuestCommand, HostClock, LeaseDeadline, LedgerFact, LedgerFault, NotRunReason, Phase,
    PhaseBudgets, ScriptOutcome, SealDeadline, StepError, TartCommand,
};
use gcu_eval::disposable::CleanupDisposition;

use super::faults::{expect_not_run, expect_quarantine, stopped};
use super::harness::{
    self, advance, assert_ids, cleanup_deadlines, drive_to, finish, guest_commands, id_clone,
    ok_body, push_guest, push_present, push_runner, query, reserve_default, reserve_named,
    runner_commands, scripts_to_reach, start_ids, CLONE, CLONE_2, RUN, RUN_2,
};

pub(super) fn owner_death_leaves_cleanup_unknown_until_recovery() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    push_guest(&io, ok_body("ok"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    let lease = LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap();
    attempt
        .arm_owner_lease(lease)
        .map_err(|err| format!("arm: {err:?}"))?;
    advance(&io.clock, Duration::from_secs(3))?;
    push_guest(&io, ok_body("ok"));
    match attempt.observe_owner_lease(query(&io.clock)) {
        Err(StepError::OwnerDead) => {}
        other => return Err(format!("observe: {other:?}")),
    }
    if attempt.phase() != Phase::Suspended || stopped(&io) {
        return Err("owner death deleted the vm".into());
    }
    let receipt = attempt.receipt().cloned().ok_or("missing owner receipt")?;
    expect_not_run(&receipt, NotRunReason::OwnerDeath, true, false)?;
    if receipt.disposition() != CleanupDisposition::Unknown
        || receipt.cleanup_confirmed()
        || !receipt.recovery_required()
    {
        return Err(format!("owner receipt {receipt:?}"));
    }
    drop(attempt);
    let row = io
        .ledger
        .borrow()
        .query(&id_clone(CLONE), query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    if row.is_sealed() || !row.facts().contains(&LedgerFact::CleanupUnknown) {
        return Err(format!("row {row:?}"));
    }
    push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    if recovered.len() != 1 {
        return Err(format!("recovered {recovered:?}"));
    }
    expect_not_run(&recovered[0], NotRunReason::OwnerDeath, false, true)?;
    if again.phase() != Phase::Idle || start_ids(&io) != [CLONE] {
        return Err("recovery restarted the orphan".into());
    }
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    harness::push_clear(&io);
    reserve_named(
        &mut again,
        &io.clock,
        RUN_2,
        CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("new reserve: {err:?}"))?;
    Ok(())
}

pub(super) fn host_crash_recovers_orphan_without_reviving_the_score() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    {
        let mut attempt = world.attempt();
        drive_to(&mut attempt, &io, Phase::Running)?;
        if stopped(&io) {
            return Err("drop path deleted".into());
        }
    }
    push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    expect_not_run(&recovered[0], NotRunReason::HostCrash, false, true)?;
    if start_ids(&io) != [CLONE] || again.phase() != Phase::Idle {
        return Err("orphan was restarted".into());
    }
    drop(again);
    let mut reject = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    harness::push_clear(&io);
    let err = reserve_named(
        &mut reject,
        &io.clock,
        RUN_2,
        CLONE,
        absolute,
        false,
        true,
        harness::manifest(),
    );
    if !matches!(
        err,
        Err(StepError::Ledger(
            gcu_eval::disposable::tart::LedgerError::DuplicateClone
        ))
    ) {
        return Err(format!("old clone id was reused: {err:?}"));
    }
    drop(reject);
    let mut fresh = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    harness::push_clear(&io);
    reserve_named(
        &mut fresh,
        &io.clock,
        RUN_2,
        CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("new id: {err:?}"))?;
    Ok(())
}

pub(super) fn recovery_deletes_after_the_absolute_deadline() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    let absolute = AttemptDeadline::after(io.clock.now(), Duration::from_secs(2)).unwrap();
    let mut attempt = world.attempt();
    reserve_named(
        &mut attempt,
        &io.clock,
        RUN,
        CLONE,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("reserve: {err:?}"))?;
    drive_to(&mut attempt, &io, Phase::Running)?;
    advance(&io.clock, Duration::from_secs(2))?;
    if !matches!(attempt.cancel(query(&io.clock)), Err(StepError::Expired)) {
        return Err("expired attempt was not refused".into());
    }
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::Expired)
    ) || stopped(&io)
    {
        return Err("past-deadline destroy deleted".into());
    }
    drop(attempt);
    push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    expect_not_run(&recovered[0], NotRunReason::HostCrash, false, true)?;
    if !stopped(&io) {
        return Err("recovery did not delete".into());
    }
    Ok(())
}

pub(super) fn listed_absence_does_not_stop_or_delete() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Cloned);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Cloned)?;
    match attempt.cancel(query(&io.clock)) {
        Err(StepError::Lost(NotRunReason::Cancelled)) => {}
        other => return Err(format!("cancel: {other:?}")),
    }
    let receipt = finish(&mut attempt, &io, false)?;
    expect_not_run(&receipt, NotRunReason::Cancelled, false, true)?;
    assert_ids(&runner_commands(&io), &[CLONE])?;
    if stopped(&io) || !receipt.cleanup_confirmed() {
        return Err(format!("listed absence still deleted: {receipt:?}"));
    }
    Ok(())
}

pub(super) fn status_timeout_does_not_delete_and_halts() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Revoking);
    push_runner(
        &io,
        ok_body("list v1\ncount 3\nmacos-s2-eval\nexternal-vm\nclone-1\nend\n"),
    );
    push_runner(&io, gcu_eval::disposable::tart::ScriptedReply::timed_out());
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Revoking)?;
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || stopped(&io) {
        return Err("timeout deleted".into());
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("seal: {err:?}"))?;
    expect_quarantine(&receipt, NotRunReason::CleanupFailed)?;
    drop(attempt);
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut again,
            &io.clock,
            RUN_2,
            CLONE_2,
            absolute,
            false,
            true,
            harness::manifest()
        ),
        Err(StepError::Halted)
    ) {
        return Err("quarantine did not halt".into());
    }
    if again.phase() != Phase::Idle || stopped(&io) {
        return Err("halt issued tart or left idle".into());
    }
    Ok(())
}

pub(super) fn identity_conflict_does_not_stop_or_delete() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Revoking);
    push_runner(
        &io,
        ok_body("list v1\ncount 3\nmacos-s2-eval\nexternal-vm\nclone-1\nend\n"),
    );
    push_runner(
        &io,
        ok_body(
            "status v1 state=running run=other-run boot=boot1 inc=inc1 cfg=cfg1 tart=up softnet=up",
        ),
    );
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Revoking)?;
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || stopped(&io) {
        return Err("identity conflict deleted".into());
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    expect_quarantine(&receipt, NotRunReason::IdentityConflict)
}

pub(super) fn malformed_list_is_not_absence() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Revoking);
    push_runner(&io, ok_body("nope"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Revoking)?;
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || stopped(&io) {
        return Err("malformed list was treated as gone".into());
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    expect_quarantine(&receipt, NotRunReason::CleanupFailed)
}

pub(super) fn delete_confirmation_still_present_is_quarantine() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Revoking);
    let listed = "list v1\ncount 3\nmacos-s2-eval\nexternal-vm\nclone-1\nend\n";
    push_runner(&io, ok_body(listed));
    push_runner(
        &io,
        ok_body(
            "status v1 state=running run=run-1 boot=boot1 inc=inc1 cfg=cfg1 tart=up softnet=up",
        ),
    );
    push_runner(&io, ok_body("ok"));
    push_runner(
        &io,
        ok_body(
            "status v1 state=stopped run=run-1 boot=boot1 inc=inc1 cfg=cfg1 tart=exited softnet=exited",
        ),
    );
    push_runner(&io, ok_body("ok"));
    push_runner(&io, ok_body(listed));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Revoking)?;
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() {
        return Err("present confirmation was accepted".into());
    }
    assert_ids(&runner_commands(&io), &[CLONE])?;
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    expect_quarantine(&receipt, NotRunReason::CleanupFailed)
}

pub(super) fn seal_fault_drops_the_score_and_requires_recovery() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Destroying);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Destroying)?;
    io.ledger.borrow_mut().push_fault(LedgerFault::ReplyLost);
    if attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .is_ok()
    {
        return Err("lost seal succeeded".into());
    }
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    expect_quarantine(&receipt, NotRunReason::ReceiptFailed)?;
    if !receipt.recovery_required() {
        return Err("unsealed seal failure hid recovery".into());
    }
    drop(attempt);
    let row = io
        .ledger
        .borrow()
        .query(&id_clone(CLONE), query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    if row.is_sealed() {
        return Err("failed seal marked the row sealed".into());
    }
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut again,
            &io.clock,
            RUN_2,
            CLONE_2,
            absolute,
            false,
            true,
            harness::manifest()
        ),
        Err(StepError::MustRecover)
    ) {
        return Err("unsealed row did not force recovery".into());
    }
    if runner_commands(&io).iter().any(
        |command| matches!(command, TartCommand::Clone { clone, .. } if clone.as_str() == CLONE_2),
    ) {
        return Err("recovery gate cloned".into());
    }
    Ok(())
}

pub(super) fn lock_busy_returns_without_waiting_or_cloning() -> Result<(), String> {
    // hold_forever forgets a try-lock guard. A later blocking lock would not
    // return; the tart_offline re-exec watchdog fails that child.
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let before = io.clock.now();
    io.lock
        .borrow_mut()
        .hold_forever()
        .map_err(|err| format!("hold: {err:?}"))?;
    let mut attempt = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    let err = reserve_named(
        &mut attempt,
        &io.clock,
        RUN,
        CLONE,
        absolute,
        false,
        true,
        harness::manifest(),
    );
    if !matches!(err, Err(StepError::LockBusy))
        || io.clock.now() != before
        || !runner_commands(&io).is_empty()
    {
        return Err(format!(
            "lock: {err:?} clock {:?} commands {:?}",
            io.clock.now(),
            runner_commands(&io)
        ));
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("seal: {err:?}"))?;
    expect_quarantine(&receipt, NotRunReason::LockBusy)
}

pub(super) fn shrunk_clone_budget_rejects_without_consuming_the_script() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Clone, Duration::from_secs(1))
        .unwrap();
    let mut world = harness::World::new(budgets);
    let io = world.open();
    harness::push_clear(&io);
    harness::push_clone_ok(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    if !matches!(
        attempt.clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            deadline
        ),
        Err(StepError::AboveBudget)
    ) {
        return Err("over-budget clone was issued".into());
    }
    if attempt.phase() != Phase::Reserved
        || runner_commands(&io)
            .iter()
            .any(|command| matches!(command, TartCommand::Clone { .. }))
    {
        return Err("clone script was consumed".into());
    }
    Ok(())
}

pub(super) fn shrunk_prep_budget_expires_without_a_clone() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Prep, Duration::from_millis(1))
        .unwrap();
    let mut world = harness::World::new(budgets);
    let io = world.open();
    harness::push_clear(&io);
    harness::push_clone_ok(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    advance(&io.clock, Duration::from_millis(1))?;
    let deadline = CloneDeadline::after(io.clock.now(), Duration::from_millis(1)).unwrap();
    if !matches!(
        attempt.clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            deadline
        ),
        Err(StepError::Expired)
    ) {
        return Err("prep window stayed open".into());
    }
    if attempt.phase() != Phase::Revoking
        || runner_commands(&io)
            .iter()
            .any(|command| matches!(command, TartCommand::Clone { .. }))
    {
        return Err(format!(
            "phase {:?} trace {:?}",
            attempt.phase(),
            runner_commands(&io)
        ));
    }
    Ok(())
}

pub(super) fn absolute_deadline_clips_the_recorded_clone_deadline() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let absolute = AttemptDeadline::after(io.clock.now(), Duration::from_secs(30)).unwrap();
    let saved = absolute.at();
    harness::push_clear(&io);
    harness::push_clear(&io);
    harness::push_clone_ok(&io);
    let mut attempt = world.attempt();
    reserve_named(
        &mut attempt,
        &io.clock,
        RUN,
        CLONE,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("reserve: {err:?}"))?;
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    attempt
        .clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            deadline,
        )
        .map_err(|err| format!("clone: {err:?}"))?;
    let trace = io.runner.borrow().trace().to_vec();
    let clone = trace
        .iter()
        .find(|entry| matches!(entry.command, TartCommand::Clone { .. }));
    if clone.map(|entry| entry.deadline) != Some(saved)
        || clone.map(|entry| entry.outcome) != Some(ScriptOutcome::Success)
    {
        return Err(format!("trace {trace:?}"));
    }
    Ok(())
}

pub(super) fn stop_latch_refuses_followups() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut attempt,
            &io.clock,
            RUN,
            CLONE,
            absolute,
            true,
            true,
            harness::manifest()
        ),
        Err(StepError::StopLatched)
    ) {
        return Err("stop latch was cleared".into());
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    expect_not_run(&receipt, NotRunReason::Refused, false, false)?;
    if !runner_commands(&io).is_empty() {
        return Err("stop latch cloned".into());
    }
    Ok(())
}

pub(super) fn manifest_mismatch_is_not_run_without_a_clone() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut attempt,
            &io.clock,
            RUN,
            CLONE,
            absolute,
            false,
            true,
            harness::other_manifest()
        ),
        Err(StepError::ManifestMismatch)
    ) {
        return Err("mismatched manifest was reserved".into());
    }
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    expect_not_run(&receipt, NotRunReason::Refused, false, true)?;
    Ok(())
}

pub(super) fn unsealed_row_must_be_recovered_before_reserve() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    harness::push_clear(&io);
    {
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
        if attempt.phase() != Phase::Reserved {
            return Err("did not reserve".into());
        }
    }
    let listed = runner_commands(&io).len();
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    let err = reserve_named(
        &mut again,
        &io.clock,
        RUN_2,
        CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    );
    if !matches!(err, Err(StepError::MustRecover))
        || again.phase() != Phase::Idle
        || runner_commands(&io).len() != listed
    {
        return Err(format!("recover gate: {err:?} phase {:?}", again.phase()));
    }
    Ok(())
}

pub(super) fn sealed_quarantine_halts_reserve_and_recover_skips_it() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    let seal = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    // The row is the fixture. Dropping the permit does not clone.
    let _ = io
        .ledger
        .borrow_mut()
        .reserve(
            harness::id_run(RUN),
            id_clone(CLONE),
            absolute,
            query(&io.clock),
        )
        .map_err(|err| format!("{err:?}"))?;
    io.ledger
        .borrow_mut()
        .append(&id_clone(CLONE), LedgerFact::Quarantined, query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    io.ledger
        .borrow_mut()
        .seal(&id_clone(CLONE), seal)
        .map_err(|err| format!("{err:?}"))?;
    let mut attempt = world.attempt();
    let recovered = attempt
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    if !recovered.is_empty() || attempt.phase() != Phase::Idle || !runner_commands(&io).is_empty() {
        return Err("sealed quarantine was reclaimed".into());
    }
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut attempt,
            &io.clock,
            RUN_2,
            CLONE_2,
            absolute,
            false,
            true,
            harness::manifest()
        ),
        Err(StepError::Halted)
    ) || attempt.phase() != Phase::Idle
    {
        return Err("halt did not stick".into());
    }
    Ok(())
}

pub(super) fn late_owner_lease_does_not_extend() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    push_guest(&io, ok_body("ok"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    attempt
        .arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    advance(&io.clock, Duration::from_secs(3))?;
    if !matches!(
        attempt.arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap()),
        Err(StepError::LeaseExpired)
    ) {
        return Err("late lease extended".into());
    }
    let leases = guest_commands(&io)
        .iter()
        .filter(|command| matches!(command, GuestCommand::Lease { .. }))
        .count();
    if leases != 1 || attempt.phase() != Phase::Suspended {
        return Err(format!("leases {leases} phase {:?}", attempt.phase()));
    }
    Ok(())
}

pub(super) fn live_owner_lease_does_not_revoke() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    push_guest(&io, ok_body("ok"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    attempt
        .arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap())
        .map_err(|err| format!("{err:?}"))?;
    if !matches!(
        attempt.observe_owner_lease(query(&io.clock)),
        Err(StepError::LeaseLive)
    ) {
        return Err("live lease was declared dead".into());
    }
    if guest_commands(&io)
        .iter()
        .any(|command| matches!(command, GuestCommand::Revoke))
        || attempt.phase() != Phase::Running
    {
        return Err("live lease revoked".into());
    }
    Ok(())
}

pub(super) fn shrunk_probe_group_rejects_before_inventory() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::ProbeGroup, Duration::from_secs(1))
        .unwrap();
    let mut world = harness::World::new(budgets);
    let io = world.open();
    scripts_to_reach(&io, Phase::Booting);
    push_guest(
        &io,
        ok_body("guest v1\nboot=boot1\nuid=uid1\ndesktop=desk1\nconfig=cfg1\nend\n"),
    );
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Booting)?;
    let deadline = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
    if !matches!(attempt.isolate(deadline), Err(StepError::AboveBudget))
        || attempt.phase() != Phase::Booting
        || !guest_commands(&io).is_empty()
    {
        return Err("probe group budget was ignored".into());
    }
    Ok(())
}

pub(super) fn budget_shrink_rejects_zero_and_above_cap() -> Result<(), String> {
    if PhaseBudgets::adr().shrink(BudgetField::Query, Duration::ZERO) != Err(BudgetError::Zero) {
        return Err("zero budget accepted".into());
    }
    if PhaseBudgets::adr().shrink(BudgetField::Query, Duration::from_secs(6))
        != Err(BudgetError::AboveCap)
    {
        return Err("above-cap budget accepted".into());
    }
    Ok(())
}
