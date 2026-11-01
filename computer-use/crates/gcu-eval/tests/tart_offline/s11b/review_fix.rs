//! Round-2 fixes. Reached only through `review::TESTS`.

use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptLedger, AttemptOutcome, BudgetField, CleanupDeadlines, CloneDeadline,
    GuestCommand, HostClock, LeaseDeadline, LedgerError, LedgerFact, LedgerFault, LedgerOp,
    LoginDeadline, NotRunReason, Phase, PhaseBudgets, QueryDeadline, RevokeDeadline, RunDeadline,
    SealDeadline, StartDeadline, StepError, TartCommand, TaskScore,
};

use super::super::faults::expect_not_run;
use super::super::harness::{
    self, cleanup_deadlines, id_clone, ok_body, push_clear, push_clone_ok, push_guest, push_named,
    push_runner, push_status, query, reserve_default, reserve_named, runner_commands, CLONE, INC,
    RUN, RUN_2,
};
use super::{is_clone, is_delete, is_start, is_stop, not_scored, opened, saw};

fn trace_len(io: &harness::Handles) -> usize {
    runner_commands(io).len()
}

fn destructive_since(io: &harness::Handles, before: usize) -> bool {
    runner_commands(io).into_iter().skip(before).any(|command| {
        matches!(
            command,
            TartCommand::Stop { .. } | TartCommand::Delete { .. } | TartCommand::Status { .. }
        )
    })
}

fn ledger_after(
    io: &harness::Handles,
    before: usize,
) -> Vec<gcu_eval::disposable::tart::TraceEntry<LedgerOp>> {
    io.ledger
        .borrow()
        .trace()
        .into_iter()
        .skip(before)
        .collect()
}

fn row(io: &harness::Handles) -> Result<gcu_eval::disposable::tart::Reservation, String> {
    io.ledger
        .borrow()
        .query(&id_clone(CLONE), query(&io.clock))
        .map_err(|err| format!("{err:?}"))
}

pub(super) fn lost_clone_reply_reclaims_the_same_incarnation() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        push_clear(&io);
        push_clear(&io);
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
        let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
        if attempt
            .clone_vm(
                gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
                deadline,
            )
            .is_ok()
        {
            return Err("lost clone succeeded".into());
        }
    }
    harness::push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    expect_not_run(&recovered[0], NotRunReason::HostCrash, false, true)?;
    if !saw(&io, is_delete) || recovered[0].outcome().score().is_some() {
        return Err("same incarnation was not reclaimed".into());
    }
    Ok(())
}

pub(super) fn lost_clone_reply_refuses_a_replacement() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        push_clear(&io);
        push_clear(&io);
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
        let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
        let _ = attempt.clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            deadline,
        );
    }
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(&format!(
            "status v1 state=running run={RUN} boot=boot1 inc=inc2 cfg={} tart=up softnet=up",
            harness::CFG
        )),
    );
    let mut again = world.attempt();
    if again.recover(&cleanup_deadlines(&io.clock)).is_ok()
        || saw(&io, is_stop)
        || saw(&io, is_delete)
    {
        return Err("replacement was destroyed".into());
    }
    Ok(())
}

pub(super) fn unwritten_identity_does_not_clone_or_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        push_clear(&io);
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
        push_clear(&io);
        io.ledger.borrow_mut().push_fault(LedgerFault::ReplyLost);
        let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
        if !matches!(
            attempt.clone_vm(
                gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
                deadline
            ),
            Err(StepError::Ledger(LedgerError::ReplyLost))
        ) {
            return Err("unwritten identity continued".into());
        }
    }
    if runner_commands(&io)
        .iter()
        .any(|command| matches!(command, TartCommand::Clone { .. }))
    {
        return Err("clone was dispatched".into());
    }
    harness::push_present(&io);
    let mut again = world.attempt();
    if again.recover(&cleanup_deadlines(&io.clock)).is_ok()
        || saw(&io, is_stop)
        || saw(&io, is_delete)
    {
        return Err("nameless vm was deleted".into());
    }
    Ok(())
}

