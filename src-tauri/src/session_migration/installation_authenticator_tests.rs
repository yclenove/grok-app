//! Actual installation MAC primitive; no accounts, model or source access.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

const DOMAIN: &str = "session-import-publication-outcome-v1";
const PAYLOAD: &[u8] = b"synthetic-canonical-outcome";

#[test]
fn loaded_authenticator_preserves_existing_wire_mac() {
    let _home = IsolatedAppHome::acquire("loaded-installation-authenticator-wire");
    let auth = InstallationAuthenticator::load().unwrap();
    // Independent original algorithm, not the newly delegated wrapper.
    let secret = load_or_create_cursor_secret().unwrap();
    let mut old = HmacSha256::new_from_slice(&secret).unwrap();
    old.update(INSTALLATION_MAC_PREFIX);
    old.update(DOMAIN.as_bytes());
    old.update(&[0]);
    old.update(PAYLOAD);
    let expected = old.finalize().into_bytes();
    assert!(auth.sign(DOMAIN, PAYLOAD).as_slice() == expected.as_slice());
    assert!(
        sign_installation_payload(DOMAIN, PAYLOAD)
            .unwrap()
            .as_slice()
            == expected.as_slice()
    );
    assert!(verify_installation_payload(DOMAIN, PAYLOAD, &expected).unwrap());
}

#[test]
fn loaded_authenticator_refuses_domain_payload_and_mac_changes() {
    let _home = IsolatedAppHome::acquire("loaded-installation-authenticator-domains");
    let auth = InstallationAuthenticator::load().unwrap();
    let seal = auth.sign(DOMAIN, PAYLOAD);
    assert!(auth.verify(DOMAIN, PAYLOAD, &seal));
    assert!(!auth.verify("published-verification-v1", PAYLOAD, &seal));
    assert!(!auth.verify(DOMAIN, b"another-synthetic-outcome", &seal));
    let mut changed = seal;
    changed[0] ^= 1;
    assert!(!auth.verify(DOMAIN, PAYLOAD, &changed));
    for len in [0, 1, 16, 31, 33, 64] {
        assert!(!auth.verify(DOMAIN, PAYLOAD, &vec![0; len]));
    }
    // Repeated calls do not consume/extend the keyed message state.
    assert!(auth.verify(DOMAIN, PAYLOAD, &auth.sign(DOMAIN, PAYLOAD)));
    assert!(auth.sign(DOMAIN, PAYLOAD) == seal);
}

#[test]
fn loaded_authenticator_does_not_access_key_files_in_writer() {
    let home = IsolatedAppHome::acquire("loaded-installation-authenticator-writer");
    let auth = InstallationAuthenticator::load().unwrap();
    let seal = auth.sign(DOMAIN, PAYLOAD);
    let path = cursor_secret_path().unwrap();
    let retained = home.path.join("synthetic-retained-key-file");
    std::fs::rename(&path, &retained).unwrap();
    std::fs::create_dir(&path).unwrap();
    // Force the shared loader to miss its cache. Only the loaded opaque crypto
    // object can operate; a hidden wrapper/load call would now hit a directory.
    *SECRET_CACHE.lock().unwrap() = None;
    let conn = crate::session_migration::catalog::open_catalog().unwrap();
    let tx = conn.unchecked_transaction().unwrap();
    let changes = tx.total_changes();
    assert!(auth.verify(DOMAIN, PAYLOAD, &seal));
    assert!(auth.sign(DOMAIN, PAYLOAD) == seal);
    assert_eq!(tx.total_changes(), changes);
    tx.rollback().unwrap();
    assert!(path.is_dir());
    assert!(std::fs::read_dir(&path).unwrap().next().is_none());
    assert!(retained.is_file());
}

#[test]
fn another_installation_cannot_verify_the_loaded_mac() {
    let seal = {
        let _home = IsolatedAppHome::acquire("loaded-installation-authenticator-first");
        InstallationAuthenticator::load()
            .unwrap()
            .sign(DOMAIN, PAYLOAD)
    };
    let _home = IsolatedAppHome::acquire("loaded-installation-authenticator-second");
    let other = InstallationAuthenticator::load().unwrap();
    assert!(!other.verify(DOMAIN, PAYLOAD, &seal));
}
