//! Negative checks for the S11b review. Each one fails if its fix is removed.

use std::time::Duration;

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptLedger, AttemptOutcome, AttemptReceipt, BudgetField, CleanupDeadlines,
    CloneDeadline, DeleteDeadline, GuestCommand, HostClock, LeaseDeadline, LedgerFact, LedgerFault,
    NotRunReason, Phase, PhaseBudgets, QueryDeadline, RevokeDeadline, RunDeadline, SealDeadline,
    StartDeadline, StepError, StopDeadline, TartCommand,
};

use super::faults::{expect_not_run, expect_quarantine, stopped};
use super::harness::{
    self, cleanup_deadlines, drive_to, finish, id_clone, ok_body, push_clear, push_clone_ok,
    push_guest, push_named, push_runner, push_status, query, reserve_default, reserve_named,
    runner_commands, scripts_to_reach, start_ids, CFG, CLONE, INC, RUN, RUN_2,
};

#[path = "review_fix.rs"]
mod fix;

#[allow(
    clippy::type_complexity,
    reason = "one table of named checks; a type alias would not make the pointer simpler"
)]
pub(super) const TESTS: &[(&str, fn() -> Result<(), String>)] = &[
    (
        "same_name_destination_blocks_reserve",
        same_name_destination_blocks_reserve,
    ),
    (
        "same_name_destination_blocks_clone",
        same_name_destination_blocks_clone,
    ),
    (
        "replacement_incarnation_is_not_stopped",
        replacement_incarnation_is_not_stopped,
    ),
    (
        "start_rejects_replacement_incarnation",
        start_rejects_replacement_incarnation,
    ),
    (
        "boot_change_during_cleanup_is_not_scored",
        boot_change_during_cleanup_is_not_scored,
    ),
    (
        "lost_clone_reply_reclaims_the_same_incarnation",
        fix::lost_clone_reply_reclaims_the_same_incarnation,
    ),
    (
        "lost_clone_reply_refuses_a_replacement",
        fix::lost_clone_reply_refuses_a_replacement,
    ),
    (
        "unwritten_identity_does_not_clone_or_delete",
        fix::unwritten_identity_does_not_clone_or_delete,
    ),
    (
        "contradictory_absent_status_is_malformed",
        contradictory_absent_status_is_malformed,
    ),
    (
        "stopped_without_exit_proof_does_not_delete",
        stopped_without_exit_proof_does_not_delete,
    ),
    (
        "fieldless_absent_status_does_not_prove_stop",
        fieldless_absent_status_does_not_prove_stop,
    ),
    (
        "illegal_run_while_revoking_stays_not_run",
        illegal_run_while_revoking_stays_not_run,
    ),
    (
        "illegal_start_while_destroying_stays_not_run",
        illegal_start_while_destroying_stays_not_run,
    ),
    (
        "late_renew_blocks_revoke_destroy_and_seal",
        late_renew_blocks_revoke_destroy_and_seal,
    ),
    (
        "skipped_observe_suspends_on_the_next_entry",
        skipped_observe_suspends_on_the_next_entry,
    ),
    (
        "expired_observe_deadline_still_suspends",
        expired_observe_deadline_still_suspends,
    ),
    (
        "prep_end_clips_the_clone_deadline",
        prep_end_clips_the_clone_deadline,
    ),
    (
        "cleanup_end_clips_the_stop_deadline",
        cleanup_end_clips_the_stop_deadline,
    ),
    (
        "post_stop_status_uses_the_stop_deadline",
        post_stop_status_uses_the_stop_deadline,
    ),
    (
        "post_delete_list_uses_the_delete_deadline",
        post_delete_list_uses_the_delete_deadline,
    ),
    (
        "lock_busy_after_running_stays_quarantined",
        lock_busy_after_running_stays_quarantined,
    ),
    (
        "lock_busy_after_revoking_stays_quarantined",
        lock_busy_after_revoking_stays_quarantined,
    ),
    (
        "post_stop_identity_change_does_not_delete",
        post_stop_identity_change_does_not_delete,
    ),
    (
        "list_without_end_marker_is_not_absence",
        list_without_end_marker_is_not_absence,
    ),
    (
        "confirmation_without_end_marker_is_not_absence",
        confirmation_without_end_marker_is_not_absence,
    ),
    (
        "recovery_stop_uses_the_caller_deadline",
        recovery_stop_uses_the_caller_deadline,
    ),
    (
        "expired_recovery_stop_does_not_stop",
        expired_recovery_stop_does_not_stop,
    ),
    (
        "committed_identity_rejects_a_replacement_body",
        fix::committed_identity_rejects_a_replacement_body,
    ),
    (
        "committed_reserve_matches_the_next_gate",
        fix::committed_reserve_matches_the_next_gate,
    ),
    (
        "stop_without_confirmation_does_not_delete",
        stop_without_confirmation_does_not_delete,
    ),
    (
        "delete_without_confirmation_is_not_absence",
        delete_without_confirmation_is_not_absence,
    ),
    (
        "committed_quarantine_still_requires_recovery",
        committed_quarantine_still_requires_recovery,
    ),
    (
        "committed_seal_readback_finalizes",
        fix::committed_seal_readback_finalizes,
    ),
    (
        "running_revoke_absent_list_is_not_cleanup",
        fix::running_revoke_absent_list_is_not_cleanup,
    ),
    (
        "recovery_of_started_vm_absent_from_the_list",
        fix::recovery_of_started_vm_absent_from_the_list,
    ),
    (
        "lease_expiry_during_stop_does_not_delete",
        fix::lease_expiry_during_stop_does_not_delete,
    ),
    (
        "reserve_absolute_clips_ledger_and_list",
        fix::reserve_absolute_clips_ledger_and_list,
    ),
    (
        "expired_revoke_does_not_dispatch_fresh_io",
        fix::expired_revoke_does_not_dispatch_fresh_io,
    ),
    (
        "seal_readback_stays_inside_the_seal_deadline",
        fix::seal_readback_stays_inside_the_seal_deadline,
    ),
    (
        "seal_reply_lost_readback_is_not_final",
        fix::seal_reply_lost_readback_is_not_final,
    ),
    (
        "run_after_ready_keeps_the_task_deadline",
        fix::run_after_ready_keeps_the_task_deadline,
    ),
    (
        "post_stop_run_change_does_not_delete",
        fix::post_stop_run_change_does_not_delete,
    ),
    (
        "config_mismatch_does_not_stop",
        fix::config_mismatch_does_not_stop,
    ),
    (
        "softnet_still_up_does_not_delete",
        fix::softnet_still_up_does_not_delete,
    ),
    (
        "drop_after_stop_recovers_without_a_score",
        fix::drop_after_stop_recovers_without_a_score,
    ),
    (
        "drop_after_delete_recovers_without_a_score",
        fix::drop_after_delete_recovers_without_a_score,
    ),
    (
        "drop_after_committed_quarantine_halts_the_next_reserve",
        fix::drop_after_committed_quarantine_halts_the_next_reserve,
    ),
    ("drop_leaves_the_vm_running", drop_leaves_the_vm_running),
];

