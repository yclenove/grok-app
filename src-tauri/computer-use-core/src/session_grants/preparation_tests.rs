use super::*;
use crate::{broker::BrokerOptions, fake::FakeAdapter};

fn broker() -> ComputerUseBroker {
    ComputerUseBroker::new(
        Arc::new(FakeAdapter::new()),
        BrokerOptions {
            feature_enabled: true,
            lease_path: std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()),
            ..BrokerOptions::default()
        },
    )
}

#[test]
fn native_resource_identity_requires_exact_preparation_and_untampered_metadata() {
    let broker = broker();
    let grants = SessionGrants::default();
    let ticket = grants.begin(&broker, "chat", None).unwrap();
    let clone = ticket.clone();
    assert!(ticket.same_preparation(&clone));
    let other = grants.begin(&broker, "other", None).unwrap();
    let mut forged = other;
    forged.session = ticket.session.clone();
    forged.run_id = ticket.run_id.clone();
    forged.attempt_id = ticket.attempt_id.clone();
    forged.selector_revision = ticket.selector_revision;
    forged.generation = ticket.generation;
    forged.nonce = ticket.nonce.clone();
    assert!(!ticket.same_preparation(&forged));
    for field in 0..6 {
        let mut forged = ticket.clone();
        match field {
            0 => forged.session.push('x'),
            1 => forged.run_id.push('x'),
            2 => forged.attempt_id.push('x'),
            3 => forged.selector_revision += 1,
            4 => forged.generation += 1,
            _ => forged.nonce.push('x'),
        }
        assert!(!ticket.same_preparation(&forged));
    }
    grants.fence_fail_checked(&broker, &ticket, || {});
    assert!(ticket.is_preparation_cancelled());
    assert!(
        ticket.same_preparation(&clone),
        "identity is not renewed authority"
    );
    let next = grants.begin(&broker, "next", None).unwrap();
    let cloned = next.clone();
    grants.publish(&broker, &next, true, || {}).unwrap();
    assert!(next.check_preparation().is_err());
    assert!(next.same_preparation(&cloned));
}

#[tokio::test]
async fn each_pending_retirement_wakes_all_and_late_preparation_waiters() {
    for transition in 0..5 {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "attempt", 1)
            .unwrap();
        let mut waiters = Vec::new();
        for _ in 0..8 {
            let ticket = ticket.clone();
            waiters.push(tokio::spawn(
                async move { ticket.preparation_cancelled().await },
            ));
        }
        tokio::task::yield_now().await;
        match transition {
            0 => {
                grants.fence_revoke_checked(&broker, "chat", || {});
            }
            1 => {
                grants
                    .fence_cancel_attempt_checked(&broker, "chat", "attempt", 1, || {})
                    .unwrap();
            }
            2 => {
                grants.fence_fail_checked(&broker, &ticket, || {});
            }
            3 => grants.forget_session("chat"),
            4 => drop(grants),
            _ => unreachable!(),
        }
        for waiter in waiters {
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .unwrap()
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(1), ticket.preparation_cancelled())
            .await
            .unwrap();
        assert!(ticket.check_preparation().is_err());
        assert!(ticket.is_preparation_cancelled());
    }
}

#[test]
fn completed_preparation_cannot_restart_and_is_not_revoked_by_replacement() {
    let broker = broker();
    let grants = SessionGrants::default();
    let original = grants.begin(&broker, "chat", None).unwrap();
    grants.publish(&broker, &original, false, || {}).unwrap();
    assert!(original.check_preparation().is_ok());
    grants.publish(&broker, &original, true, || {}).unwrap();
    assert!(original
        .check_preparation()
        .unwrap_err()
        .contains("completed"));
    assert!(!original.is_preparation_cancelled());
    let next = grants
        .begin(&broker, "chat", Some(&original.run_id))
        .unwrap();
    grants.fence_fail_checked(&broker, &next, || {});
    assert!(next.is_preparation_cancelled());
    assert!(!original.is_preparation_cancelled());
}

#[test]
fn stale_or_foreign_failure_does_not_cancel_new_preparation() {
    let broker = broker();
    let grants = SessionGrants::default();
    let first = grants.begin(&broker, "chat", None).unwrap();
    grants.revoke_checked(&broker, "chat", || {}).unwrap();
    let next = grants.begin(&broker, "chat", Some(&first.run_id)).unwrap();
    assert!(
        !grants
            .fence_fail_checked(&broker, &first, || panic!("stale clear"))
            .matched
    );
    let mut forged = next.clone();
    forged.session = "foreign-chat".into();
    assert!(
        !grants
            .fence_fail_checked(&broker, &forged, || panic!("foreign clear"))
            .matched
    );
    assert!(next.check_preparation().is_ok());
    assert!(first.is_preparation_cancelled());
}

#[test]
fn catalog_exhaustion_still_revokes_pending_preparation() {
    for transition in 0..3 {
        let broker = broker();
        let grants = SessionGrants::default();
        let ticket = grants
            .begin_attempt(&broker, "chat", None, "attempt", 1)
            .unwrap();
        grants.exhaust_catalog_generation();
        let result = match transition {
            0 => grants.fence_revoke_checked(&broker, "chat", || {}),
            1 => {
                grants
                    .fence_cancel_attempt_checked(&broker, "chat", "attempt", 1, || {})
                    .unwrap()
                    .revoke
            }
            _ => grants.fence_fail_checked(&broker, &ticket, || {}),
        };
        assert!(result.error.is_some());
        assert!(ticket.is_preparation_cancelled());
        assert!(ticket.check_preparation().is_err());
    }
}

#[test]
fn cancelled_attempt_tombstone_cannot_mint_a_late_preparation_ticket() {
    let broker = broker();
    let grants = SessionGrants::default();
    grants
        .fence_cancel_attempt_checked(&broker, "chat", "late", 5, || {})
        .unwrap();
    assert!(grants
        .begin_attempt(&broker, "chat", None, "late", 5)
        .is_err());
    let independent = grants
        .begin_attempt(&broker, "other", None, "late", 5)
        .unwrap();
    assert!(independent.check_preparation().is_ok());
}

#[test]
fn completing_or_revoking_one_session_never_changes_another_signal() {
    let broker = broker();
    let grants = SessionGrants::default();
    let a = grants.begin(&broker, "a", None).unwrap();
    let b = grants.begin(&broker, "b", None).unwrap();
    grants.publish(&broker, &a, true, || {}).unwrap();
    grants.forget_session("a");
    assert!(b.check_preparation().is_ok());
    grants.fence_revoke_checked(&broker, "b", || {});
    assert!(b.is_preparation_cancelled());
    assert!(!a.is_preparation_cancelled());
}