pub(super) fn committed_identity_rejects_a_replacement_body() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        push_clear(&io);
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
        push_clear(&io);
        io.ledger
            .borrow_mut()
            .push_fault(LedgerFault::CommittedLost);
        push_runner(&io, ok_body("clone v1 inc=inc2 cfg=cfg2"));
        let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
        if !matches!(
            attempt.clone_vm(
                gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
                deadline
            ),
            Err(StepError::Cleanup(NotRunReason::IdentityConflict))
        ) {
            return Err("mismatched body replaced the reservation".into());
        }
    }
    let stored = row(&io)?;
    let expected = stored.expected().ok_or("identity missing")?;
    if expected.incarnation() != INC || expected.config() != harness::CFG {
        return Err(format!("stored {stored:?}"));
    }
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(
            "status v1 state=running run=run-1 boot=boot1 inc=inc2 cfg=cfg2 tart=up softnet=up",
        ),
    );
    let mut again = world.attempt();
    if again.recover(&cleanup_deadlines(&io.clock)).is_ok() || saw(&io, is_delete) {
        return Err("replacement body was deleted".into());
    }
    Ok(())
}

pub(super) fn committed_reserve_matches_the_next_gate() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_clear(&io);
    io.ledger
        .borrow_mut()
        .push_fault(LedgerFault::CommittedLost);
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
            harness::manifest(),
        ),
        Err(StepError::Ledger(LedgerError::ReplyLost))
    ) {
        return Err("committed reserve was acknowledged".into());
    }
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    let stored = row(&io)?;
    if !receipt.recovery_required()
        || receipt.cleanup_confirmed()
        || receipt.run().map(|id| id.as_str()) != Some(RUN)
        || receipt.clone_id().map(|id| id.as_str()) != Some(CLONE)
        || stored.is_sealed()
    {
        return Err(format!("receipt {receipt:?} row {stored:?}"));
    }
    drop(attempt);
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut again,
            &io.clock,
            RUN_2,
            harness::CLONE_2,
            absolute,
            false,
            true,
            harness::manifest(),
        ),
        Err(StepError::MustRecover)
    ) {
        return Err("next reserve did not require recovery".into());
    }
    Ok(())
}

pub(super) fn committed_seal_readback_finalizes() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Destroying)?;
    io.ledger
        .borrow_mut()
        .push_fault(LedgerFault::CommittedLost);
    let receipt = attempt
        .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
        .map_err(|err| format!("seal: {err:?}"))?;
    let stored = row(&io)?;
    if receipt.recovery_required()
        || !receipt.followups_allowed()
        || !receipt.cleanup_confirmed()
        || !matches!(receipt.outcome(), AttemptOutcome::Scored(TaskScore::Pass))
        || !stored.is_sealed()
    {
        return Err(format!("receipt {receipt:?} row {stored:?}"));
    }
    drop(attempt);
    push_clear(&io);
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    reserve_named(
        &mut again,
        &io.clock,
        RUN_2,
        harness::CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("next reserve: {err:?}"))?;
    Ok(())
}

pub(super) fn running_revoke_absent_list_is_not_cleanup() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    attempt
        .revoke(RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap())
        .map_err(|err| format!("revoke: {err:?}"))?;
    let before = trace_len(&io);
    harness::push_absent(&io);
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() {
        return Err("absent running vm was cleaned".into());
    }
    let receipt = super::seal_of(&mut attempt, &io)?;
    not_scored(&receipt)?;
    if receipt.cleanup_confirmed() || destructive_since(&io, before) {
        return Err(format!("confirmed or destroyed {receipt:?}"));
    }
    Ok(())
}

pub(super) fn recovery_of_started_vm_absent_from_the_list() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let attempt = super::drive(&mut world, &io, Phase::Running)?;
        drop(attempt);
    }
    let ledger_before = io.ledger.borrow().trace().len();
    let runner_before = trace_len(&io);
    let now = io.clock.now();
    let deadlines = CleanupDeadlines {
        query: QueryDeadline::after(now, Duration::from_secs(1)).unwrap(),
        ..cleanup_deadlines(&io.clock)
    };
    harness::push_absent(&io);
    let mut again = world.attempt();
    if again.recover(&deadlines).is_ok() {
        return Err("started absence was confirmed".into());
    }
    let receipt = again.receipt().cloned().ok_or("no receipt")?;
    not_scored(&receipt)?;
    let end = deadlines.query.at();
    let fresh = ledger_after(&io, ledger_before);
    if receipt.cleanup_confirmed()
        || !receipt.recovery_required()
        || destructive_since(&io, runner_before)
        || fresh.is_empty()
        || fresh.iter().any(|entry| entry.deadline > end)
    {
        return Err(format!("recovery {receipt:?} ledger {fresh:?}"));
    }
    Ok(())
}