fn opened(budgets: PhaseBudgets) -> (harness::World, harness::Handles) {
    let world = harness::World::new(budgets);
    let io = world.open();
    (world, io)
}

fn drive<'a>(
    world: &'a mut harness::World,
    io: &harness::Handles,
    target: Phase,
) -> Result<harness::Machine<'a>, String> {
    scripts_to_reach(io, target);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, io, target)?;
    Ok(attempt)
}

fn not_scored(receipt: &AttemptReceipt) -> Result<(), String> {
    if receipt.outcome().score().is_some() {
        Err(format!("scored {receipt:?}"))
    } else {
        Ok(())
    }
}

fn saw(io: &harness::Handles, kind: fn(&TartCommand) -> bool) -> bool {
    runner_commands(io).iter().any(kind)
}

fn is_stop(command: &TartCommand) -> bool {
    matches!(command, TartCommand::Stop { .. })
}

fn is_delete(command: &TartCommand) -> bool {
    matches!(command, TartCommand::Delete { .. })
}

fn is_clone(command: &TartCommand) -> bool {
    matches!(command, TartCommand::Clone { .. })
}

fn is_start(command: &TartCommand) -> bool {
    matches!(command, TartCommand::Start { .. })
}

fn push_until_stop(io: &harness::Handles) {
    push_named(io, CLONE);
    push_status(io, "running", RUN, "up", "up");
    push_runner(io, ok_body("ok"));
}

