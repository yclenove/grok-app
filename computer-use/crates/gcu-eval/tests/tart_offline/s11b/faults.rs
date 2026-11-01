//! Fault table and evidence prefixes. Fake clock only.

use gcu_eval::disposable::tart::{
    AttemptLedger, AttemptOutcome, AttemptReceipt, GroupDeadline, GuestCommand, HostClock,
    LedgerFact, NotRunReason, Phase, PhaseBudgets, RevokeDeadline, RuntimeLoss, SealDeadline,
    StartDeadline, StepError, TartCommand, TaskScore,
};
use gcu_eval::disposable::{CleanupDisposition, Probe};

use super::harness::{
    self, assert_ids, cleanup_deadlines, drive_to, finish, guest_commands, id_clone, ok_body,
    push_guest, push_isolate, query, run_count, runner_commands, scripts_to_reach, start_ids,
    CLONE, RUN,
};

#[allow(
    clippy::type_complexity,
    reason = "one table of named checks; a type alias would not make the pointer simpler"
)]
pub(super) const TESTS: &[(&str, fn() -> Result<(), String>)] = &[
    (
        "success_trace_keeps_pass_and_rejects_later_events",
        success_trace_keeps_pass_and_rejects_later_events,
    ),
    (
        "unexpected_start_from_idle_does_not_clone",
        unexpected_start_from_idle_does_not_clone,
    ),
    (
        "second_run_stays_not_run_after_seal",
        second_run_stays_not_run_after_seal,
    ),
    (
        "second_revoke_does_not_send_again",
        second_revoke_does_not_send_again,
    ),
    (
        "failed_probe_is_not_run_without_a_client",
        failed_probe_is_not_run_without_a_client,
    ),
    (
        "unknown_probes_block_ready_without_a_client",
        unknown_probes_block_ready_without_a_client,
    ),
    ("foreign_ack_is_missing_proof", foreign_ack_is_missing_proof),
    (
        "missing_login_is_missing_proof",
        missing_login_is_missing_proof,
    ),
    ("boot_change_is_not_adopted", boot_change_is_not_adopted),
    (
        "provider_crash_stays_not_run_after_cleanup",
        provider_crash_stays_not_run_after_cleanup,
    ),
    ("guest_crash_does_not_restart", guest_crash_does_not_restart),
    ("tart_exit_does_not_restart", tart_exit_does_not_restart),
    (
        "softnet_exit_does_not_restart",
        softnet_exit_does_not_restart,
    ),
    (
        "owner_death_leaves_cleanup_unknown_until_recovery",
        super::recovery::owner_death_leaves_cleanup_unknown_until_recovery,
    ),
    (
        "host_crash_recovers_orphan_without_reviving_the_score",
        super::recovery::host_crash_recovers_orphan_without_reviving_the_score,
    ),
    (
        "recovery_deletes_after_the_absolute_deadline",
        super::recovery::recovery_deletes_after_the_absolute_deadline,
    ),
    (
        "listed_absence_does_not_stop_or_delete",
        super::recovery::listed_absence_does_not_stop_or_delete,
    ),
    (
        "status_timeout_does_not_delete_and_halts",
        super::recovery::status_timeout_does_not_delete_and_halts,
    ),
    (
        "identity_conflict_does_not_stop_or_delete",
        super::recovery::identity_conflict_does_not_stop_or_delete,
    ),
    (
        "malformed_list_is_not_absence",
        super::recovery::malformed_list_is_not_absence,
    ),
    (
        "delete_confirmation_still_present_is_quarantine",
        super::recovery::delete_confirmation_still_present_is_quarantine,
    ),
    (
        "seal_fault_drops_the_score_and_requires_recovery",
        super::recovery::seal_fault_drops_the_score_and_requires_recovery,
    ),
    (
        "lock_busy_returns_without_waiting_or_cloning",
        super::recovery::lock_busy_returns_without_waiting_or_cloning,
    ),
    (
        "shrunk_clone_budget_rejects_without_consuming_the_script",
        super::recovery::shrunk_clone_budget_rejects_without_consuming_the_script,
    ),
    (
        "shrunk_prep_budget_expires_without_a_clone",
        super::recovery::shrunk_prep_budget_expires_without_a_clone,
    ),
    (
        "absolute_deadline_clips_the_recorded_clone_deadline",
        super::recovery::absolute_deadline_clips_the_recorded_clone_deadline,
    ),
    (
        "stop_latch_refuses_followups",
        super::recovery::stop_latch_refuses_followups,
    ),
    (
        "manifest_mismatch_is_not_run_without_a_clone",
        super::recovery::manifest_mismatch_is_not_run_without_a_clone,
    ),
    (
        "unsealed_row_must_be_recovered_before_reserve",
        super::recovery::unsealed_row_must_be_recovered_before_reserve,
    ),
    (
        "sealed_quarantine_halts_reserve_and_recover_skips_it",
        super::recovery::sealed_quarantine_halts_reserve_and_recover_skips_it,
    ),
    (
        "late_owner_lease_does_not_extend",
        super::recovery::late_owner_lease_does_not_extend,
    ),
    (
        "live_owner_lease_does_not_revoke",
        super::recovery::live_owner_lease_does_not_revoke,
    ),
    (
        "shrunk_probe_group_rejects_before_inventory",
        super::recovery::shrunk_probe_group_rejects_before_inventory,
    ),
    (
        "budget_shrink_rejects_zero_and_above_cap",
        super::recovery::budget_shrink_rejects_zero_and_above_cap,
    ),
];