pub(super) fn lease_expiry_during_stop_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    attempt
        .arm_owner_lease(
            gcu_eval::disposable::tart::LeaseDeadline::after(
                io.clock.now(),
                gcu_eval::disposable::tart::LeaseDeadline::CAP,
            )
            .unwrap(),
        )
        .map_err(|err| format!("arm: {err:?}"))?;
    push_guest(&io, ok_body("ok"));
    attempt
        .revoke(RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap())
        .map_err(|err| format!("revoke: {err:?}"))?;
    harness::advance(&io.clock, Duration::from_secs(2))?;
    io.runner
        .borrow_mut()
        .advance_after_stop(Duration::from_secs(2));
    harness::push_present(&io);
    let deadlines = cleanup_deadlines(&io.clock);
    let _ = attempt.destroy(&deadlines);
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    let stored = row(&io)?;
    if saw(&io, is_delete)
        || attempt.phase() != Phase::Suspended
        || !receipt.recovery_required()
        || stored.is_sealed()
    {
        return Err(format!(
            "phase {:?} delete {} receipt {receipt:?} row {stored:?}",
            attempt.phase(),
            saw(&io, is_delete)
        ));
    }
    Ok(())
}

pub(super) fn reserve_absolute_clips_ledger_and_list() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let absolute = AttemptDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    let end = absolute.at();
    push_clear(&io);
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
    let ledger = io.ledger.borrow().trace();
    let runner = io.runner.borrow().trace().to_vec();
    if ledger.is_empty()
        || ledger.iter().any(|entry| entry.deadline != end)
        || runner.is_empty()
        || runner.iter().any(|entry| entry.deadline != end)
        || !harness::guest_commands(&io).is_empty()
    {
        return Err(format!("ledger {ledger:?} runner {runner:?}"));
    }
    Ok(())
}

pub(super) fn expired_revoke_does_not_dispatch_fresh_io() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    attempt
        .arm_owner_lease(
            gcu_eval::disposable::tart::LeaseDeadline::after(
                io.clock.now(),
                gcu_eval::disposable::tart::LeaseDeadline::CAP,
            )
            .unwrap(),
        )
        .map_err(|err| format!("arm: {err:?}"))?;
    let revoke = RevokeDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    harness::advance(&io.clock, Duration::from_secs(3))?;
    let guests = harness::guest_commands(&io).len();
    let ledger = io.ledger.borrow().trace().len();
    if !matches!(attempt.revoke(revoke), Err(StepError::OwnerDead))
        || attempt.phase() != Phase::Suspended
    {
        return Err(format!("phase {:?}", attempt.phase()));
    }
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    if !receipt.recovery_required()
        || harness::guest_commands(&io).len() != guests
        || io.ledger.borrow().trace().len() != ledger
    {
        return Err(format!("fresh io receipt {receipt:?}"));
    }
    let stored = row(&io)?;
    if stored.facts().contains(&LedgerFact::CleanupUnknown) {
        return Err(format!("cleanup note was written: {stored:?}"));
    }
    Ok(())
}

pub(super) fn seal_readback_stays_inside_the_seal_deadline() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Destroying)?;
    let before = io.ledger.borrow().trace().len();
    let seal = SealDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    io.ledger
        .borrow_mut()
        .push_fault(LedgerFault::CommittedLost);
    let receipt = attempt.seal(seal).map_err(|err| format!("seal: {err:?}"))?;
    let fresh = ledger_after(&io, before);
    if !matches!(receipt.outcome(), AttemptOutcome::Scored(TaskScore::Pass))
        || receipt.recovery_required()
        || fresh.iter().any(|entry| entry.deadline > seal.at())
        || !fresh
            .iter()
            .any(|entry| matches!(entry.command, LedgerOp::Query) && entry.deadline == seal.at())
    {
        return Err(format!("receipt {receipt:?} ledger {fresh:?}"));
    }
    Ok(())
}

