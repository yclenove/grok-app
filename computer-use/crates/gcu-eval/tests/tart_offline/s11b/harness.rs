//! Fake world for one attempt. Scripts can be pushed between steps.
//!
//! The machine borrows the wrappers. The wrappers share `RefCell`s, so a test
//! can enqueue the next reply without a worker thread. Nothing here sleeps.

use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::rc::Rc;
use std::time::Duration;

use gcu_eval::disposable::tart::{
    Attempt, AttemptDeadline, AttemptLedger, AttemptReceipt, BoundedOutput, CallError,
    CleanupDeadlines, CleanupLock, CloneDeadline, CloneId, DeleteDeadline, Digest, FakeCleanupLock,
    FakeClock, FakeGuestChannel, FakeLedger, FakeTartRunner, GoldenImage, GroupDeadline,
    GuestChannel, GuestCommand, HostClock, InstallManifest, LeaseDeadline, LoginDeadline, Phase,
    PhaseBudgets, QueryDeadline, ReserveInput, RevokeDeadline, RunDeadline, RunId, ScriptedReply,
    SealDeadline, SoftnetFlags, StageDeadline, StartDeadline, StepError, StopDeadline, TartCommand,
    TartRunner, VmIdentity,
};
use gcu_eval::disposable::Probe;

pub(super) const RUN: &str = "run-1";
pub(super) const CLONE: &str = "clone-1";
pub(super) const RUN_2: &str = "run-2";
pub(super) const CLONE_2: &str = "clone-2";
pub(super) const INC: &str = "inc1";
pub(super) const CFG: &str = "cfg1";
const GOLDEN: &str = "macos-s2-eval";
const EXTERNAL: &str = "external-vm";

pub(super) struct Share<T> {
    inner: Rc<RefCell<T>>,
}

impl<T> Share<T> {
    fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(value)),
        }
    }

    pub(super) fn handle(&self) -> Rc<RefCell<T>> {
        Rc::clone(&self.inner)
    }
}

impl TartRunner for Share<FakeTartRunner> {
    fn clone_vm(
        &mut self,
        permit: gcu_eval::disposable::tart::ClonePermit,
        golden: GoldenImage,
        deadline: CloneDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().clone_vm(permit, golden, deadline)
    }

    fn start(
        &mut self,
        clone: &CloneId,
        softnet: &SoftnetFlags,
        deadline: StartDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().start(clone, softnet, deadline)
    }

    fn stop(
        &mut self,
        clone: &CloneId,
        deadline: StopDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().stop(clone, deadline)
    }

    fn delete(
        &mut self,
        clone: &CloneId,
        deadline: DeleteDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().delete(clone, deadline)
    }

    fn list(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().list(deadline)
    }

    fn status(
        &mut self,
        clone: &CloneId,
        deadline: QueryDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().status(clone, deadline)
    }
}

impl GuestChannel for Share<FakeGuestChannel> {
    fn protocol_version(&self) -> u16 {
        self.inner.borrow().protocol_version()
    }

    fn inventory(&mut self, deadline: QueryDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().inventory(deadline)
    }

    fn probe(
        &mut self,
        probe: Probe,
        deadline: gcu_eval::disposable::tart::ProbeDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().probe(probe, deadline)
    }

    fn bootstrap(&mut self, deadline: StageDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().bootstrap(deadline)
    }

    fn login(&mut self, deadline: LoginDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().login(deadline)
    }

    fn run(&mut self, deadline: RunDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().run(deadline)
    }

    fn lease(
        &mut self,
        seq: std::num::NonZeroU64,
        deadline: LeaseDeadline,
    ) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().lease(seq, deadline)
    }

    fn revoke(&mut self, deadline: RevokeDeadline) -> Result<BoundedOutput, CallError> {
        self.inner.borrow_mut().revoke(deadline)
    }
}