pub(super) fn expect_not_run(
    receipt: &AttemptReceipt,
    reason: NotRunReason,
    client: bool,
    follow: bool,
) -> Result<(), String> {
    match receipt.outcome() {
        AttemptOutcome::NotRun {
            reason: got,
            client_started,
        } if *got == reason && *client_started == client => {}
        other => return Err(format!("outcome {other:?}")),
    }
    if receipt.followups_allowed() != follow || receipt.outcome().score().is_some() {
        return Err(format!(
            "follow {follow} score {:?}",
            receipt.outcome().score()
        ));
    }
    Ok(())
}

pub(super) fn expect_quarantine(
    receipt: &AttemptReceipt,
    reason: NotRunReason,
) -> Result<(), String> {
    match receipt.outcome() {
        AttemptOutcome::Quarantined { reason: got, .. } if *got == reason => {}
        other => return Err(format!("outcome {other:?}")),
    }
    if receipt.followups_allowed() || receipt.outcome().score().is_some() {
        return Err("quarantine kept a follow-up or a score".into());
    }
    Ok(())
}

pub(super) fn stopped(io: &harness::Handles) -> bool {
    runner_commands(io).iter().any(|command| {
        matches!(
            command,
            TartCommand::Stop { .. } | TartCommand::Delete { .. }
        )
    })
}

fn success_trace_keeps_pass_and_rejects_later_events() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Final);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Final)?;
    let receipt = attempt.receipt().cloned().ok_or("missing receipt")?;
    if receipt.outcome() != &AttemptOutcome::Scored(TaskScore::Pass) {
        return Err(format!("{:?}", receipt.outcome()));
    }
    if !receipt.followups_allowed()
        || !receipt.cleanup_confirmed()
        || receipt.recovery_required()
        || receipt.disposition() != CleanupDisposition::Allocated
        || attempt.client_starts() != 1
        || attempt.phase() != Phase::Final
    {
        return Err(format!("receipt {receipt:?} phase {:?}", attempt.phase()));
    }
    let commands = runner_commands(&io);
    let clone = id_clone(CLONE);
    let flags = harness::flags();
    let expected = [
        TartCommand::List,
        TartCommand::List,
        TartCommand::Clone {
            golden: gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
            clone: clone.clone(),
        },
        TartCommand::List,
        TartCommand::Status {
            clone: clone.clone(),
        },
        TartCommand::Start {
            clone: clone.clone(),
            softnet: flags,
        },
        TartCommand::List,
        TartCommand::Status {
            clone: clone.clone(),
        },
        TartCommand::Stop {
            clone: clone.clone(),
        },
        TartCommand::Status {
            clone: clone.clone(),
        },
        TartCommand::Delete {
            clone: clone.clone(),
        },
        TartCommand::List,
    ];
    if commands != expected {
        return Err(format!("runner {commands:?}"));
    }
    assert_ids(&commands, &[CLONE])?;
    let mut guests = vec![GuestCommand::Inventory];
    guests.extend(Probe::ALL.into_iter().map(GuestCommand::Probe));
    guests.extend([
        GuestCommand::Bootstrap,
        GuestCommand::Login,
        GuestCommand::Inventory,
        GuestCommand::Run,
        GuestCommand::Revoke,
    ]);
    if guest_commands(&io) != guests {
        return Err(format!("guest {:?}", guest_commands(&io)));
    }
    let row = io
        .ledger
        .borrow()
        .query(&clone, query(&io.clock))
        .map_err(|err| format!("{err:?}"))?;
    if !row.is_sealed() || row.facts() != [LedgerFact::CloneIssued, LedgerFact::StartIssued] {
        return Err(format!("row {row:?}"));
    }
    let runner_len = commands.len();
    let guest_len = guests.len();
    let run = gcu_eval::disposable::tart::RunDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::RunDeadline::CAP,
    )
    .unwrap();
    if !matches!(attempt.run(run), Err(StepError::Terminal))
        || !matches!(attempt.cancel(query(&io.clock)), Err(StepError::Terminal))
        || !matches!(
            attempt.seal(SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap()),
            Err(StepError::Terminal)
        )
        || !matches!(
            attempt.destroy(&cleanup_deadlines(&io.clock)),
            Err(StepError::Terminal)
        )
    {
        return Err("terminal call was accepted".into());
    }
    if runner_commands(&io).len() != runner_len || guest_commands(&io).len() != guest_len {
        return Err("terminal call issued io".into());
    }
    Ok(())
}