pub(super) fn seal_reply_lost_readback_is_not_final() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Destroying)?;
    let before = io.ledger.borrow().trace().len();
    let seal = SealDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    io.ledger.borrow_mut().push_fault(LedgerFault::ReplyLost);
    if attempt.seal(seal).is_ok() {
        return Err("unwritten seal was final".into());
    }
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    let fresh = ledger_after(&io, before);
    let stored = row(&io)?;
    not_scored(&receipt)?;
    if !receipt.recovery_required()
        || stored.is_sealed()
        || fresh.iter().any(|entry| entry.deadline > seal.at())
        || !fresh
            .iter()
            .any(|entry| matches!(entry.command, LedgerOp::Query))
    {
        return Err(format!(
            "receipt {receipt:?} row {stored:?} ledger {fresh:?}"
        ));
    }
    drop(attempt);
    let mut again = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut again,
            &io.clock,
            RUN_2,
            harness::CLONE_2,
            absolute,
            false,
            true,
            harness::manifest(),
        ),
        Err(StepError::MustRecover)
    ) {
        return Err("unsealed seal did not require recovery".into());
    }
    Ok(())
}

pub(super) fn run_after_ready_keeps_the_task_deadline() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Ready)?;
    push_guest(&io, ok_body("score pass"));
    let deadline = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
    attempt
        .run(deadline)
        .map_err(|err| format!("run: {err:?}"))?;
    let got = io
        .guest
        .borrow()
        .trace()
        .iter()
        .find(|entry| matches!(entry.command, gcu_eval::disposable::tart::GuestCommand::Run))
        .map(|entry| entry.deadline);
    if got != Some(deadline.at()) {
        return Err(format!("run deadline {got:?} expected {:?}", deadline.at()));
    }
    Ok(())
}

pub(super) fn post_stop_run_change_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
    super::push_until_stop(&io);
    push_runner(
        &io,
        ok_body(&format!(
            "status v1 state=stopped run={RUN_2} boot=boot1 inc={INC} cfg={} tart=exited softnet=exited",
            harness::CFG
        )),
    );
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::Cleanup(NotRunReason::IdentityConflict))
    ) || saw(&io, is_delete)
    {
        return Err("post-stop run change deleted".into());
    }
    Ok(())
}

pub(super) fn config_mismatch_does_not_stop() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(&format!(
            "status v1 state=running run={RUN} boot=boot1 inc={INC} cfg=cfg-other tart=up softnet=up"
        )),
    );
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::Cleanup(NotRunReason::IdentityConflict))
    ) || saw(&io, is_stop)
    {
        return Err("config mismatch stopped".into());
    }
    Ok(())
}

pub(super) fn softnet_still_up_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
    super::push_until_stop(&io);
    push_status(&io, "stopped", RUN, "exited", "up");
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::Cleanup(NotRunReason::CleanupFailed))
    ) || saw(&io, is_delete)
    {
        return Err("live softnet was deleted".into());
    }
    Ok(())
}

pub(super) fn drop_after_stop_recovers_without_a_score() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
        super::push_until_stop(&io);
        if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || !saw(&io, is_stop) {
            return Err("stop prefix did not stop".into());
        }
    }
    harness::push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    expect_not_run(&recovered[0], NotRunReason::HostCrash, false, true)?;
    if !recovered[0].cleanup_confirmed() || again.phase() != Phase::Idle {
        return Err(format!("recovered {recovered:?} phase {:?}", again.phase()));
    }
    push_clear(&io);
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    reserve_named(
        &mut again,
        &io.clock,
        RUN_2,
        harness::CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("new id: {err:?}"))?;
    Ok(())
}

pub(super) fn drop_after_delete_recovers_without_a_score() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
        super::push_until_stop(&io);
        push_status(&io, "stopped", RUN, "exited", "exited");
        push_runner(&io, ok_body("ok"));
        if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || !saw(&io, is_delete) {
            return Err("delete prefix did not delete".into());
        }
    }
    harness::push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    expect_not_run(&recovered[0], NotRunReason::HostCrash, false, true)?;
    if !recovered[0].cleanup_confirmed() {
        return Err(format!("unconfirmed {recovered:?}"));
    }
    Ok(())
}

pub(super) fn drop_after_committed_quarantine_halts_the_next_reserve() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
        push_runner(&io, ok_body("nope"));
        if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() {
            return Err("bad list was destroyed".into());
        }
        io.ledger
            .borrow_mut()
            .push_fault(LedgerFault::CommittedLost);
        if attempt
            .seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap())
            .is_ok()
        {
            return Err("committed quarantine was sealed".into());
        }
    }
    harness::push_present(&io);
    let mut again = world.attempt();
    let recovered = again
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    not_scored(&recovered[0])?;
    drop(again);
    let mut next = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if !matches!(
        reserve_named(
            &mut next,
            &io.clock,
            RUN_2,
            harness::CLONE_2,
            absolute,
            false,
            true,
            harness::manifest(),
        ),
        Err(StepError::Halted)
    ) {
        return Err("sealed quarantine did not halt".into());
    }
    Ok(())
}

