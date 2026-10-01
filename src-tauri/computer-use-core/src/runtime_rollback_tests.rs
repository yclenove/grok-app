use super::*;

fn rollback_fixture() -> (RuntimeStore, PathBuf, String) {
    let (store, root) = super::tests::store();
    let bundle = super::tests::write_full_seed_bundle(&root);
    let previous = store.install_from_bundle(&bundle).unwrap();
    store
        .install_pack(
            "new-pack",
            vec![NewComponent {
                id: JS_RUNTIME.into(),
                version: "1".into(),
                relpath: "bin/node".into(),
                bytes: b"MZ-new-node".to_vec(),
            }],
        )
        .unwrap();
    store.activate_pack("new-pack").unwrap();
    (store, root, previous)
}

fn rewrite_manifest(store: &RuntimeStore, pack: &str, change: impl FnOnce(&mut RuntimeManifest)) {
    let path = store.pack_dir(pack).join("manifest.json");
    let mut manifest = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut manifest);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

fn assert_refused_without_pointer_write(store: &RuntimeStore) -> String {
    let before = fs::read(store.pointer_path()).unwrap();
    let err = store
        .rollback_pack()
        .expect_err("unsafe previous pack must be refused");
    assert_eq!(fs::read(store.pointer_path()).unwrap(), before, "{err}");
    err
}

#[test]
fn rollback_refuses_corrupt_component_and_preserves_current_pointer() {
    let (store, root, previous) = rollback_fixture();
    fs::write(
        store
            .pack_dir(&previous)
            .join(super::tests::fixture_target().node_relpath()),
        b"tampered",
    )
    .unwrap();
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("hash_mismatch"), "{err}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_refuses_missing_worker_sibling_and_preserves_current_pointer() {
    let (store, root, previous) = rollback_fixture();
    fs::remove_file(store.pack_dir(&previous).join("playwright/loopback.mjs")).unwrap();
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("missing_sibling"), "{err}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_refuses_cross_arch_and_preserves_current_pointer() {
    let (store, root, previous) = rollback_fixture();
    rewrite_manifest(&store, &previous, |m| {
        m.components.get_mut(JS_RUNTIME).unwrap().arch = "wrong-arch".into()
    });
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("arch_mismatch"), "{err}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_refuses_incomplete_and_incompatible_manifests() {
    for change in 0..4 {
        let (store, root, previous) = rollback_fixture();
        rewrite_manifest(&store, &previous, |m| match change {
            0 => {
                m.components.remove(MCP_SERVER);
            }
            1 => m.pack_id = "different-pack".into(),
            2 => m.schema_version = PACK_SCHEMA_VERSION + 1,
            _ => m.compatibility.protocol = 99,
        });
        assert_refused_without_pointer_write(&store);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rollback_refuses_escaped_pack_and_component_paths() {
    let (store, root, previous) = rollback_fixture();
    rewrite_manifest(&store, &previous, |m| {
        m.components.get_mut(JS_RUNTIME).unwrap().relpath = "../outside".into()
    });
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("path"), "{err}");
    let mut pointer = store.read_pointer();
    pointer.previous = Some("../bundle".into());
    store.write_pointer(&pointer).unwrap();
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("pack id"), "{err}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_refuses_symlinked_component() {
    let (store, root, previous) = rollback_fixture();
    let component = store
        .pack_dir(&previous)
        .join(super::tests::fixture_target().node_relpath());
    let outside = root.join("outside-node");
    fs::copy(&component, &outside).unwrap();
    fs::remove_file(&component).unwrap();
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_file(&outside, &component);
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&outside, &component);
    if let Err(error) = link {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("symlink"), "{err}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_refuses_symlinked_worker_sibling_with_matching_bytes() {
    let (store, root, previous) = rollback_fixture();
    let sibling = store.pack_dir(&previous).join("playwright/loopback.mjs");
    let outside = root.join("outside-loopback.mjs");
    fs::copy(&sibling, &outside).unwrap();
    fs::remove_file(&sibling).unwrap();
    #[cfg(windows)]
    let link = std::os::windows::fs::symlink_file(&outside, &sibling);
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&outside, &sibling);
    if let Err(error) = link {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let err = assert_refused_without_pointer_write(&store);
    assert!(err.contains("missing_sibling"), "{err}");
    fs::remove_dir_all(root).unwrap();
}