fn unexpected_start_from_idle_does_not_clone() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    let mut attempt = world.attempt();
    let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
    if !matches!(
        attempt.start(&harness::flags(), deadline),
        Err(StepError::Unexpected)
    ) {
        return Err("start was accepted".into());
    }
    if attempt.phase() != Phase::Destroying || !runner_commands(&io).is_empty() {
        return Err(format!("phase {:?}", attempt.phase()));
    }
    Ok(())
}

fn second_run_stays_not_run_after_seal() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    let deadline = gcu_eval::disposable::tart::RunDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::RunDeadline::CAP,
    )
    .unwrap();
    if attempt.run(deadline).is_ok() || run_count(&io) != 1 {
        return Err(format!("runs {}", run_count(&io)));
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::Unexpected, true, true)
}

fn second_revoke_does_not_send_again() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    push_guest(&io, ok_body("ok"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    let deadline = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke(deadline)
        .map_err(|err| format!("revoke: {err:?}"))?;
    let again = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
    attempt
        .revoke(again)
        .map_err(|err| format!("second: {err:?}"))?;
    let revokes = guest_commands(&io)
        .iter()
        .filter(|command| matches!(command, GuestCommand::Revoke))
        .count();
    if revokes != 1 {
        return Err(format!("revokes {revokes}"));
    }
    Ok(())
}

fn failed_probe_is_not_run_without_a_client() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Booting);
    push_guest(
        &io,
        ok_body("guest v1\nboot=boot1\nuid=uid1\ndesktop=desk1\nconfig=cfg1\nend\n"),
    );
    push_guest(&io, ok_body("failed"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Booting)?;
    let deadline = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
    if !matches!(attempt.isolate(deadline), Err(StepError::Breach)) {
        return Err("probe failure was not a breach".into());
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::EnvironmentBreach, false, true)?;
    if run_count(&io) != 0 {
        return Err("client started".into());
    }
    Ok(())
}

fn unknown_probes_block_ready_without_a_client() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Booting);
    push_isolate(&io, "unknown");
    push_guest(&io, ok_body(&format!("ack v1 run={RUN} generation=1")));
    push_guest(&io, ok_body("login v1 ready"));
    push_guest(
        &io,
        ok_body("guest v1\nboot=boot1\nuid=uid1\ndesktop=desk1\nconfig=cfg1\nend\n"),
    );
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Booting)?;
    let deadline = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
    attempt
        .isolate(deadline)
        .map_err(|err| format!("isolate: {err:?}"))?;
    let stage = gcu_eval::disposable::tart::StageDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::StageDeadline::CAP,
    )
    .unwrap();
    attempt
        .bootstrap(stage)
        .map_err(|err| format!("bootstrap: {err:?}"))?;
    let login = gcu_eval::disposable::tart::LoginDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::LoginDeadline::CAP,
    )
    .unwrap();
    attempt
        .login(login)
        .map_err(|err| format!("login: {err:?}"))?;
    if !matches!(
        attempt.ready(query(&io.clock)),
        Err(StepError::MalformedBody)
    ) {
        return Err("unknown probes became ready".into());
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::MissingProof, false, true)?;
    Ok(())
}

