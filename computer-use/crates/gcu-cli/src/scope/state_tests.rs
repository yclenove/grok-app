//! Control authority tests use no platform clock or driver.
use super::*;
use gcu_host_protocol::scope::{Control, Scope, VERSION};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
struct FakeClock(AtomicU64);
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
}
fn scope() -> Scope {
    Scope {
        engine_instance: "fake-engine".into(),
        desktop_session: "fake-desktop".into(),
        control_lease: "private-lease".into(),
        targets: vec![],
    }
}
fn install(generation: u64) -> Control {
    let scope = scope();
    Control::Install {
        version: VERSION,
        run_id: "run".into(),
        generation,
        scope_digest: scope.digest("run", generation).unwrap(),
        scope,
    }
}
fn active() -> Machine<FakeClock> {
    let mut state = Machine::new(FakeClock::default());
    assert_eq!(state.phase(), Phase::Unbound);
    state.apply(install(1)).unwrap();
    state
}
#[test]
fn install_valid_ack_binds_digest_and_authority() {
    let mut state = Machine::new(FakeClock::default());
    assert!(state.lease().is_none());
    let ack = state.apply(install(1)).unwrap().unwrap();
    assert_eq!(ack.scope_digest, scope().digest("run", 1).unwrap());
    assert_eq!(state.phase(), Phase::Active);
    assert!(state.lease().unwrap().arm_attempt().is_some());
}
#[test]
fn replay_or_run_mismatch_revokes_instead_of_ignoring() {
    for frame in [
        install(2),
        Control::Rebind {
            run_id: "run".into(),
            generation: 1,
            scope: scope(),
        },
        Control::Rebind {
            run_id: "run".into(),
            generation: 0,
            scope: scope(),
        },
        Control::Renew {
            run_id: "other".into(),
            generation: 1,
            seq: 1,
        },
        Control::Renew {
            run_id: "run".into(),
            generation: 2,
            seq: 1,
        },
        Control::Revoke {
            run_id: "other".into(),
        },
    ] {
        let mut state = active();
        let lease = state.lease().unwrap();
        assert!(state.apply(frame).is_err());
        assert_eq!(state.phase(), Phase::Revoked);
        assert!(lease.current().is_none());
        assert!(state.apply(install(9)).is_err());
    }
    let mut state = active();
    let renew = Control::Renew {
        run_id: "run".into(),
        generation: 1,
        seq: 1,
    };
    state.apply(renew.clone()).unwrap();
    assert!(state.apply(renew).is_err());
    assert_eq!(state.phase(), Phase::Revoked);
}
#[test]
fn rebind_new_generation_invalidates_old_attempt() {
    let mut state = active();
    let lease = state.lease().unwrap();
    let mut attempt = lease.arm_attempt().unwrap();
    let ack = state
        .apply(Control::Rebind {
            run_id: "run".into(),
            generation: 2,
            scope: scope(),
        })
        .unwrap()
        .unwrap();
    assert_eq!(ack.generation, 2);
    assert!(!attempt.confirm(&lease));
    assert_eq!(lease.arm_attempt().unwrap().scope().generation(), 2);
}
#[test]
fn expiry_uses_engine_clock_and_revocation_is_terminal() {
    let mut state = active();
    state
        .clock
        .0
        .store(LEASE.as_millis() as u64, Ordering::SeqCst);
    assert!(state.check_time().is_err());
    assert_eq!(state.phase(), Phase::Revoked);
    assert!(state.apply(install(2)).is_err());
}
#[test]
fn late_renew_cannot_resurrect_expired_lease() {
    let mut state = active();
    state
        .clock
        .0
        .store(LEASE.as_millis() as u64, Ordering::SeqCst);
    assert!(state
        .apply(Control::Renew {
            run_id: "run".into(),
            generation: 1,
            seq: 1
        })
        .is_err());
    assert_eq!(state.phase(), Phase::Revoked);
}
#[test]
fn unknown_version_and_wrong_digest_fail_closed() {
    for frame in [
        Control::Install {
            version: VERSION + 1,
            run_id: "run".into(),
            generation: 1,
            scope_digest: scope().digest("run", 1).unwrap(),
            scope: scope(),
        },
        Control::Install {
            version: VERSION,
            run_id: "run".into(),
            generation: 1,
            scope_digest: "wrong".into(),
            scope: scope(),
        },
    ] {
        let mut state = Machine::new(FakeClock::default());
        assert!(state.apply(frame).is_err());
        assert_eq!(state.phase(), Phase::Revoked);
    }
}

#[test]
fn valid_renew_extends_deadline_but_cannot_change_generation() {
    let mut state = active();
    state.clock.0.store(2000, Ordering::SeqCst);
    state
        .apply(Control::Renew {
            run_id: "run".into(),
            generation: 1,
            seq: 1,
        })
        .unwrap();
    state.clock.0.store(3001, Ordering::SeqCst);
    assert!(state.check_time().is_ok());
    assert_eq!(state.lease().unwrap().current().unwrap().generation(), 1);
    state.clock.0.store(5000, Ordering::SeqCst);
    assert!(state.check_time().is_err());
}
#[test]
fn revoke_then_new_install_is_terminal_even_with_new_generation() {
    let mut state = active();
    state
        .apply(Control::Revoke {
            run_id: "run".into(),
        })
        .unwrap();
    assert!(state.apply(install(99)).is_err());
    assert!(state.lease().is_none());
}