impl AttemptLedger for Share<FakeLedger> {
    fn reserve(
        &mut self,
        run: RunId,
        clone: CloneId,
        absolute: AttemptDeadline,
        call: QueryDeadline,
    ) -> Result<gcu_eval::disposable::tart::ClonePermit, gcu_eval::disposable::tart::LedgerError>
    {
        self.inner.borrow_mut().reserve(run, clone, absolute, call)
    }

    fn query(
        &self,
        clone: &CloneId,
        call: QueryDeadline,
    ) -> Result<gcu_eval::disposable::tart::Reservation, gcu_eval::disposable::tart::LedgerError>
    {
        self.inner.borrow().query(clone, call)
    }

    fn append(
        &mut self,
        clone: &CloneId,
        fact: gcu_eval::disposable::tart::LedgerFact,
        call: QueryDeadline,
    ) -> Result<(), gcu_eval::disposable::tart::LedgerError> {
        self.inner.borrow_mut().append(clone, fact, call)
    }

    fn note_identity(
        &mut self,
        clone: &CloneId,
        identity: &VmIdentity,
        call: QueryDeadline,
    ) -> Result<(), gcu_eval::disposable::tart::LedgerError> {
        self.inner.borrow_mut().note_identity(clone, identity, call)
    }

    fn seal(
        &mut self,
        clone: &CloneId,
        call: SealDeadline,
    ) -> Result<(), gcu_eval::disposable::tart::LedgerError> {
        self.inner.borrow_mut().seal(clone, call)
    }

    fn rows(
        &self,
        call: QueryDeadline,
    ) -> Result<Vec<gcu_eval::disposable::tart::Reservation>, gcu_eval::disposable::tart::LedgerError>
    {
        self.inner.borrow().rows(call)
    }
}

impl CleanupLock for Share<FakeCleanupLock> {
    fn try_acquire(
        &mut self,
        deadline: gcu_eval::disposable::tart::MonoInstant,
    ) -> Result<(), gcu_eval::disposable::tart::LockError> {
        self.inner.borrow_mut().try_acquire(deadline)
    }
}

pub(super) type Machine<'a> = Attempt<
    'a,
    Share<FakeTartRunner>,
    Share<FakeGuestChannel>,
    Share<FakeLedger>,
    Share<FakeCleanupLock>,
    FakeClock,
>;

pub(super) struct World {
    clock: FakeClock,
    runner: Share<FakeTartRunner>,
    guest: Share<FakeGuestChannel>,
    ledger: Share<FakeLedger>,
    lock: Share<FakeCleanupLock>,
    budgets: PhaseBudgets,
}

pub(super) struct Handles {
    pub runner: Rc<RefCell<FakeTartRunner>>,
    pub guest: Rc<RefCell<FakeGuestChannel>>,
    pub ledger: Rc<RefCell<FakeLedger>>,
    pub lock: Rc<RefCell<FakeCleanupLock>>,
    pub clock: FakeClock,
}

impl World {
    pub(super) fn new(budgets: PhaseBudgets) -> Self {
        let clock = FakeClock::new();
        Self {
            clock: clock.clone(),
            runner: Share::new(FakeTartRunner::new(clock.clone())),
            guest: Share::new(FakeGuestChannel::new(clock.clone())),
            ledger: Share::new(FakeLedger::new(clock.clone())),
            lock: Share::new(FakeCleanupLock::new(clock)),
            budgets,
        }
    }

    pub(super) fn open(&self) -> Handles {
        Handles {
            runner: self.runner.handle(),
            guest: self.guest.handle(),
            ledger: self.ledger.handle(),
            lock: self.lock.handle(),
            clock: self.clock.clone(),
        }
    }

    pub(super) fn attempt(&mut self) -> Machine<'_> {
        Attempt::new(
            &mut self.runner,
            &mut self.guest,
            &mut self.ledger,
            &mut self.lock,
            &self.clock,
            self.budgets,
        )
    }
}

pub(super) fn ok_body(body: &str) -> ScriptedReply {
    ScriptedReply::success(body.as_bytes().to_vec()).unwrap()
}

pub(super) fn fail_body(body: &str) -> ScriptedReply {
    ScriptedReply::failed(body.as_bytes().to_vec()).unwrap()
}