fn status_text(state: &str, boot: &str, inc: &str, tart: &str, softnet: &str) -> String {
    format!(
        "status v1 state={state} run={RUN} boot={boot} inc={inc} cfg={CFG} tart={tart} softnet={softnet}"
    )
}

fn seal_of(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
) -> Result<AttemptReceipt, String> {
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    attempt
        .seal(deadline)
        .map_err(|err| format!("seal: {err:?}"))
}

fn rows(io: &harness::Handles) -> Result<Vec<gcu_eval::disposable::tart::Reservation>, String> {
    io.ledger
        .borrow()
        .rows(query(&io.clock))
        .map_err(|err| format!("{err:?}"))
}

fn same_name_destination_blocks_reserve() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_named(&io, CLONE);
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
    if !matches!(err, Err(StepError::DestinationExists)) || attempt.phase() != Phase::Idle {
        return Err(format!("reserve {err:?} phase {:?}", attempt.phase()));
    }
    if !rows(&io)?.is_empty() || saw(&io, is_clone) {
        return Err("same-name destination was reserved".into());
    }
    Ok(())
}

fn same_name_destination_blocks_clone() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    push_clear(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    push_named(&io, CLONE);
    let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
    let err = attempt.clone_vm(
        gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
        deadline,
    );
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::IdentityConflict))) || saw(&io, is_clone)
    {
        return Err(format!("clone {err:?}"));
    }
    Ok(())
}

fn replacement_incarnation_is_not_stopped() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(&status_text("running", "boot1", "inc2", "up", "up")),
    );
    push_runner(&io, ok_body("ok"));
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || saw(&io, is_stop) {
        return Err("replacement vm was stopped".into());
    }
    Ok(())
}

fn start_rejects_replacement_incarnation() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Cloned)?;
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(&status_text("running", "boot1", "inc2", "up", "up")),
    );
    push_runner(&io, ok_body("ok"));
    let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
    let err = attempt.start(&harness::flags(), deadline);
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::IdentityConflict))) || saw(&io, is_start)
    {
        return Err(format!("start {err:?} ids {:?}", start_ids(&io)));
    }
    Ok(())
}

fn boot_change_during_cleanup_is_not_scored() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_named(&io, CLONE);
    push_runner(
        &io,
        ok_body(&status_text("running", "boot2", INC, "up", "up")),
    );
    push_runner(&io, ok_body("ok"));
    push_runner(
        &io,
        ok_body(&status_text("stopped", "boot2", INC, "exited", "exited")),
    );
    push_runner(&io, ok_body("ok"));
    push_clear(&io);
    attempt
        .destroy(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("destroy: {err:?}"))?;
    if !saw(&io, is_delete) {
        return Err("owned vm was not destroyed".into());
    }
    let receipt = seal_of(&mut attempt, &io)?;
    expect_not_run(&receipt, NotRunReason::BootChanged, true, true)?;
    not_scored(&receipt)
}

