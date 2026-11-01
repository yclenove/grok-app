//! Every forward edge and terminal, with one fault mode each.
//!
//! Cleanup after the fault deletes only the reserved clone id. A new attempt
//! uses a new id.

use gcu_eval::disposable::tart::{
    AttemptDeadline, AttemptOutcome, AttemptReceipt, CloneDeadline, GroupDeadline, HostClock,
    LedgerFault, LoginDeadline, NotRunReason, Phase, RevokeDeadline, RunDeadline, ScriptedReply,
    SealDeadline, StageDeadline, StartDeadline, StepError,
};

use super::harness::{
    self, assert_ids, cleanup_deadlines, drive_to, fail_body, finish, push_runner, query,
    reserve_named, run_count, runner_commands, scripts_to_reach, start_ids, CLONE, CLONE_2, RUN_2,
};

#[derive(Clone, Copy, Debug)]
enum Step {
    Reserve,
    Clone,
    Start,
    Isolate,
    Bootstrap,
    Login,
    Ready,
    Run,
    Revoke,
    Destroy,
    Seal,
}

#[derive(Clone, Copy, Debug)]
enum Inject {
    Failed,
    TimedOut,
    Malformed,
    Overlong,
    Lost,
    Cancel,
}

const STEPS: [Step; 11] = [
    Step::Reserve,
    Step::Clone,
    Step::Start,
    Step::Isolate,
    Step::Bootstrap,
    Step::Login,
    Step::Ready,
    Step::Run,
    Step::Revoke,
    Step::Destroy,
    Step::Seal,
];

const INJECTS: [Inject; 6] = [
    Inject::Failed,
    Inject::TimedOut,
    Inject::Malformed,
    Inject::Overlong,
    Inject::Lost,
    Inject::Cancel,
];

#[allow(
    clippy::type_complexity,
    reason = "one table of named checks; a type alias would not make the pointer simpler"
)]
pub(super) const TESTS: &[(&str, fn() -> Result<(), String>)] = &[(
    "every_edge_fault_cleans_only_the_owned_clone",
    every_edge_fault_cleans_only_the_owned_clone,
)];

fn previous(step: Step) -> Phase {
    match step {
        Step::Reserve => Phase::Idle,
        Step::Clone => Phase::Reserved,
        Step::Start => Phase::Cloned,
        Step::Isolate => Phase::Booting,
        Step::Bootstrap => Phase::Isolated,
        Step::Login => Phase::Bootstrapped,
        Step::Ready => Phase::CredentialReady,
        Step::Run => Phase::Ready,
        Step::Revoke => Phase::Running,
        Step::Destroy => Phase::Revoking,
        Step::Seal => Phase::Destroying,
    }
}

fn ledger_fault(inject: Inject) -> LedgerFault {
    match inject {
        Inject::Failed => LedgerFault::Failed,
        Inject::TimedOut => LedgerFault::TimedOut,
        Inject::Malformed => LedgerFault::Malformed,
        Inject::Overlong => LedgerFault::Overlong,
        Inject::Lost => LedgerFault::ReplyLost,
        Inject::Cancel => LedgerFault::Failed,
    }
}

fn fault_reply(inject: Inject) -> ScriptedReply {
    match inject {
        Inject::Failed => fail_body("no"),
        Inject::TimedOut => ScriptedReply::timed_out(),
        Inject::Malformed => ScriptedReply::malformed(b"???".to_vec()).unwrap(),
        Inject::Overlong => ScriptedReply::overlong(),
        Inject::Lost | Inject::Cancel => ok_unused(),
    }
}

fn ok_unused() -> ScriptedReply {
    harness::ok_body("ok")
}

fn client_expected(step: Step, inject: Inject) -> bool {
    match step {
        Step::Run => !matches!(inject, Inject::Cancel),
        Step::Revoke | Step::Destroy | Step::Seal => true,
        _ => false,
    }
}

fn quarantined(step: Step, inject: Inject) -> bool {
    match (step, inject) {
        (Step::Revoke, Inject::Cancel) => false,
        (Step::Revoke | Step::Destroy, _) => true,
        (Step::Seal, Inject::Cancel) => false,
        (Step::Seal, _) => true,
        _ => false,
    }
}

fn reason_for(step: Step, inject: Inject) -> NotRunReason {
    if quarantined(step, inject) {
        return if matches!(step, Step::Seal) {
            NotRunReason::ReceiptFailed
        } else {
            NotRunReason::CleanupFailed
        };
    }
    match inject {
        Inject::Failed => NotRunReason::CommandFailed,
        Inject::TimedOut => NotRunReason::Timeout,
        Inject::Malformed => NotRunReason::Malformed,
        Inject::Overlong => NotRunReason::Overlong,
        Inject::Lost => NotRunReason::ReplyLost,
        Inject::Cancel => NotRunReason::Cancelled,
    }
}