pub(super) fn manifest() -> InstallManifest {
    let digest = |byte: u8| Digest::try_from_bytes([byte; 32]).unwrap();
    InstallManifest::new(digest(1), digest(2), digest(3))
}

pub(super) fn other_manifest() -> InstallManifest {
    let digest = |byte: u8| Digest::try_from_bytes([byte; 32]).unwrap();
    InstallManifest::new(digest(4), digest(5), digest(6))
}

pub(super) fn flags() -> SoftnetFlags {
    SoftnetFlags::try_new(&[Ipv4Addr::new(1, 1, 1, 1)]).unwrap()
}

pub(super) fn id_run(raw: &str) -> RunId {
    RunId::new(raw).unwrap()
}

pub(super) fn id_clone(raw: &str) -> CloneId {
    CloneId::new(raw).unwrap()
}

fn guest_view(boot: &str) -> String {
    format!("guest v1\nboot={boot}\nuid=uid1\ndesktop=desk1\nconfig=cfg1\nend\n")
}

fn ack(run: &str) -> String {
    format!("ack v1 run={run} generation=1")
}

fn list(names: &[&str]) -> String {
    let mut text = format!("list v1\ncount {}\n", names.len());
    for name in names {
        text.push_str(name);
        text.push('\n');
    }
    text.push_str("end\n");
    text
}

fn status_line(state: &str, run: &str, tart: &str, softnet: &str) -> String {
    format!(
        "status v1 state={state} run={run} boot=boot1 inc={INC} cfg={CFG} tart={tart} softnet={softnet}"
    )
}

pub(super) fn clone_body() -> String {
    format!("clone v1 inc={INC} cfg={CFG}")
}

pub(super) fn push_runner(io: &Handles, reply: ScriptedReply) {
    io.runner.borrow_mut().push(reply);
}

pub(super) fn push_guest(io: &Handles, reply: ScriptedReply) {
    io.guest.borrow_mut().push(reply);
}

pub(super) fn push_isolate(io: &Handles, probe: &str) {
    push_guest(io, ok_body(&guest_view("boot1")));
    for _ in 0..Probe::ALL.len() {
        push_guest(io, ok_body(probe));
    }
}

pub(super) fn push_clear(io: &Handles) {
    push_runner(io, ok_body(&list(&[GOLDEN, EXTERNAL])));
}

pub(super) fn push_named(io: &Handles, clone: &str) {
    push_runner(io, ok_body(&list(&[GOLDEN, EXTERNAL, clone])));
}

pub(super) fn push_status(io: &Handles, state: &str, run: &str, tart: &str, softnet: &str) {
    push_runner(io, ok_body(&status_line(state, run, tart, softnet)));
}

pub(super) fn push_clone_ok(io: &Handles) {
    push_runner(io, ok_body(&clone_body()));
}

pub(super) fn push_present(io: &Handles) {
    push_named(io, CLONE);
    push_status(io, "running", RUN, "up", "up");
    push_runner(io, ok_body("ok"));
    push_status(io, "stopped", RUN, "exited", "exited");
    push_runner(io, ok_body("ok"));
    push_clear(io);
}

/// One complete list that omits the clone. Absence is this list, not a status.
pub(super) fn push_absent(io: &Handles) {
    push_clear(io);
}

fn rank(phase: Phase) -> u8 {
    match phase {
        Phase::Idle => 0,
        Phase::Reserved => 1,
        Phase::Cloned => 2,
        Phase::Booting => 3,
        Phase::Isolated => 4,
        Phase::Bootstrapped => 5,
        Phase::CredentialReady => 6,
        Phase::Ready => 7,
        Phase::Running => 8,
        Phase::Revoking => 9,
        Phase::Destroying => 10,
        Phase::Final => 11,
        Phase::Quarantined | Phase::Suspended => 12,
    }
}