fn contradictory_absent_status_is_malformed() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_runner(&io, ok_body("status v1 state=absent tart=up softnet=up"));
    push_runner(&io, ok_body("ok"));
    let err = attempt.destroy(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::Malformed))) || saw(&io, is_delete) {
        return Err(format!("contradictory absent: {err:?}"));
    }
    Ok(())
}

fn stopped_without_exit_proof_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_runner(
        &io,
        ok_body(&format!(
            "status v1 state=stopped run={RUN} boot=boot1 inc={INC} cfg={CFG}"
        )),
    );
    push_runner(&io, ok_body("ok"));
    let err = attempt.destroy(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::Malformed))) || saw(&io, is_delete) {
        return Err(format!("missing exit proof: {err:?}"));
    }
    Ok(())
}

fn fieldless_absent_status_does_not_prove_stop() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_runner(&io, ok_body("status v1 state=absent"));
    push_runner(&io, ok_body("ok"));
    let err = attempt.destroy(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::CleanupFailed))) || saw(&io, is_delete) {
        return Err(format!("fieldless absent: {err:?}"));
    }
    Ok(())
}

fn illegal_run_while_revoking_stays_not_run() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    let deadline = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
    if !matches!(attempt.run(deadline), Err(StepError::Unexpected)) {
        return Err("run during revoke was accepted".into());
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::Unexpected, true, true)?;
    not_scored(&receipt)
}

fn illegal_start_while_destroying_stays_not_run() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Destroying)?;
    let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
    if !matches!(
        attempt.start(&harness::flags(), deadline),
        Err(StepError::Unexpected)
    ) {
        return Err("start during destroy was accepted".into());
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::Unexpected, true, true)?;
    not_scored(&receipt)
}

fn arm_and_expire(attempt: &mut harness::Machine<'_>, io: &harness::Handles) -> Result<(), String> {
    push_guest(io, ok_body("ok"));
    attempt
        .arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap())
        .map_err(|err| format!("arm: {err:?}"))?;
    harness::advance(&io.clock, Duration::from_secs(3))
}

fn lease_count(io: &harness::Handles) -> usize {
    harness::guest_commands(io)
        .iter()
        .filter(|command| matches!(command, GuestCommand::Lease { .. }))
        .count()
}

fn late_renew_blocks_revoke_destroy_and_seal() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Running)?;
    arm_and_expire(&mut attempt, &io)?;
    if !matches!(
        attempt.arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap()),
        Err(StepError::LeaseExpired)
    ) || attempt.phase() != Phase::Suspended
        || lease_count(&io) != 1
    {
        return Err(format!("renew phase {:?}", attempt.phase()));
    }
    if !matches!(
        attempt.arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap()),
        Err(StepError::Terminal)
    ) || lease_count(&io) != 1
        || attempt.phase() != Phase::Suspended
    {
        return Err(format!("rearm phase {:?}", attempt.phase()));
    }
    harness::push_present(&io);
    push_guest(&io, ok_body("score pass"));
    let run = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    if !matches!(attempt.run(run), Err(StepError::Terminal))
        || !matches!(
            attempt.observe_owner_lease(query(&io.clock)),
            Err(StepError::Terminal)
        )
        || !matches!(attempt.revoke(revoke), Err(StepError::Terminal))
        || !matches!(
            attempt.destroy(&cleanup_deadlines(&io.clock)),
            Err(StepError::DeleteForbidden)
        )
        || !matches!(
            attempt.seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap()),
            Err(StepError::Terminal)
        )
        || attempt.phase() != Phase::Suspended
        || lease_count(&io) != 1
        || saw(&io, is_stop)
    {
        return Err(format!("revived phase {:?}", attempt.phase()));
    }
    not_scored(attempt.receipt().ok_or("missing receipt")?)
}

