//! Manual integration probes: real GNOME Shell, original Host manager and bundle.
//! Not an installed Ubuntu desktop, OS permission, or renderer IPC acceptance.
use super::*;

fn private_fixture() {
    assert_eq!(
        std::env::var("GROK_CU_PRIVATE_SHELL_ACCEPTANCE").as_deref(),
        Ok("1")
    );
    assert_eq!(std::env::var("HOME").as_deref(), Ok("/tmp/cu-home"));
    assert_eq!(
        std::env::var("XDG_RUNTIME_DIR").as_deref(),
        Ok("/tmp/cu-runtime")
    );
    assert_eq!(
        std::fs::read_to_string("/tmp/cu-home/.private-shell-acceptance").unwrap(),
        "owned isolated GNOME Shell acceptance\n"
    );
    assert!(!super::super::super::feature::feature_enabled());
}

#[tokio::test]
#[ignore = "requires owned real-Shell PID/mount/network/D-Bus runner"]
async fn private_real_shell_install_requires_new_process() {
    private_fixture();
    let before = status().await.unwrap();
    assert_eq!(before.state, "missing");
    assert_eq!(before.actions, vec![HelperAction::Install]);
    act(HelperAction::Install).await.unwrap();
    let after = status().await.unwrap();
    assert_eq!(after.installation.as_deref(), Some("current"));
    assert_eq!(after.state, "restart_required");
    assert!(!after.actions.contains(&HelperAction::Enable));
    assert!(act(HelperAction::Enable).await.is_err());
}

#[tokio::test]
#[ignore = "requires owned real-Shell runner with intentionally absent GDM"]
async fn private_real_shell_enable_error_disables_and_repairs() {
    private_fixture();
    assert_eq!(status().await.unwrap().state, "disabled");
    let error = act(HelperAction::Enable).await.unwrap_err();
    assert!(error.contains("unconfirmed"), "{error}");
    let failed = status().await.unwrap();
    assert_eq!(failed.state, "restart_required");
    assert!(!failed.actions.contains(&HelperAction::Repair));
    act(HelperAction::Disable).await.unwrap();
    let disabled = status().await.unwrap();
    assert!(disabled.actions.contains(&HelperAction::Repair));
    assert!(!disabled.actions.contains(&HelperAction::Enable));
    assert!(act(HelperAction::Enable).await.is_err());
    let bundle = Bundle::new(root().unwrap());
    std::fs::write(
        bundle.path().join("policy.js"),
        "// intentionally damaged owned fixture\n",
    )
    .unwrap();
    assert_eq!(status().await.unwrap().state, "repair_required");
    act(HelperAction::Repair).await.unwrap();
    assert_eq!(bundle.inspect().unwrap(), Installation::Current);
    let repaired = status().await.unwrap();
    assert_eq!(repaired.state, "restart_required");
    assert!(act(HelperAction::Enable).await.is_err());
    assert!(GnomeHelperControl::connect()
        .await
        .unwrap()
        .snapshot()
        .await
        .unwrap()
        .inactive());
}

#[tokio::test]
#[ignore = "requires owned real-Shell runner after original Shell exited and joined"]
async fn private_real_shell_repaired_bundle_discovered_after_restart() {
    private_fixture();
    let current = status().await.unwrap();
    assert_eq!(current.installation.as_deref(), Some("current"));
    assert_eq!(current.state, "disabled");
    assert!(current.actions.contains(&HelperAction::Enable));
    let control = GnomeHelperControl::connect().await.unwrap();
    let snapshot = control.snapshot().await.unwrap();
    assert!(snapshot.enableable());
    assert!(
        !snapshot.active(),
        "management readiness is not desktop authority"
    );
    assert!(!Bundle::new(root().unwrap()).restart_required(&stamp(&control)));
}