/// Queue the successes required to reach `target`, not the step after it.
pub(super) fn scripts_to_reach(io: &Handles, target: Phase) {
    let target = rank(target);
    if target >= rank(Phase::Reserved) {
        push_clear(io);
    }
    if target >= rank(Phase::Cloned) {
        push_clear(io);
        push_clone_ok(io);
    }
    if target >= rank(Phase::Booting) {
        push_named(io, CLONE);
        push_status(io, "running", RUN, "up", "up");
        push_runner(io, ok_body("ok"));
    }
    if target >= rank(Phase::Isolated) {
        push_isolate(io, "passed");
    }
    if target >= rank(Phase::Bootstrapped) {
        push_guest(io, ok_body(&ack(RUN)));
    }
    if target >= rank(Phase::CredentialReady) {
        push_guest(io, ok_body("login v1 ready"));
    }
    if target >= rank(Phase::Ready) {
        push_guest(io, ok_body(&guest_view("boot1")));
    }
    if target >= rank(Phase::Running) {
        push_guest(io, ok_body("score pass"));
    }
    if target >= rank(Phase::Revoking) {
        push_guest(io, ok_body("ok"));
    }
    if target >= rank(Phase::Destroying) {
        push_present(io);
    }
}

pub(super) fn query(clock: &FakeClock) -> QueryDeadline {
    QueryDeadline::after(clock.now(), QueryDeadline::CAP).unwrap()
}

pub(super) fn cleanup_deadlines(clock: &FakeClock) -> CleanupDeadlines {
    CleanupDeadlines {
        query: query(clock),
        stop: StopDeadline::after(clock.now(), StopDeadline::CAP).unwrap(),
        delete: DeleteDeadline::after(clock.now(), DeleteDeadline::CAP).unwrap(),
        seal: SealDeadline::after(clock.now(), SealDeadline::CAP).unwrap(),
        revoke: RevokeDeadline::after(clock.now(), RevokeDeadline::CAP).unwrap(),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the extra flags are the reserve inputs a caller would pass one by one"
)]
pub(super) fn reserve_named(
    attempt: &mut Machine<'_>,
    clock: &FakeClock,
    run: &str,
    clone: &str,
    absolute: AttemptDeadline,
    stop_latched: bool,
    owner_present: bool,
    approved: InstallManifest,
) -> Result<(), StepError> {
    attempt.reserve(
        ReserveInput {
            run: id_run(run),
            clone_id: id_clone(clone),
            absolute,
            manifest: manifest(),
            approved,
            stop_latched,
            owner_present,
        },
        query(clock),
    )
}

pub(super) fn reserve_default(attempt: &mut Machine<'_>, clock: &FakeClock) -> Result<(), String> {
    let absolute = AttemptDeadline::after(clock.now(), AttemptDeadline::CAP).unwrap();
    reserve_named(
        attempt,
        clock,
        RUN,
        CLONE,
        absolute,
        false,
        true,
        manifest(),
    )
    .map_err(|err| format!("reserve: {err:?}"))
}

fn call(name: &str, result: Result<(), StepError>) -> Result<(), String> {
    result.map_err(|err| format!("{name}: {err:?}"))
}

