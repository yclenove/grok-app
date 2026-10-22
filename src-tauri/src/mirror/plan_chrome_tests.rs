//! Real mirror dispatcher and conditional chrome commands in an isolated store.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;

fn run(method: &str, params: Value, host: &MirrorHost) -> Result<Value, RpcError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(dispatch(method, params, host, None, None))
}

#[test]
fn mirror_restored_chrome_uses_the_same_conditional_writer_and_read_only_gate() {
    let _home = IsolatedAppHome::acquire_with_user_home("mirror-plan-chrome");
    let session = store::create_session(None, None, false).unwrap();
    let host = MirrorHost::from_env();
    let view = run(
        "session.planChromeGet",
        json!({"sessionId": session.id}),
        &host,
    )
    .unwrap();
    let params = json!({"sessionId": session.id, "chrome": {"userClosed": true}, "expectedRevision": view["chromeRevision"]});
    assert_eq!(
        run("session.reviewPlan", params.clone(), &host)
            .unwrap_err()
            .code,
        "UNSUPPORTED"
    );
    assert_eq!(
        run("session.resolvePlan", json!({}), &host)
            .unwrap_err()
            .code,
        "UNSUPPORTED"
    );
    assert_eq!(
        run("session.resolveAskUser", json!({}), &host)
            .unwrap_err()
            .code,
        "UNSUPPORTED"
    );
    assert!(crate::plan_chrome::load_plan_chrome(&session.id).is_none());
    host.set_read_only(false);
    run("session.reviewPlan", params.clone(), &host).unwrap();
    assert!(
        crate::plan_chrome::load_plan_chrome(&session.id)
            .unwrap()
            .user_closed
    );
    assert!(run("session.reviewPlan", params, &host).is_err());
    assert_eq!(
        run(
            "session.agentPlanSnapshot",
            json!({"sessionId": session.id}),
            &host
        )
        .unwrap()["found"],
        false
    );
}

#[test]
fn mirror_missing_identity_or_non_dismissal_never_falls_back_to_a_direct_save() {
    let _home = IsolatedAppHome::acquire_with_user_home("mirror-plan-chrome-identity");
    let session = store::create_session(None, None, false).unwrap();
    let host = MirrorHost::from_env();
    host.set_read_only(false);
    let view = run(
        "session.planChromeGet",
        json!({"sessionId": session.id}),
        &host,
    )
    .unwrap();
    for params in [
        json!({"sessionId": session.id, "chrome": {"userClosed": true}}),
        json!({"sessionId": session.id, "chrome": {"visible": false}, "expectedRevision": view["chromeRevision"]}),
        json!({"sessionId": session.id, "chrome": {"userClosed": true}, "expectedRevision": "foreign"}),
    ] {
        assert!(run("session.reviewPlan", params, &host).is_err());
    }
    assert!(crate::plan_chrome::load_plan_chrome(&session.id).is_none());
}