fn skipped_observe_suspends_on_the_next_entry() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Running)?;
    arm_and_expire(&mut attempt, &io)?;
    harness::push_present(&io);
    let revoke = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    if !matches!(attempt.revoke(revoke), Err(StepError::OwnerDead))
        || attempt.phase() != Phase::Suspended
    {
        return Err(format!("observe skip phase {:?}", attempt.phase()));
    }
    if !matches!(
        attempt.destroy(&cleanup_deadlines(&io.clock)),
        Err(StepError::DeleteForbidden)
    ) || saw(&io, is_stop)
    {
        return Err("skipped observe still deleted".into());
    }
    not_scored(attempt.receipt().ok_or("missing receipt")?)
}

fn expired_observe_deadline_still_suspends() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Running)?;
    push_guest(&io, ok_body("ok"));
    attempt
        .arm_owner_lease(LeaseDeadline::after(io.clock.now(), LeaseDeadline::CAP).unwrap())
        .map_err(|err| format!("arm: {err:?}"))?;
    let deadline = QueryDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    harness::advance(&io.clock, Duration::from_secs(3))?;
    if !matches!(
        attempt.observe_owner_lease(deadline),
        Err(StepError::OwnerDead)
    ) || attempt.phase() != Phase::Suspended
        || saw(&io, is_stop)
    {
        return Err(format!("stale observe phase {:?}", attempt.phase()));
    }
    not_scored(attempt.receipt().ok_or("missing receipt")?)
}

fn prep_end_clips_the_clone_deadline() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Prep, Duration::from_secs(1))
        .unwrap();
    let (mut world, io) = opened(budgets);
    push_clear(&io);
    let mut attempt = world.attempt();
    reserve_default(&mut attempt, &io.clock)?;
    let origin = io.clock.now();
    harness::advance(&io.clock, Duration::from_millis(500))?;
    push_clear(&io);
    push_clone_ok(&io);
    let requested = CloneDeadline::after(io.clock.now(), Duration::from_secs(60)).unwrap();
    attempt
        .clone_vm(
            gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            requested,
        )
        .map_err(|err| format!("clone: {err:?}"))?;
    let expected = CloneDeadline::after(origin, Duration::from_secs(1))
        .unwrap()
        .at();
    let clone = io
        .runner
        .borrow()
        .trace()
        .iter()
        .find(|entry| is_clone(&entry.command))
        .cloned();
    if clone.as_ref().map(|entry| entry.deadline) != Some(expected) {
        return Err(format!("clone deadline {clone:?} expected {expected:?}"));
    }
    Ok(())
}

fn cleanup_end_clips_the_stop_deadline() -> Result<(), String> {
    let budgets = PhaseBudgets::adr()
        .shrink(BudgetField::Cleanup, Duration::from_secs(2))
        .unwrap();
    let (mut world, io) = opened(budgets);
    let mut attempt = drive(&mut world, &io, Phase::Running)?;
    let started = io.clock.now();
    push_guest(&io, ok_body("ok"));
    attempt
        .revoke(RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap())
        .map_err(|err| format!("revoke: {err:?}"))?;
    let revoke_at = RevokeDeadline::after(started, Duration::from_secs(2))
        .unwrap()
        .at();
    let revoke = io
        .guest
        .borrow()
        .trace()
        .iter()
        .find(|entry| matches!(entry.command, GuestCommand::Revoke))
        .map(|entry| entry.deadline);
    if revoke != Some(revoke_at) {
        return Err(format!("revoke deadline {revoke:?} expected {revoke_at:?}"));
    }
    harness::advance(&io.clock, Duration::from_millis(1500))?;
    harness::push_present(&io);
    attempt
        .destroy(&cleanup_deadlines(&io.clock))
        .map_err(|err| format!("destroy: {err:?}"))?;
    let expected = StopDeadline::after(started, Duration::from_secs(2))
        .unwrap()
        .at();
    let stop = io
        .runner
        .borrow()
        .trace()
        .iter()
        .find(|entry| is_stop(&entry.command))
        .cloned();
    if stop.as_ref().map(|entry| entry.deadline) != Some(expected) {
        return Err(format!("stop deadline {stop:?} expected {expected:?}"));
    }
    Ok(())
}