pub(super) fn drive_to(
    attempt: &mut Machine<'_>,
    io: &Handles,
    target: Phase,
) -> Result<(), String> {
    for _ in 0..12 {
        if attempt.phase() == target {
            return Ok(());
        }
        match attempt.phase() {
            Phase::Idle => reserve_default(attempt, &io.clock)?,
            Phase::Reserved => {
                let deadline = CloneDeadline::after(io.clock.now(), CloneDeadline::CAP).unwrap();
                call(
                    "clone",
                    attempt.clone_vm(GoldenImage::MacosS2Eval, deadline),
                )?;
            }
            Phase::Cloned => {
                let deadline = StartDeadline::after(io.clock.now(), StartDeadline::CAP).unwrap();
                call("start", attempt.start(&flags(), deadline))?;
            }
            Phase::Booting => {
                let deadline = GroupDeadline::after(io.clock.now(), GroupDeadline::CAP).unwrap();
                call("isolate", attempt.isolate(deadline))?;
            }
            Phase::Isolated => {
                let deadline = StageDeadline::after(io.clock.now(), StageDeadline::CAP).unwrap();
                call("bootstrap", attempt.bootstrap(deadline))?;
            }
            Phase::Bootstrapped => {
                let deadline = LoginDeadline::after(io.clock.now(), LoginDeadline::CAP).unwrap();
                call("login", attempt.login(deadline))?;
            }
            Phase::CredentialReady => call("ready", attempt.ready(query(&io.clock)))?,
            Phase::Ready => {
                let deadline = RunDeadline::after(io.clock.now(), RunDeadline::CAP).unwrap();
                call("run", attempt.run(deadline))?;
            }
            Phase::Running => {
                let deadline = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
                call("revoke", attempt.revoke(deadline))?;
            }
            Phase::Revoking => call("destroy", attempt.destroy(&cleanup_deadlines(&io.clock)))?,
            Phase::Destroying => {
                let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
                attempt
                    .seal(deadline)
                    .map(|_| ())
                    .map_err(|err| format!("seal: {err:?}"))?;
            }
            other => return Err(format!("drive stuck at {other:?}")),
        }
    }
    Err(format!(
        "did not reach {target:?}, at {:?}",
        attempt.phase()
    ))
}

pub(super) fn finish(
    attempt: &mut Machine<'_>,
    io: &Handles,
    present: bool,
) -> Result<AttemptReceipt, String> {
    if attempt.phase() == Phase::Revoking {
        push_guest(io, ok_body("ok"));
        let deadline = RevokeDeadline::after(io.clock.now(), RevokeDeadline::CAP).unwrap();
        match attempt.revoke(deadline) {
            Ok(()) | Err(StepError::Cleanup(_)) => {}
            Err(err) => return Err(format!("finish revoke: {err:?}")),
        }
        if present {
            push_present(io);
        } else {
            push_absent(io);
        }
        attempt
            .destroy(&cleanup_deadlines(&io.clock))
            .map_err(|err| format!("finish destroy: {err:?}"))?;
    } else if attempt.phase() != Phase::Destroying {
        return Err(format!("finish from {:?}", attempt.phase()));
    }
    let deadline = SealDeadline::after(io.clock.now(), SealDeadline::CAP).unwrap();
    attempt
        .seal(deadline)
        .map_err(|err| format!("finish seal: {err:?}"))
}

pub(super) fn runner_commands(io: &Handles) -> Vec<TartCommand> {
    io.runner
        .borrow()
        .trace()
        .iter()
        .map(|entry| entry.command.clone())
        .collect()
}

pub(super) fn guest_commands(io: &Handles) -> Vec<GuestCommand> {
    io.guest
        .borrow()
        .trace()
        .iter()
        .map(|entry| entry.command.clone())
        .collect()
}

pub(super) fn assert_ids(commands: &[TartCommand], allowed: &[&str]) -> Result<(), String> {
    for command in commands {
        let named = match command {
            TartCommand::Clone { clone, .. }
            | TartCommand::Start { clone, .. }
            | TartCommand::Stop { clone }
            | TartCommand::Delete { clone }
            | TartCommand::Status { clone } => Some(clone.as_str()),
            TartCommand::List => None,
        };
        if let Some(named) = named {
            if !allowed.contains(&named) {
                return Err(format!("command named {named}: {command:?}"));
            }
        }
    }
    Ok(())
}

pub(super) fn run_count(io: &Handles) -> usize {
    guest_commands(io)
        .iter()
        .filter(|command| matches!(command, GuestCommand::Run))
        .count()
}

pub(super) fn start_ids(io: &Handles) -> Vec<String> {
    runner_commands(io)
        .iter()
        .filter_map(|command| match command {
            TartCommand::Start { clone, .. } => Some(clone.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

pub(super) fn advance(clock: &FakeClock, step: Duration) -> Result<(), String> {
    clock
        .advance(step)
        .map_err(|err| format!("advance: {err:?}"))
}
