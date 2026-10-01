//! Manual original-Host management tests on the explicitly owned installed VM.
//! No renderer IPC, portal grant, physical-human or release acceptance claim.
use super::*;

fn owned_vm() {
    use std::os::unix::fs::MetadataExt;
    const ID: &str = "812400f8-a6c6-4c38-a735-2b8d0ef8d3e2";
    assert_eq!(
        std::env::var("GROK_CU_OWNED_VM_ACCEPTANCE").as_deref(),
        Ok(ID)
    );
    let marker = std::path::Path::new("/etc/cu-owned-vm-id");
    let metadata = std::fs::symlink_metadata(marker).unwrap();
    assert!(metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0);
    assert_eq!(std::fs::read_to_string(marker).unwrap().trim(), ID);
    assert_eq!(std::env::var("HOME").as_deref(), Ok("/home/cuaccept"));
    assert_eq!(
        std::env::var("XDG_RUNTIME_DIR").as_deref(),
        Ok("/run/user/1000")
    );
    assert_eq!(std::env::var("XDG_SESSION_TYPE").as_deref(), Ok("wayland"));
    assert!(!super::super::super::feature::feature_enabled());
}

async fn expect_state(expected: &str) {
    let state = status().await.unwrap();
    println!("VM_HOST_STATUS {}", serde_json::to_string(&state).unwrap());
    assert_eq!(state.state, expected);
}

#[tokio::test]
#[ignore = "requires the owned installed Ubuntu GNOME VM, before helper install"]
async fn installed_vm_install_requires_new_shell_process() {
    owned_vm();
    expect_state("missing").await;
    act(HelperAction::Install).await.unwrap();
    expect_state("restart_required").await;
    assert!(act(HelperAction::Enable).await.is_err());
    assert_eq!(
        Bundle::new(root().unwrap()).inspect().unwrap(),
        Installation::Current
    );
}

#[tokio::test]
#[ignore = "requires owned installed GNOME VM with an older, disabled managed helper"]
async fn installed_vm_upgrade_embedded_helper_requires_new_shell_process() {
    owned_vm();
    expect_state("repair_required").await;
    assert!(GnomeHelperControl::connect()
        .await
        .unwrap()
        .snapshot()
        .await
        .unwrap()
        .inactive());
    act(HelperAction::Repair).await.unwrap();
    assert_eq!(
        Bundle::new(root().unwrap()).inspect().unwrap(),
        Installation::Current
    );
    expect_state("restart_required").await;
    assert!(act(HelperAction::Enable).await.is_err());
    assert!(!super::super::super::feature::feature_enabled());
}

#[tokio::test]
#[ignore = "requires owned installed GNOME Wayland, after a new Shell process"]
async fn installed_vm_enable_disable_enable_reads_live_endpoint() {
    owned_vm();
    expect_state("disabled").await;
    let control = GnomeHelperControl::connect().await.unwrap();
    println!("VM_SHELL_PROCESS {}", control.shell_process);
    for _ in 0..2 {
        act(HelperAction::Enable).await.unwrap();
        expect_state("ready").await;
        assert!(control.snapshot().await.unwrap().active());
        act(HelperAction::Disable).await.unwrap();
        expect_state("disabled").await;
        assert!(control.snapshot().await.unwrap().inactive());
    }
    act(HelperAction::Enable).await.unwrap();
    expect_state("ready").await;
    assert!(!super::super::super::feature::feature_enabled());
}

#[tokio::test]
#[ignore = "requires owned installed GNOME VM after the live input observations"]
async fn installed_vm_disable_and_repair_requires_new_shell_process() {
    owned_vm();
    act(HelperAction::Disable).await.unwrap();
    expect_state("disabled").await;
    let bundle = Bundle::new(root().unwrap());
    std::fs::write(
        bundle.path().join("policy.js"),
        "// owned VM repair fixture\n",
    )
    .unwrap();
    expect_state("repair_required").await;
    act(HelperAction::Repair).await.unwrap();
    assert_eq!(bundle.inspect().unwrap(), Installation::Current);
    expect_state("restart_required").await;
    assert!(act(HelperAction::Enable).await.is_err());
}

#[tokio::test]
#[ignore = "requires owned installed GNOME VM after repair and another login"]
async fn installed_vm_repaired_helper_enables_and_finishes_disabled() {
    owned_vm();
    expect_state("disabled").await;
    act(HelperAction::Enable).await.unwrap();
    expect_state("ready").await;
    act(HelperAction::Disable).await.unwrap();
    expect_state("disabled").await;
    assert!(GnomeHelperControl::connect()
        .await
        .unwrap()
        .snapshot()
        .await
        .unwrap()
        .inactive());
}

// This probe never creates a portal session or grants input. In particular its
// Host policy remains false throughout; only real login1/Shell/helper binding
// and joined observer ownership are exercised, not App grant acceptance.
struct ReadOnlyPolicyProbe;
impl grok_computer_use_wayland::PortalInputPolicy for ReadOnlyPolicyProbe {
    fn input_available(&self) -> bool {
        false
    }
    fn user_input_active(&self) -> bool {
        false
    }
}

#[tokio::test]
#[ignore = "requires owned installed GNOME VM, SSH process and explicitly enabled helper"]
async fn installed_vm_ssh_process_cannot_borrow_graphical_policy() {
    use grok_computer_use_wayland::GnomeNativePolicyWatch;
    owned_vm();
    expect_state("ready").await;
    let error =
        match GnomeNativePolicyWatch::connect(std::sync::Arc::new(ReadOnlyPolicyProbe)).await {
            Ok(watch) => {
                drop(watch);
                panic!("SSH process borrowed graphical user-manager Display");
            }
            Err(error) => error,
        };
    assert!(
        error.contains("not this user's unlocked active local Wayland session"),
        "{error}"
    );
    println!("VM_NATIVE_POLICY_ACTUAL_SSH_SESSION_REJECTED {error}");
}

#[tokio::test]
#[ignore = "requires owned installed GNOME VM, graphical launcher and explicitly enabled helper"]
async fn installed_vm_graphical_process_connects_native_policy() {
    use grok_computer_use_wayland::GnomeNativePolicyWatch;
    owned_vm();
    expect_state("ready").await;
    println!("VM_NATIVE_POLICY_PROBE_PID {}", std::process::id());
    let watch = GnomeNativePolicyWatch::connect(std::sync::Arc::new(ReadOnlyPolicyProbe))
        .await
        .expect("the actual graphical application must bind real login1/Shell/helper owners");
    let policy = watch.input_policy();
    assert!(!policy.input_available());
    let (send, receive) = tokio::sync::oneshot::channel();
    let run = watch.run_until(receive);
    let check = async {
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        assert!(
            !policy.input_available(),
            "read-only probe cannot grant input"
        );
        send.send(()).expect("original watch must still be alive");
    };
    let (result, ()) = tokio::join!(run, check);
    result.expect("original native observer must stop and join successfully");
    assert!(!policy.input_available());
    println!("VM_NATIVE_POLICY_READONLY_CONNECTED_AND_JOINED");
}