fn post_stop_status_uses_the_stop_deadline() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    io.runner
        .borrow_mut()
        .advance_after_stop(Duration::from_secs(2));
    harness::push_present(&io);
    let now = io.clock.now();
    let deadlines = CleanupDeadlines {
        query: QueryDeadline::after(now, Duration::from_secs(5)).unwrap(),
        stop: StopDeadline::after(now, Duration::from_secs(1)).unwrap(),
        delete: DeleteDeadline::after(now, DeleteDeadline::CAP).unwrap(),
        seal: SealDeadline::after(now, SealDeadline::CAP).unwrap(),
        revoke: RevokeDeadline::after(now, RevokeDeadline::CAP).unwrap(),
    };
    if attempt.destroy(&deadlines).is_ok() || saw(&io, is_delete) {
        return Err("status ran past the stop deadline".into());
    }
    Ok(())
}

fn post_delete_list_uses_the_delete_deadline() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    io.runner
        .borrow_mut()
        .advance_after_delete(Duration::from_secs(2));
    harness::push_present(&io);
    let now = io.clock.now();
    let deadlines = CleanupDeadlines {
        query: QueryDeadline::after(now, Duration::from_secs(5)).unwrap(),
        stop: StopDeadline::after(now, StopDeadline::CAP).unwrap(),
        delete: DeleteDeadline::after(now, Duration::from_secs(1)).unwrap(),
        seal: SealDeadline::after(now, SealDeadline::CAP).unwrap(),
        revoke: RevokeDeadline::after(now, RevokeDeadline::CAP).unwrap(),
    };
    if attempt.destroy(&deadlines).is_ok() {
        return Err("absence check ran past the delete deadline".into());
    }
    if !saw(&io, is_delete) {
        return Err("delete was not issued".into());
    }
    Ok(())
}

fn lock_busy_after_running_stays_quarantined() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Running)?;
    io.lock
        .borrow_mut()
        .hold_forever()
        .map_err(|err| format!("hold: {err:?}"))?;
    let err = attempt.revoke(RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap());
    if !matches!(err, Err(StepError::LockBusy)) || attempt.phase() != Phase::Quarantined {
        return Err(format!("lock {err:?} phase {:?}", attempt.phase()));
    }
    expect_quarantine(
        attempt.receipt().ok_or("missing receipt")?,
        NotRunReason::LockBusy,
    )
}

fn lock_busy_after_revoking_stays_quarantined() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    io.lock
        .borrow_mut()
        .hold_forever()
        .map_err(|err| format!("hold: {err:?}"))?;
    let err = attempt.destroy(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::LockBusy)) || attempt.phase() != Phase::Quarantined {
        return Err(format!("lock {err:?} phase {:?}", attempt.phase()));
    }
    expect_quarantine(
        attempt.receipt().ok_or("missing receipt")?,
        NotRunReason::LockBusy,
    )
}

fn post_stop_identity_change_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_runner(
        &io,
        ok_body(&status_text("stopped", "boot1", "inc2", "exited", "exited")),
    );
    push_runner(&io, ok_body("ok"));
    let err = attempt.destroy(&cleanup_deadlines(&io.clock));
    if !matches!(err, Err(StepError::Cleanup(NotRunReason::IdentityConflict)))
        || saw(&io, is_delete)
    {
        return Err(format!("post-stop identity: {err:?}"));
    }
    Ok(())
}

fn list_without_end_marker_is_not_absence() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_runner(&io, ok_body("list v1\ncount 0\nNOTEND\n"));
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || saw(&io, is_stop) {
        return Err("broken list was treated as absence".into());
    }
    let receipt = seal_of(&mut attempt, &io)?;
    if matches!(receipt.outcome(), AttemptOutcome::Scored(_)) {
        return Err("broken list kept a score".into());
    }
    Ok(())
}

