use super::*;
use grok_computer_use_core::runtime::{NewComponent, PLACEHOLDER_JS_RUNTIME};
use std::sync::Arc;

struct Fixture {
    root: PathBuf,
    installation: Installation,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("grok-cu-first-chat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let resource = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        Self {
            root,
            installation: Installation {
                executable: resource.clone(),
                resource,
            },
        }
    }

    fn store(&self) -> RuntimeStore {
        RuntimeStore::new(self.root.clone())
    }

    fn verify_private_node(&self) {
        let node = self.store().resolve(JS_RUNTIME).unwrap();
        assert!(node.starts_with(&self.root));
        let version = crate::process_util::command(node)
            .arg("--version")
            .output()
            .unwrap();
        assert!(version.status.success());
        assert_eq!(
            String::from_utf8(version.stdout).unwrap().trim(),
            "v20.18.0"
        );
        for (id, expected) in [
            (MCP_SERVER, SCRIPT),
            (grok_computer_use_core::runtime::MCP_PROTOCOL, PROTOCOL),
        ] {
            assert_eq!(
                std::fs::read(self.store().resolve(id).unwrap()).unwrap(),
                expected.as_bytes()
            );
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "Preserved original first-chat runtime fixture: {}",
                self.root.display()
            );
        } else {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }
}

#[test]
fn first_chat_initializes_bundled_runtime_in_an_empty_private_home() {
    let f = Fixture::new();
    assert!(f.store().resolve(JS_RUNTIME).is_err());
    initialize(&f.root, Some(&f.installation)).unwrap();
    f.verify_private_node();
    let pointer = std::fs::read(f.root.join("current.json")).unwrap();
    initialize(&f.root, None).unwrap();
    assert_eq!(std::fs::read(f.root.join("current.json")).unwrap(), pointer);
}

#[test]
fn first_chat_initializes_an_exact_app_created_mcp_only_pack() {
    let f = Fixture::new();
    f.store().ensure_embedded_mcp(SCRIPT, PROTOCOL).unwrap();
    assert!(f.store().resolve(JS_RUNTIME).is_err());
    initialize(&f.root, Some(&f.installation)).unwrap();
    f.verify_private_node();
}

#[test]
fn first_chat_keeps_a_broken_existing_runtime_for_explicit_repair() {
    let f = Fixture::new();
    f.store()
        .install_pack(
            "broken",
            vec![NewComponent {
                id: JS_RUNTIME.into(),
                version: "broken".into(),
                relpath: "bin/node".into(),
                bytes: PLACEHOLDER_JS_RUNTIME.to_vec(),
            }],
        )
        .unwrap();
    f.store().activate_pack("broken").unwrap();
    let pointer = std::fs::read(f.root.join("current.json")).unwrap();
    assert!(initialize(&f.root, Some(&f.installation)).is_err());
    assert_eq!(std::fs::read(f.root.join("current.json")).unwrap(), pointer);
    assert_eq!(
        std::fs::read(f.root.join("packs/broken/bin/node")).unwrap(),
        PLACEHOLDER_JS_RUNTIME
    );
}

#[test]
fn first_chat_preserves_a_corrupt_pointer_and_interrupted_preparation() {
    let f = Fixture::new();
    std::fs::write(f.root.join("current.json"), b"original corrupt pointer").unwrap();
    assert!(initialize(&f.root, Some(&f.installation)).is_err());
    assert_eq!(
        std::fs::read(f.root.join("current.json")).unwrap(),
        b"original corrupt pointer"
    );
    std::fs::remove_file(f.root.join("current.json")).unwrap();
    let staging = f.root.join(".staging/original-owner");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("evidence"), b"keep original").unwrap();
    assert!(initialize(&f.root, Some(&f.installation)).is_err());
    assert_eq!(
        std::fs::read(staging.join("evidence")).unwrap(),
        b"keep original"
    );
    assert!(!f.root.join("current.json").exists());
}

#[test]
fn first_chat_without_an_installer_does_not_use_system_node() {
    let f = Fixture::new();
    assert!(initialize(&f.root, None)
        .unwrap_err()
        .contains("PATH is not used"));
    let missing = Installation {
        resource: f.root.clone(),
        executable: f.root.clone(),
    };
    assert!(initialize(&f.root, Some(&missing)).is_err());
    assert!(!f.root.join("current.json").exists());
}

#[test]
fn first_chat_concurrent_connections_initialize_one_active_pack() {
    let f = Arc::new(Fixture::new());
    let jobs: Vec<_> = (0..2)
        .map(|_| {
            let f = f.clone();
            std::thread::spawn(move || initialize(&f.root, Some(&f.installation)))
        })
        .collect();
    for job in jobs {
        job.join().unwrap().unwrap();
    }
    f.verify_private_node();
    assert_eq!(std::fs::read_dir(f.root.join("packs")).unwrap().count(), 1);
}