fn above_one_second(nanos: u64) -> bool {
    nanos > 1_000_000_000
}

pub(super) fn prep_end_must_include_reserve() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Prep, Duration::from_secs(1))
        .unwrap();
    let (mut world, io) = opened(budgets);
    push_clear(&io);
    push_clear(&io);
    push_clone_ok(&io);
    io.lock
        .borrow_mut()
        .advance_on_acquire(Duration::from_millis(500));
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    harness::advance(&io.clock, Duration::from_millis(600))?;
    let cloned = attempt.clone_vm(
        gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
        CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap(),
    );
    let ledger_late = io
        .ledger
        .borrow()
        .trace()
        .iter()
        .any(|entry| above_one_second(entry.deadline.nanos()));
    let runner_late = io
        .runner
        .borrow()
        .trace()
        .iter()
        .any(|entry| above_one_second(entry.deadline.nanos()));
    if cloned.is_ok() || ledger_late || runner_late || saw(&io, is_clone) {
        return Err(format!(
            "prep=1s from reserve entry; at {}ns clone={cloned:?} ledger={:?} runner={:?}",
            io.clock.now().nanos(),
            io.ledger.borrow().trace(),
            io.runner.borrow().trace()
        ));
    }
    Ok(())
}

pub(super) fn unconfirmed_start_note_must_not_start() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let start = {
        let mut attempt = super::drive(&mut world, &io, Phase::Cloned)?;
        push_named(&io, CLONE);
        push_status(&io, "running", RUN, "up", "up");
        push_runner(&io, ok_body("ok"));
        push_clear(&io);
        io.ledger.borrow_mut().push_fault(LedgerFault::ReplyLost);
        io.ledger
            .borrow_mut()
            .push_read_fault(LedgerFault::TimedOut);
        attempt.start(
            &harness::flags(),
            StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap(),
        )
    };
    let stored = row(&io)?;
    let mut recovery = world.attempt();
    let receipts = recovery.recover(&cleanup_deadlines(&io.clock));
    if start.is_ok()
        || stored.facts().contains(&LedgerFact::StartIssued)
        || saw(&io, is_start)
        || receipts
            .as_ref()
            .is_ok_and(|items| items.iter().any(|receipt| receipt.cleanup_confirmed()))
    {
        return Err(format!(
            "unconfirmed start={start:?} facts={:?} recovery={receipts:?}",
            stored.facts()
        ));
    }
    Ok(())
}

pub(super) fn unreadable_identity_readback_does_not_clone() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_clear(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    push_clear(&io);
    push_clone_ok(&io);
    io.ledger
        .borrow_mut()
        .push_fault(LedgerFault::CommittedLost);
    io.ledger
        .borrow_mut()
        .push_read_fault(LedgerFault::TimedOut);
    let cloned = attempt.clone_vm(
        gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
        CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap(),
    );
    let stored = row(&io)?;
    let expected = stored.expected().ok_or("identity was not written")?;
    if cloned.is_ok()
        || saw(&io, is_clone)
        || expected.incarnation() != INC
        || expected.config() != harness::CFG
    {
        return Err(format!("clone={cloned:?} row={stored:?}"));
    }
    Ok(())
}

pub(super) fn lease_must_not_renew_after_inner_expiry() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    attempt
        .arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap())
        .map_err(|err| format!("arm: {err:?}"))?;
    harness::advance(&io.clock, Duration::from_millis(2500))?;
    io.lock
        .borrow_mut()
        .advance_on_acquire(Duration::from_secs(1));
    push_guest(&io, ok_body("ok"));
    let renewed =
        attempt.arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap());
    let leases: Vec<_> = io
        .guest
        .borrow()
        .trace()
        .iter()
        .filter(|entry| matches!(entry.command, GuestCommand::Lease { .. }))
        .cloned()
        .collect();
    if renewed.is_ok()
        || attempt.phase() != Phase::Suspended
        || leases.len() != 1
        || leases[0].deadline.nanos() != 3_000_000_000
    {
        return Err(format!(
            "renewed={renewed:?} phase={:?} leases={leases:?}",
            attempt.phase()
        ));
    }
    let run = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
    if !matches!(attempt.run(run), Err(StepError::Terminal))
        || !matches!(
            attempt.destroy(&cleanup_deadlines(&io.clock)),
            Err(StepError::DeleteForbidden)
        )
        || saw(&io, is_stop)
        || saw(&io, is_delete)
        || attempt.phase() != Phase::Suspended
    {
        return Err(format!("work after expiry phase {:?}", attempt.phase()));
    }
    Ok(())
}

