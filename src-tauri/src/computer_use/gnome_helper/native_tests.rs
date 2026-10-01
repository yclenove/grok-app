use super::*;
use grok_computer_use_wayland::HelperExtension;

struct Fixture {
    root: PathBuf,
    bundle: Bundle,
    shell: ShellStamp,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("cu-helper-state-{}", uuid::Uuid::new_v4()));
        let bundle = Bundle::new(root.clone());
        let shell = ShellStamp {
            bus: "a".repeat(32),
            owner: ":1.99".into(),
            process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:1:10".into(),
        };
        Self {
            root,
            bundle,
            shell,
        }
    }
    fn snapshot(&self) -> HelperSnapshot {
        HelperSnapshot {
            shell_version: "46.1".into(),
            user_extensions_enabled: true,
            extension: Some(HelperExtension {
                state: 2,
                enabled: false,
                path: self.bundle.path().to_string_lossy().into_owned(),
                per_user: true,
            }),
            health: HelperHealth::Absent,
        }
    }
    fn state(&self, snapshot: &HelperSnapshot, shell: &ShellStamp) -> &'static str {
        present(
            &self.bundle,
            self.bundle.inspect().unwrap(),
            snapshot,
            shell,
        )
        .state
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn disk_installation_cannot_claim_ready_in_the_old_shell_or_before_discovery() {
    let f = Fixture::new();
    let mut snapshot = f.snapshot();
    assert_eq!(f.state(&snapshot, &f.shell), "missing");
    f.bundle.install(false, &f.shell).unwrap();
    snapshot.extension.as_mut().unwrap().state = 1;
    snapshot.extension.as_mut().unwrap().enabled = true;
    snapshot.health = HelperHealth::Ready;
    assert_eq!(f.state(&snapshot, &f.shell), "restart_required");
    let next = ShellStamp {
        process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:2:11".into(),
        ..f.shell.clone()
    };
    assert_eq!(f.state(&snapshot, &next), "ready");
    snapshot.extension = None;
    snapshot.health = HelperHealth::Absent;
    assert_eq!(f.state(&snapshot, &next), "restart_required");
}
#[test]
fn live_disabled_global_block_and_ambiguous_health_are_distinct() {
    let f = Fixture::new();
    f.bundle.install(false, &f.shell).unwrap();
    let next = ShellStamp {
        process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:2:11".into(),
        ..f.shell.clone()
    };
    let mut snapshot = f.snapshot();
    assert_eq!(f.state(&snapshot, &next), "disabled");
    snapshot.user_extensions_enabled = false;
    assert_eq!(f.state(&snapshot, &next), "global_disabled");
    snapshot.user_extensions_enabled = true;
    snapshot.health = HelperHealth::Blocked;
    assert_eq!(f.state(&snapshot, &next), "blocked");
    snapshot.health = HelperHealth::Unknown;
    assert_eq!(f.state(&snapshot, &next), "unconfirmed");
    snapshot.shell_version = "47.0".into();
    assert_eq!(f.state(&snapshot, &next), "unsupported");
}
#[test]
fn foreign_shell_path_and_disk_damage_never_claim_ready() {
    let f = Fixture::new();
    f.bundle.install(false, &f.shell).unwrap();
    let mut snapshot = f.snapshot();
    snapshot.extension.as_mut().unwrap().path = "/usr/share/gnome-shell/extensions/foreign".into();
    assert_eq!(f.state(&snapshot, &f.shell), "conflict");
    snapshot = f.snapshot();
    snapshot.extension.as_mut().unwrap().per_user = false;
    assert_eq!(f.state(&snapshot, &f.shell), "conflict");
    snapshot = f.snapshot();
    std::fs::write(f.bundle.path().join("policy.js"), "damaged").unwrap();
    assert_eq!(f.state(&snapshot, &f.shell), "repair_required");
    std::fs::write(f.bundle.path().join("user-file"), "preserve").unwrap();
    assert_eq!(f.state(&snapshot, &f.shell), "conflict");
}

#[test]
fn sticky_shell_error_exposes_repair_after_confirmed_disable_but_never_enable() {
    let f = Fixture::new();
    f.bundle.install(false, &f.shell).unwrap();
    let next = ShellStamp {
        process: "b35fd080-b9b4-4d74-8aeb-3269565ed550:2:11".into(),
        ..f.shell.clone()
    };
    for state in [3, 4, 99] {
        let mut snapshot = f.snapshot();
        snapshot.extension.as_mut().unwrap().state = state;
        snapshot.extension.as_mut().unwrap().enabled = true;
        let status = present(&f.bundle, Installation::Current, &snapshot, &next);
        assert_eq!(status.state, "restart_required");
        assert!(!status.actions.contains(&HelperAction::Repair));
        assert!(!status.actions.contains(&HelperAction::Enable));
        snapshot.extension.as_mut().unwrap().enabled = false;
        let status = present(&f.bundle, Installation::Current, &snapshot, &next);
        assert_eq!(status.state, "restart_required");
        assert!(status.actions.contains(&HelperAction::Repair));
        assert!(!status.actions.contains(&HelperAction::Enable));
        snapshot.health = HelperHealth::Unknown;
        let status = present(&f.bundle, Installation::Current, &snapshot, &next);
        assert!(!status.actions.contains(&HelperAction::Repair));
    }
}
