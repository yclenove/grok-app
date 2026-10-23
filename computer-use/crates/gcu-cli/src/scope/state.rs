//! Sole scope writer. The runtime calls this only for private control frames or
//! terminal channel/clock failure. Data handlers receive a lease, never Machine.
use gcu_core::scope::{self as core, ScopeLease};
use gcu_host_protocol::{
    scope::{Ack, Control, Scope, LEASE, VERSION},
    Error,
};
use std::time::{Duration, Instant};

pub(super) trait Clock {
    fn now(&self) -> Duration;
}
pub(super) struct Monotonic(Instant);
impl Default for Monotonic {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for Monotonic {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Unbound,
    Active,
    Revoked,
}

enum State {
    Unbound,
    Active {
        lease: ScopeLease,
        record: Scope,
        run: String,
        generation: u64,
        seq: u64,
        deadline: Duration,
    },
    Revoked,
}
pub(super) struct Machine<C: Clock> {
    clock: C,
    state: State,
}
impl<C: Clock> Machine<C> {
    pub(super) fn new(clock: C) -> Self {
        Self {
            clock,
            state: State::Unbound,
        }
    }
    pub(super) fn phase(&self) -> Phase {
        match self.state {
            State::Unbound => Phase::Unbound,
            State::Active { .. } => Phase::Active,
            State::Revoked => Phase::Revoked,
        }
    }
    pub(super) fn lease(&self) -> Option<ScopeLease> {
        match &self.state {
            State::Active { lease, .. } => Some(lease.clone()),
            _ => None,
        }
    }
    pub(super) fn revoke(&mut self) {
        if let State::Active { lease, .. } = &self.state {
            lease.revoke();
        }
        self.state = State::Revoked;
    }
    pub(super) fn check_time(&mut self) -> Result<(), Error> {
        if matches!(&self.state, State::Active {deadline,..} if self.clock.now() >= *deadline) {
            self.revoke();
        }
        if self.phase() == Phase::Revoked {
            Err(Error::Disconnected)
        } else {
            Ok(())
        }
    }
    pub(super) fn remaining(&self) -> Duration {
        match &self.state {
            State::Active { deadline, .. } => deadline.saturating_sub(self.clock.now()),
            _ => Duration::ZERO,
        }
    }
    pub(super) fn apply(&mut self, frame: Control) -> Result<Option<Ack>, Error> {
        self.check_time()?;
        let result = self.transition(frame);
        if result.is_err() {
            self.revoke();
        }
        result
    }
    fn transition(&mut self, frame: Control) -> Result<Option<Ack>, Error> {
        let deadline = self.clock.now().checked_add(LEASE).ok_or(Error::Invalid)?;
        match (&mut self.state, frame) {
            (
                State::Unbound,
                Control::Install {
                    version,
                    run_id,
                    generation,
                    scope_digest,
                    scope,
                },
            ) => {
                if version != VERSION || scope.digest(&run_id, generation)? != scope_digest {
                    return Err(Error::Invalid);
                }
                let lease = ScopeLease::install(to_core(&run_id, generation, &scope)?);
                let ack = Ack {
                    run_id: run_id.clone(),
                    generation,
                    scope_digest,
                };
                self.state = State::Active {
                    lease,
                    record: scope,
                    run: run_id,
                    generation,
                    seq: 0,
                    deadline,
                };
                Ok(Some(ack))
            }
            (
                State::Active {
                    lease,
                    record,
                    run,
                    generation,
                    seq,
                    deadline: expires,
                },
                Control::Rebind {
                    run_id,
                    generation: next,
                    scope,
                },
            ) => {
                if &run_id != run
                    || next <= *generation
                    || scope.engine_instance != record.engine_instance
                    || scope.desktop_session != record.desktop_session
                    || scope.control_lease != record.control_lease
                {
                    return Err(Error::Invalid);
                }
                let next_scope = to_core(&run_id, next, &scope)?;
                let digest = scope.digest(&run_id, next)?;
                lease.rebind(next_scope).map_err(|_| Error::Invalid)?;
                *generation = next;
                *seq = 0;
                *expires = deadline;
                *record = scope;
                Ok(Some(Ack {
                    run_id,
                    generation: next,
                    scope_digest: digest,
                }))
            }
            (
                State::Active {
                    run,
                    generation,
                    seq,
                    deadline: expires,
                    ..
                },
                Control::Renew {
                    run_id,
                    generation: next,
                    seq: next_seq,
                },
            ) => {
                if &run_id != run || next != *generation || next_seq <= *seq {
                    return Err(Error::Invalid);
                }
                *seq = next_seq;
                *expires = deadline;
                Ok(None)
            }
            (State::Active { run, .. }, Control::Revoke { run_id }) if run_id == *run => {
                self.revoke();
                Ok(None)
            }
            _ => Err(Error::Invalid),
        }
    }
}
impl<C: Clock> Drop for Machine<C> {
    fn drop(&mut self) {
        self.revoke();
    }
}
fn instance(
    i: &gcu_host_protocol::scope::Instance,
) -> Result<core::ProcessInstance, core::ScopeError> {
    core::ProcessInstance::new(
        i.pid,
        core::OsStartTime::new(i.os_start_time)?,
        core::BootIdentity::new(&i.boot_identity)?,
    )
}
fn to_core(run: &str, generation: u64, scope: &Scope) -> Result<core::FixtureScope, Error> {
    let convert = || -> Result<_, core::ScopeError> {
        let targets = scope
            .targets
            .iter()
            .map(|target| {
                let windows = target
                    .windows
                    .iter()
                    .map(|win| {
                        Ok(core::RegisteredWindow::new(core::WindowBinding::new(
                            &win.native_id,
                            win.incarnation,
                            instance(&win.owner_instance)?,
                        )?))
                    })
                    .collect::<Result<Vec<_>, core::ScopeError>>()?;
                core::FixtureTarget::new(
                    instance(&target.instance)?,
                    &target.role,
                    core::ExecutableProof::new(&target.executable_proof)?,
                    windows,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        core::FixtureScope::try_new(
            run,
            generation,
            &scope.engine_instance,
            &scope.desktop_session,
            &scope.control_lease,
            targets,
        )
    };
    convert().map_err(|_| Error::Invalid)
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