fn vm_present(step: Step, inject: Inject) -> bool {
    match step {
        Step::Reserve => false,
        Step::Clone => !matches!(inject, Inject::Cancel),
        _ => true,
    }
}

fn every_edge_fault_cleans_only_the_owned_clone() -> Result<(), String> {
    let mut errors = Vec::new();
    for step in STEPS {
        for inject in INJECTS {
            if let Err(err) = one(step, inject) {
                errors.push(format!("{step:?}/{inject:?}: {err}"));
                if errors.len() == 8 {
                    break;
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn one(step: Step, inject: Inject) -> Result<(), String> {
    let mut world = harness::World::new(gcu_eval::disposable::tart::PhaseBudgets::adr());
    let io = world.open();
    scripts_to_reach(&io, previous(step));
    queue_fault(&io, step, inject);
    let mut attempt = world.attempt();
    drive_to(&mut attempt, &io, previous(step))?;
    let receipt = apply(&mut attempt, &io, step, inject)?;
    let reason = reason_for(step, inject);
    let quarantine = quarantined(step, inject);
    match receipt.outcome() {
        AttemptOutcome::Quarantined {
            reason: got,
            client_started,
        } if quarantine => {
            if *got != reason || *client_started != client_expected(step, inject) {
                return Err(format!("quarantine {got:?} client {client_started}"));
            }
        }
        AttemptOutcome::NotRun {
            reason: got,
            client_started,
        } if !quarantine => {
            if *got != reason || *client_started != client_expected(step, inject) {
                return Err(format!("not_run {got:?} client {client_started}"));
            }
        }
        other => return Err(format!("outcome {other:?}")),
    }
    if receipt.outcome().score().is_some() {
        return Err("fault kept a score".into());
    }
    if receipt.followups_allowed() == quarantine {
        return Err(format!("followups {}", receipt.followups_allowed()));
    }
    if run_count(&io) > 1 {
        return Err("duplicate client".into());
    }
    assert_ids(&runner_commands(&io), &[CLONE])?;
    let follow = receipt.followups_allowed();
    drop(attempt);
    follow_up(&mut world, &io, follow)
}

fn queue_fault(io: &harness::Handles, step: Step, inject: Inject) {
    if matches!(step, Step::Seal) || matches!(inject, Inject::Cancel) {
        return;
    }
    if matches!(inject, Inject::Lost) {
        // A lost ledger reply is a fault. A lost Tart or guest reply is an empty script.
        if matches!(step, Step::Reserve) {
            io.ledger.borrow_mut().push_fault(LedgerFault::ReplyLost);
        }
        return;
    }
    if matches!(step, Step::Reserve) {
        io.ledger.borrow_mut().push_fault(ledger_fault(inject));
        return;
    }
    let reply = fault_reply(inject);
    match step {
        Step::Clone | Step::Start | Step::Destroy => push_runner(io, reply),
        Step::Isolate | Step::Bootstrap | Step::Login | Step::Ready | Step::Run | Step::Revoke => {
            harness::push_guest(io, reply);
        }
        Step::Reserve | Step::Seal => {}
    }
}

fn apply(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
    step: Step,
    inject: Inject,
) -> Result<AttemptReceipt, String> {
    if matches!(inject, Inject::Cancel) && !matches!(step, Step::Destroy | Step::Seal) {
        match attempt.cancel(query(&io.clock)) {
            Err(StepError::Lost(NotRunReason::Cancelled)) => {}
            other => return Err(format!("cancel: {other:?}")),
        }
        return if matches!(step, Step::Reserve) {
            just_seal(attempt, io)
        } else {
            finish(attempt, io, vm_present(step, inject))
        };
    }
    match step {
        Step::Reserve => {
            let err = reserve_named(
                attempt,
                &io.clock,
                harness::RUN,
                CLONE,
                AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap(),
                false,
                true,
                harness::manifest(),
            );
            if err.is_ok() {
                return Err("reserve fault succeeded".into());
            }
            just_seal(attempt, io)
        }
        Step::Clone => {
            let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
            expect_err(attempt.clone_vm(
                gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
                deadline,
            ))?;
            finish(attempt, io, vm_present(step, inject))
        }
        Step::Start => {
            let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
            expect_err(attempt.start(&harness::flags(), deadline))?;
            finish(attempt, io, true)
        }
        Step::Isolate => {
            let deadline = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
            expect_err(attempt.isolate(deadline))?;
            finish(attempt, io, true)
        }
        Step::Bootstrap => {
            let deadline = StageDeadline::after(io.clock.now(), StageDeadline::CAP).unwrap();
            expect_err(attempt.bootstrap(deadline))?;
            finish(attempt, io, true)
        }
        Step::Login => {
            let deadline = LoginDeadline::after(io.clock.now(), LoginDeadline::CAP).unwrap();
            expect_err(attempt.login(deadline))?;
            finish(attempt, io, true)
        }
        Step::Ready => {
            expect_err(attempt.ready(query(&io.clock)))?;
            finish(attempt, io, true)
        }
        Step::Run => {
            let deadline = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
            expect_err(attempt.run(deadline))?;
            finish(attempt, io, true)
        }
        Step::Revoke => {
            let deadline = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
            expect_err(attempt.revoke(deadline))?;
            finish(attempt, io, true)
        }
        Step::Destroy => {
            if matches!(inject, Inject::Cancel) {
                let deadlines = cleanup_deadlines(&io.clock);
                harness::advance(&io.clock, gcu_eval::disposable::tart::QueryDeadline::CAP)?;
                expect_err(attempt.destroy(&deadlines))?;
            } else {
                expect_err(attempt.destroy(&cleanup_deadlines(&io.clock)))?;
            }
            just_seal(attempt, io)
        }
        Step::Seal => seal_fault(attempt, io, inject),
    }
}

fn expect_err(result: Result<(), StepError>) -> Result<(), String> {
    match result {
        Err(_) => Ok(()),
        Ok(()) => Err("fault step succeeded".into()),
    }
}

fn just_seal(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
) -> Result<AttemptReceipt, String> {
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    attempt
        .seal(deadline)
        .map_err(|err| format!("seal: {err:?}"))
}

fn seal_fault(
    attempt: &mut harness::Machine<'_>,
    io: &harness::Handles,
    inject: Inject,
) -> Result<AttemptReceipt, String> {
    if matches!(inject, Inject::Cancel) {
        match attempt.cancel(query(&io.clock)) {
            Err(StepError::Lost(NotRunReason::Cancelled)) => {}
            other => return Err(format!("seal cancel: {other:?}")),
        }
        return just_seal(attempt, io);
    }
    io.ledger.borrow_mut().push_fault(ledger_fault(inject));
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    if attempt.seal(deadline).is_ok() {
        return Err("seal fault succeeded".into());
    }
    attempt
        .receipt()
        .cloned()
        .ok_or_else(|| "seal fault stored no receipt".into())
}

fn follow_up(
    world: &mut harness::World,
    io: &harness::Handles,
    follow: bool,
) -> Result<(), String> {
    let before = runner_commands(io);
    let old_starts = start_ids(io);
    let mut attempt = world.attempt();
    let absolute = AttemptDeadline::after(io.clock.now(), AttemptDeadline::CAP).unwrap();
    if follow {
        push_runner(io, harness::ok_body("ok"));
        push_runner(io, harness::ok_body("ok"));
        reserve_named(
            &mut attempt,
            &io.clock,
            RUN_2,
            CLONE_2,
            absolute,
            false,
            true,
            harness::manifest(),
        )
        .map_err(|err| format!("new reserve: {err:?}"))?;
        let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
        attempt
            .clone_vm(
                gcu_eval::disposable::tart::GoldenImage::MacosS2Eval,
                deadline,
            )
            .map_err(|err| format!("new clone: {err:?}"))?;
        let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
        attempt
            .start(&harness::flags(), deadline)
            .map_err(|err| format!("new start: {err:?}"))?;
        let starts = start_ids(io);
        if starts.iter().filter(|id| id.as_str() == CLONE).count() != old_starts.len() {
            return Err(format!("old clone restarted: {starts:?}"));
        }
        if starts.last().map(String::as_str) != Some(CLONE_2) {
            return Err(format!("new start was {starts:?}"));
        }
        assert_ids(&runner_commands(io), &[CLONE, CLONE_2])?;
        Ok(())
    } else {
        let err = reserve_named(
            &mut attempt,
            &io.clock,
            RUN_2,
            CLONE_2,
            absolute,
            false,
            true,
            harness::manifest(),
        );
        if !matches!(
            err,
            Err(StepError::Halted | StepError::MustRecover | StepError::LockBusy)
        ) {
            return Err(format!("follow-up was not stopped: {err:?}"));
        }
        if runner_commands(io).len() != before.len() {
            return Err("stopped follow-up issued tart".into());
        }
        Ok(())
    }
}
