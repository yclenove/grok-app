use super::*;

#[test]
fn warmed_member_codec_preserves_key_bytes_header_and_candidate_boundaries() {
    let home = IsolatedAppHome::acquire("acceptance-codec-key-boundary");
    let auth = InstallationAuthenticator::load().unwrap();
    let header_json = seal(&auth, HEADER_DOMAIN, header()).unwrap();
    let verified = verify_header(&auth, header_json.as_bytes(), &header().job_id).unwrap();
    let encoded = seal(&auth, MEMBER_DOMAIN, member(&verified)).unwrap();
    let first = verify_member(&auth, encoded.as_bytes(), &verified, &"f".repeat(64)).unwrap();
    let second = verify_member(&auth, encoded.as_bytes(), &verified, &"f".repeat(64)).unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "unchanged bytes should reuse only the pure codec fact"
    );
    assert!(verify_member(&auth, encoded.as_bytes(), &verified, &"9".repeat(64)).is_err());
    let mut other_header = header();
    other_header.context_digest = "9".repeat(64);
    let other_header = seal(&auth, HEADER_DOMAIN, other_header).unwrap();
    let other_header = verify_header(&auth, other_header.as_bytes(), &header().job_id).unwrap();
    assert!(verify_member(&auth, encoded.as_bytes(), &other_header, &"f".repeat(64)).is_err());
    let changed = encoded.replace(&"0".repeat(64), &"2".repeat(64));
    assert_ne!(changed, encoded);
    assert!(verify_member(&auth, changed.as_bytes(), &verified, &"f".repeat(64)).is_err());

    std::env::set_var("GROK_APP_HOME", home.path.join("second-synthetic-key"));
    let other_auth = InstallationAuthenticator::load().unwrap();
    assert!(verify_member(&other_auth, encoded.as_bytes(), &verified, &"f".repeat(64)).is_err());
    assert!(verify_member(&auth, encoded.as_bytes(), &verified, &"f".repeat(64)).is_ok());
}