pub(super) fn recovery_committed_seal_must_match_followup_gate() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_clear(&io);
    {
        let mut attempt = world.attempt();
        reserve_default(&mut attempt, &io.clock)?;
    }
    push_clear(&io);
    io.ledger
        .borrow_mut()
        .push_seal_fault(LedgerFault::CommittedLost);
    let mut attempt = world.attempt();
    let recovered = attempt
        .recover(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("recover: {err:?}"))?;
    let receipt = recovered.first().ok_or("no receipt")?;
    expect_not_run(receipt, NotRunReason::HostCrash, false, true)?;
    let stored = row(&io)?;
    if !receipt.cleanup_confirmed()
        || receipt.recovery_required()
        || attempt.phase() != Phase::Idle
        || attempt.receipt().is_some()
        || !stored.is_sealed()
        || stored.facts().contains(&LedgerFact::Quarantined)
    {
        return Err(format!(
            "receipt={receipt:?} phase={:?} row={stored:?}",
            attempt.phase()
        ));
    }
    drop(attempt);
    push_clear(&io);
    let mut next = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    reserve_named(
        &mut next,
        &io.clock,
        RUN_2,
        harness::CLONE_2,
        absolute,
        false,
        true,
        harness::manifest(),
    )
    .map_err(|err| format!("next reserve: {err:?}"))?;
    Ok(())
}

pub(super) fn tart_still_up_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = super::drive(&mut world, &io, Phase::Revoking)?;
    super::push_until_stop(&io);
    push_status(&io, "stopped", RUN, "up", "exited");
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::Cleanup(NotRunReason::CleanupFailed))
    ) || saw(&io, is_delete)
    {
        return Err("live tart was deleted".into());
    }
    Ok(())
}

pub(super) fn login_dispatch_stays_inside_prep() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Prep, Duration::from_secs(1))
        .unwrap();
    let (mut world, io) = opened(budgets);
    let mut attempt = super::drive(&mut world, &io, Phase::Bootstrapped)?;
    push_guest(&io, ok_body("login v1 ready"));
    let caller = LoginDeadline::after(io.clock.now(), Duration::from_secs(30)).unwrap();
    attempt
        .login(caller)
        .map_err(|err| format!("login: {err:?}"))?;
    let got = io
        .guest
        .borrow()
        .trace()
        .iter()
        .find(|entry| matches!(entry.command, GuestCommand::Login))
        .map(|entry| entry.deadline.nanos());
    if got != Some(1_000_000_000) {
        return Err(format!("login deadline {got:?} caller {:?}", caller.at()));
    }
    drop(attempt);
    let (mut world, io) = opened(budgets);
    let mut attempt = super::drive(&mut world, &io, Phase::Bootstrapped)?;
    harness::advance(&io.clock, Duration::from_secs(1))?;
    let before = io.guest.borrow().trace().len();
    let caller = LoginDeadline::after(io.clock.now(), Duration::from_secs(30)).unwrap();
    if !matches!(attempt.login(caller), Err(StepError::Expired))
        || io.guest.borrow().trace().len() != before
    {
        return Err("login after prep end was dispatched".into());
    }
    Ok(())
}

pub(super) fn clone_config_mismatch_is_not_accepted() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_clear(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    push_clear(&io);
    push_runner(&io, ok_body(&format!("clone v1 inc={INC} cfg=cfg-other")));
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    if !matches!(
        attempt.clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            deadline
        ),
        Err(StepError::Cleanup(NotRunReason::IdentityConflict))
    ) {
        return Err("config-only body was accepted".into());
    }
    let stored = row(&io)?;
    let expected = stored.expected().ok_or("identity missing")?;
    if expected.incarnation() != INC
        || expected.config() != harness::CFG
        || attempt.phase() == Phase::Cloned
    {
        return Err(format!("stored {stored:?} phase {:?}", attempt.phase()));
    }
    Ok(())
}