fn confirmation_without_end_marker_is_not_absence() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_status(&io, "stopped", RUN, "exited", "exited");
    push_runner(&io, ok_body("ok"));
    push_runner(&io, ok_body("list v1\ncount 0\nNOTEND\n"));
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() {
        return Err("broken confirmation was absence".into());
    }
    if !saw(&io, is_delete) {
        return Err("delete did not reach confirmation".into());
    }
    let receipt = seal_of(&mut attempt, &io)?;
    not_scored(&receipt)
}

fn recovery_stop_uses_the_caller_deadline() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let attempt = drive(&mut world, &io, Phase::Cloned)?;
        drop(attempt);
    }
    let stop = StopDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    let expected = stop.at();
    let deadlines = CleanupDeadlines {
        stop,
        ..cleanup_deadlines(&io.clock)
    };
    harness::push_present(&io);
    let mut again = world.attempt();
    again
        .recover(&deadlines)
        .map_err(|err| format!("recover: {err:?}"))?;
    let got = io
        .runner
        .borrow()
        .trace()
        .iter()
        .find(|entry| is_stop(&entry.command))
        .map(|entry| entry.deadline);
    if got != Some(expected) {
        return Err(format!("recovery stop {got:?} expected {expected:?}"));
    }
    Ok(())
}

fn expired_recovery_stop_does_not_stop() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let attempt = drive(&mut world, &io, Phase::Cloned)?;
        drop(attempt);
    }
    let stop = StopDeadline::after(io.clock.now(), Duration::from_secs(1)).unwrap();
    harness::advance(&io.clock, Duration::from_secs(1))?;
    let deadlines = CleanupDeadlines {
        stop,
        ..cleanup_deadlines(&io.clock)
    };
    harness::push_present(&io);
    let mut again = world.attempt();
    if again.recover(&deadlines).is_ok() || saw(&io, is_stop) {
        return Err("expired recovery stop was issued".into());
    }
    Ok(())
}

fn stop_without_confirmation_does_not_delete() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok()
        || !saw(&io, is_stop)
        || saw(&io, is_delete)
    {
        return Err("unconfirmed stop deleted".into());
    }
    Ok(())
}

fn delete_without_confirmation_is_not_absence() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
    push_until_stop(&io);
    push_status(&io, "stopped", RUN, "exited", "exited");
    push_runner(&io, ok_body("ok"));
    if attempt.destroy(&cleanup_deadlines(&io.clock)).is_ok() || !saw(&io, is_delete) {
        return Err("unconfirmed delete was absence".into());
    }
    Ok(())
}

fn committed_quarantine_still_requires_recovery() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    let mut attempt = drive(&mut world, &io, Phase::Revoking)?;
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
        return Err("committed-lost seal was acknowledged".into());
    }
    let receipt = attempt.receipt().cloned().ok_or("no receipt")?;
    let row = io
        .ledger
        .borrow()
        .query(&id_clone(CLONE), query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    if !receipt.recovery_required()
        || row.is_sealed()
        || !row.facts().contains(&LedgerFact::Quarantined)
    {
        return Err(format!("receipt {receipt:?} row {row:?}"));
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
            harness::manifest()
        ),
        Err(StepError::MustRecover)
    ) {
        return Err("committed quarantine did not force recovery".into());
    }
    Ok(())
}

fn drop_leaves_the_vm_running() -> Result<(), String> {
    let (mut world, io) = opened(PhaseBudgets::adr());
    {
        let attempt = drive(&mut world, &io, Phase::Running)?;
        if attempt.phase() != Phase::Running {
            return Err(format!("phase {:?}", attempt.phase()));
        }
    }
    if stopped(&io) {
        return Err("drop stopped the vm".into());
    }
    Ok(())
}
