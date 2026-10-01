use super::*;
use crate::browser::{ExistingTabHost, TabAttachment};

const ORIGIN: &str = "chrome-extension://lease-extension";

fn paired() -> (ExtensionPairing, PairingSession, PairingConnection) {
    let mut pairing = ExtensionPairing::new("lease-instance", "lease-extension");
    let challenge = pairing.begin_challenge();
    pairing.confirm_app_for(&challenge.nonce).unwrap();
    let session = pairing
        .complete_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    let binding = PairingConnection {
        instance_id: session.instance_id.clone(),
        connection_nonce: session.connection_nonce.clone(),
        generation: session.generation,
    };
    (pairing, session, binding)
}

#[test]
fn exact_monotonic_deadline_is_a_dispatch_and_renewal_boundary() {
    let (mut pairing, session, binding) = paired();
    let deadline = pairing.lease_deadline.unwrap();
    assert!(pairing
        .connect_at(
            ORIGIN,
            Some("lease-extension"),
            Some(&session.session_key),
            deadline - Duration::from_nanos(1)
        )
        .is_ok());
    for now in [deadline, deadline + Duration::from_secs(1)] {
        assert!(pairing
            .connect_at(
                ORIGIN,
                Some("lease-extension"),
                Some(&session.session_key),
                now
            )
            .is_err());
        assert!(pairing
            .heartbeat_at(ORIGIN, &session.session_key, &binding, now)
            .is_err());
        assert_eq!(pairing.lease_deadline, Some(deadline));
    }
}

#[test]
fn only_the_complete_live_binding_can_extend_a_lease() {
    let (mut pairing, session, binding) = paired();
    let old_deadline = pairing.lease_deadline.unwrap();
    let now = old_deadline - Duration::from_secs(5);
    for field in 0..4 {
        let mut wrong = binding.clone();
        let token = if field == 0 {
            "wrong"
        } else {
            &session.session_key
        };
        match field {
            1 => wrong.instance_id.push_str("-old"),
            2 => wrong.connection_nonce.push_str("-old"),
            3 => wrong.generation += 1,
            _ => {}
        }
        assert!(pairing.heartbeat_at(ORIGIN, token, &wrong, now).is_err());
        assert_eq!(pairing.lease_deadline, Some(old_deadline));
    }
    assert!(pairing
        .heartbeat_at(
            "https://untrusted.invalid",
            &session.session_key,
            &binding,
            now
        )
        .is_err());
    assert_eq!(pairing.lease_deadline, Some(old_deadline));
    pairing
        .heartbeat_at(ORIGIN, &session.session_key, &binding, now)
        .unwrap();
    assert_eq!(pairing.lease_deadline, Some(now + CONNECTION_LEASE));
    assert!(pairing
        .authenticate_connection_at(ORIGIN, &session.session_key, &binding, old_deadline)
        .is_ok());
}

#[test]
fn expiry_and_revoke_never_allow_a_late_heartbeat_to_restore_a_key() {
    let (mut pairing, session, binding) = paired();
    pairing.expire_connection_for_test();
    assert!(pairing.session_key().is_none());
    assert!(pairing.expire_connection());
    assert!(!pairing.expire_connection());
    assert!(pairing.session_key.is_none());
    assert!(pairing
        .heartbeat_connection(ORIGIN, &session.session_key, &binding)
        .is_err());
    let challenge = pairing.begin_challenge();
    pairing.confirm_app_for(&challenge.nonce).unwrap();
    let fresh = pairing
        .complete_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000002",
        ))
        .unwrap();
    let deadline = pairing.lease_deadline;
    assert!(pairing
        .heartbeat_connection(ORIGIN, &session.session_key, &binding)
        .is_err());
    assert_eq!(pairing.lease_deadline, deadline);
    assert!(pairing.session_key() == Some(fresh.session_key.as_str()));
    pairing.revoke();
    assert!(pairing.lease_deadline.is_none());
}

#[test]
fn expiry_returns_borrowed_tabs_without_closing_them_and_fences_before_sweep() {
    let host = ExistingTabHost::new();
    host.set_installed_extension_id("lease-extension");
    let challenge = host.begin_pairing_challenge();
    host.confirm_pairing_app_for(&challenge.nonce).unwrap();
    let session = host
        .complete_pairing_request(&ExtensionPairing::proof_for(
            &challenge,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap();
    host.share_and_grant(TabAttachment {
        session: "session",
        run_id: "run",
        tab_id: "shared-tab",
        title: "owned fixture",
        url: "https://fixture.invalid/",
        origin: ORIGIN,
        extension_id: Some("lease-extension"),
        pairing_token: Some(&session.session_key),
        home_index: Some(0),
        document_generation: Some(1),
        connection_generation: Some(session.generation),
        focused: true,
    })
    .unwrap();
    assert!(host.act("run", "shared-tab").is_ok());
    host.expire_pairing_connection_for_test();
    assert!(host.act("run", "shared-tab").is_err());
    assert!(host.tab_write_identity("run", "shared-tab").is_err());
    assert!(host.expire_pairing_connection());
    assert!(host.list_shared_candidates().is_empty());
    assert!(host.list_for_run("run").is_empty());
    // The lease sweep already returned the grant; user navigation still updates
    // this retained tab record without resurrecting a borrow or closing the tab.
    let returned = host
        .note_user_navigation("shared-tab", "https://fixture.invalid/after-expiry")
        .unwrap();
    assert!(!returned.closed);
    assert!(!returned.borrowed);
}