fn foreign_ack_is_missing_proof() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Isolated);
    push_guest(&io, ok_body("ack v1 run=other generation=1"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Isolated)?;
    let deadline = gcu_eval::disposable::tart::StageDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::StageDeadline::CAP,
    )
    .unwrap();
    if attempt.bootstrap(deadline).is_ok() {
        return Err("foreign ack accepted".into());
    }
    expect_not_run(
        &finish(&mut attempt, &io, true)?,
        NotRunReason::MissingProof,
        false,
        true,
    )
}

fn missing_login_is_missing_proof() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Bootstrapped);
    push_guest(&io, ok_body("login v1 missing"));
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Bootstrapped)?;
    let deadline = gcu_eval::disposable::tart::LoginDeadline::after(
        io.clock.now(),
        gcu_eval::disposable::tart::LoginDeadline::CAP,
    )
    .unwrap();
    if attempt.login(deadline).is_ok() {
        return Err("missing login accepted".into());
    }
    expect_not_run(
        &finish(&mut attempt, &io, true)?,
        NotRunReason::MissingProof,
        false,
        true,
    )
}

fn boot_change_is_not_adopted() -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::CredentialReady);
    push_guest(
        &io,
        ok_body("guest v1\nboot=boot2\nuid=uid1\ndesktop=desk1\nconfig=cfg1\nend\n"),
    );
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::CredentialReady)?;
    if !matches!(attempt.ready(query(&io.clock)), Err(StepError::Breach)) {
        return Err("boot change was adopted".into());
    }
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, NotRunReason::BootChanged, false, true)?;
    if start_ids(&io) != [CLONE] || run_count(&io) != 0 {
        return Err("restarted or ran".into());
    }
    Ok(())
}

fn loss_after_run(
    note: impl FnOnce(&mut harness::Machine<'_>, &harness::Handles) -> Result<(), String>,
    reason: NotRunReason,
) -> Result<(), String> {
    let mut world = harness::World::new(PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, Phase::Running);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, Phase::Running)?;
    note(&mut attempt, &io)?;
    let receipt = finish(&mut attempt, &io, true)?;
    expect_not_run(&receipt, reason, true, true)?;
    if start_ids(&io) != [CLONE] {
        return Err(format!("starts {:?}", start_ids(&io)));
    }
    Ok(())
}

fn provider_crash_stays_not_run_after_cleanup() -> Result<(), String> {
    loss_after_run(
        |attempt, io| match attempt.note_provider_crash(query(&io.clock)) {
            Err(StepError::Lost(NotRunReason::ProviderCrash)) => Ok(()),
            other => Err(format!("{other:?}")),
        },
        NotRunReason::ProviderCrash,
    )
}

fn guest_crash_does_not_restart() -> Result<(), String> {
    loss_after_run(
        |attempt, io| match attempt.note_runtime_loss(RuntimeLoss::GuestCrash, query(&io.clock)) {
            Err(StepError::Lost(NotRunReason::GuestCrash)) => Ok(()),
            other => Err(format!("{other:?}")),
        },
        NotRunReason::GuestCrash,
    )
}

fn tart_exit_does_not_restart() -> Result<(), String> {
    loss_after_run(
        |attempt, io| match attempt.note_runtime_loss(RuntimeLoss::TartExited, query(&io.clock)) {
            Err(StepError::Lost(NotRunReason::TartExited)) => Ok(()),
            other => Err(format!("{other:?}")),
        },
        NotRunReason::TartExited,
    )
}

fn softnet_exit_does_not_restart() -> Result<(), String> {
    loss_after_run(
        |attempt, io| match attempt.note_runtime_loss(RuntimeLoss::SoftnetExited, query(&io.clock))
        {
            Err(StepError::Lost(NotRunReason::SoftnetExited)) => Ok(()),
            other => Err(format!("{other:?}")),
        },
        NotRunReason::SoftnetExited,
    )
}
