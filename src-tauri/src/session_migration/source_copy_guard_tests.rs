use super::*;

struct TempTree(PathBuf);

impl TempTree {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "grok-source-copy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        #[cfg(not(windows))]
        let path = fs::canonicalize(path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        remove_tree(&self.0);
    }
}

fn remove_tree(root: &Path) {
    if let Ok(read) = fs::read_dir(root) {
        for item in read.flatten() {
            let path = item.path();
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if is_reparse(&meta) {
                    let _ = fs::remove_dir(&path);
                    let _ = fs::remove_file(&path);
                    continue;
                }
                if meta.is_dir() {
                    remove_tree(&path);
                }
            }
        }
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn network_device_relative_and_empty_are_refused_before_io() {
    assert!(matches!(
        classify_requested_root(Path::new("")),
        Err(GuardError::MissingSourceCopy)
    ));
    assert!(matches!(
        classify_requested_root(Path::new("   ")),
        Err(GuardError::MissingSourceCopy)
    ));
    assert!(matches!(
        classify_requested_root(Path::new(r"relative\copy")),
        Err(GuardError::RelativePath)
    ));
    assert!(matches!(
        classify_requested_root(Path::new(r"\\server\share\copy")),
        Err(GuardError::NetworkOrDevice)
    ));
    assert!(matches!(
        classify_requested_root(Path::new(r"\\.\PhysicalDrive0")),
        Err(GuardError::NetworkOrDevice)
    ));
    assert!(matches!(
        classify_requested_root(Path::new("NUL")),
        Err(GuardError::NetworkOrDevice)
    ));
}

#[test]
fn live_profile_is_refused_without_stat_even_if_missing() {
    // A native absolute path: on Unix `C:\...` is a relative file name.
    let root = if cfg!(windows) {
        PathBuf::from(r"C:\missing-audit-live\.codex")
    } else {
        PathBuf::from("/missing-audit-live/.codex")
    };
    let err = classify_against(&root, &[("codex-home", root.as_path())]).unwrap_err();
    assert!(matches!(err, GuardError::LiveProfile { kind } if kind == "codex-home"));
    let child = root.join("sessions").join("a.jsonl");
    let err = classify_against(&child, &[("codex-home", root.as_path())]).unwrap_err();
    assert!(matches!(err, GuardError::LiveProfile { kind } if kind == "codex-home"));
    assert!(!root.exists());
}

#[test]
fn known_live_roots_cover_provider_homes_without_listing() {
    let roots = known_live_profile_roots();
    let kinds: Vec<&str> = roots.iter().map(|(kind, _)| *kind).collect();
    for kind in [
        "codex-home",
        "claude-config",
        "cursor-user",
        "cursor-home",
        "grok-home",
    ] {
        assert!(kinds.contains(&kind), "{kind} missing from {kinds:?}");
    }
    if std::env::var_os("APPDATA").is_some() {
        assert!(kinds.contains(&"grok-appdata"));
    }
}

#[test]
fn live_profile_ancestors_are_refused_before_io() {
    let tree = TempTree::new();
    let live = tree.path().join("synthetic-user").join(".codex");
    let roots = [("synthetic-codex", live.as_path())];
    for ancestor in live.ancestors().skip(1) {
        let err = classify_against(ancestor, &roots).unwrap_err();
        assert!(matches!(err, GuardError::LiveProfile { kind } if kind == "synthetic-codex"));
    }
    // Classification must not require the live profile to exist or enumerate it.
    assert!(!live.exists());
}

#[test]
fn normalized_live_profile_ancestors_are_refused_before_io() {
    let tree = TempTree::new();
    let parent = tree.path().join("synthetic-user");
    let live = parent.join(".claude");
    let candidate = parent.join("isolated-copy").join("..");
    let err = classify_against(&candidate, &[("synthetic-claude", live.as_path())]).unwrap_err();
    assert!(matches!(err, GuardError::LiveProfile { kind } if kind == "synthetic-claude"));
    assert!(!parent.exists());
}

#[test]
fn isolated_copy_siblings_are_not_live_profile_ancestors() {
    let tree = TempTree::new();
    let live = tree.path().join("synthetic-user").join(".codex");
    let roots = [("synthetic-codex", live.as_path())];
    for candidate in [
        tree.path().join("isolated-copy"),
        tree.path().join("synthetic-user").join(".codex-copy"),
        tree.path().join("synthetic-user-copy"),
    ] {
        assert_eq!(classify_against(&candidate, &roots), Ok(()));
    }
}

#[test]
fn credential_directory_is_refused_without_reading_nested_secret() {
    let tree = TempTree::new();
    let marker = "ZZCREDENTIALMARKER9f3a";
    let cred = tree.path().join("auth.json");
    fs::create_dir(&cred).unwrap();
    let secret = cred.join("secret.txt");
    fs::write(&secret, marker).unwrap();
    #[cfg(windows)]
    let _hold = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&secret)
            .unwrap()
    };
    let err = audit_isolated_copy(tree.path(), "codex").unwrap_err();
    let shown = err.to_string();
    assert!(matches!(err, GuardError::CredentialFile { .. }), "{shown}");
    assert!(!shown.contains(marker));
    #[cfg(windows)]
    drop(_hold);
    assert_eq!(fs::read_to_string(&secret).unwrap(), marker);
}

#[test]
fn junction_audit_root_is_refused_before_reading_the_target() {
    let outside = TempTree::new();
    let parent = TempTree::new();
    let marker = "ZZROOTJUNCTIONMARKER9f3a";
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, marker).unwrap();
    let link = parent.path().join("root-link");
    assert!(
        make_dir_link(&link, outside.path()),
        "directory link could not be created"
    );
    #[cfg(windows)]
    let _hold = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&secret)
            .unwrap()
    };
    let err = audit_isolated_copy(&link, "cursor").unwrap_err();
    let shown = err.to_string();
    assert!(matches!(err, GuardError::SymlinkEscape { .. }), "{shown}");
    assert!(!shown.contains(marker), "{shown}");
    #[cfg(windows)]
    drop(_hold);
    assert_eq!(fs::read_to_string(&secret).unwrap(), marker);
}

#[test]
fn junction_escape_is_refused_without_reading_the_target() {
    let root = TempTree::new();
    let outside = TempTree::new();
    let marker = "ZZJUNCTIONMARKER9f3a";
    fs::write(outside.path().join("secret.txt"), marker).unwrap();
    let link = root.path().join("escape-link");
    assert!(
        make_dir_link(&link, outside.path()),
        "directory link could not be created"
    );
    let err = audit_isolated_copy(root.path(), "cursor").unwrap_err();
    let shown = err.to_string();
    assert!(matches!(err, GuardError::SymlinkEscape { .. }), "{shown}");
    assert!(!shown.contains(marker));
    assert_eq!(
        fs::read_to_string(outside.path().join("secret.txt")).unwrap(),
        marker
    );
}

#[test]
fn junction_ancestor_is_refused_before_reading_the_audit_root() {
    let outside = TempTree::new();
    let parent = TempTree::new();
    let target = outside.path().join("nested").join("isolated-copy");
    fs::create_dir_all(&target).unwrap();
    let marker = "ZZANCESTORJUNCTIONMARKER9f3a";
    let secret = target.join("secret.txt");
    fs::write(&secret, marker).unwrap();
    let link = parent.path().join("parent-link");
    assert!(
        make_dir_link(&link, outside.path()),
        "directory link could not be created"
    );
    #[cfg(windows)]
    let _hold = {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&secret)
            .unwrap()
    };
    let root = link.join("nested").join("isolated-copy");
    let err = audit_isolated_copy(&root, "cursor").unwrap_err();
    let shown = err.to_string();
    assert!(matches!(err, GuardError::SymlinkEscape { .. }), "{shown}");
    assert!(!shown.contains(marker), "{shown}");
    assert!(!shown.contains(&parent.path().to_string_lossy().into_owned()));
    #[cfg(windows)]
    drop(_hold);
    assert_eq!(fs::read_to_string(&secret).unwrap(), marker);
}

#[test]
fn stable_copy_hashes_match_and_jsonl_values_stay_out_of_the_report() {
    let tree = TempTree::new();
    let dir = tree.path().join("隔离 副本");
    fs::create_dir_all(&dir).unwrap();
    let marker = "ZZBODYMARKER9f3a";
    fs::write(
            dir.join("笔记.jsonl"),
            format!(
                "{{\"role\":\"user\",\"type\":\"message\",\"text\":\"{marker}\"}}\n{{\"role\":\"assistant\"\n"
            ),
        )
        .unwrap();
    let report = audit_isolated_copy(tree.path(), "claude-code").unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(report.source_unchanged);
    assert_eq!(report.provider_hint, "claude-code");
    assert_eq!(report.file_count, 1);
    assert!(report
        .format_families
        .iter()
        .any(|family| family == "jsonl"));
    assert_eq!(report.field_coverage.jsonl_records, 1);
    assert_eq!(report.field_coverage.jsonl_partial_tail, 1);
    assert_eq!(report.field_coverage.keys.get("role"), Some(&1));
    assert_eq!(report.field_coverage.keys.get("type"), Some(&1));
    assert!(!encoded.contains(marker), "{encoded}");
    assert!(report.peak_working_set_bytes.unwrap_or(0) > 0 || !cfg!(windows));
    let again = audit_isolated_copy(tree.path(), "claude-code").unwrap();
    assert_eq!(report.manifest_sha256, again.manifest_sha256);
}

#[test]
fn append_between_passes_reports_source_changed_without_body() {
    let tree = TempTree::new();
    let file = tree.path().join("note.jsonl");
    let marker = "ZZCHANGEMARKER9f3a";
    fs::write(&file, "{\"role\":\"user\"}\n").unwrap();
    let err = audit_isolated_copy_between(tree.path(), "generic-file", || {
        fs::write(
            &file,
            format!("{{\"role\":\"user\",\"text\":\"{marker}\"}}\n"),
        )
        .unwrap();
    })
    .unwrap_err();
    let shown = err.to_string();
    assert!(matches!(err, GuardError::SourceChanged), "{shown}");
    assert!(!shown.contains(marker));
}

#[test]
fn cursor_sidecars_are_counted_without_opening_sqlite() {
    let tree = TempTree::new();
    let marker = "ZZSQLMARKER9f3a";
    let workspace = tree.path().join("workspaceStorage").join("abc");
    let global = tree.path().join("globalStorage");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&global).unwrap();
    fs::write(workspace.join("state.vscdb"), marker).unwrap();
    fs::write(workspace.join("state.vscdb-wal"), marker).unwrap();
    fs::write(tree.path().join("store.db"), marker).unwrap();
    fs::write(tree.path().join("store.db-wal"), marker).unwrap();
    fs::write(tree.path().join("store.db-shm"), marker).unwrap();
    fs::write(tree.path().join("store.db-journal"), marker).unwrap();
    fs::write(global.join("state.vscdb"), marker).unwrap();
    let report = audit_isolated_copy(tree.path(), "cursor").unwrap();
    let encoded = serde_json::to_string(&report).unwrap();
    assert!(report.source_unchanged);
    assert_eq!(report.cursor.database_files, 2);
    assert_eq!(report.cursor.companion_databases, 1);
    assert_eq!(report.cursor.wal_files, 2);
    assert_eq!(report.cursor.shm_files, 1);
    assert_eq!(report.cursor.wal_without_shm, 1);
    assert_eq!(report.cursor.rollback_journals, 1);
    assert!(report.cursor.rollback_journal_present);
    assert!(!encoded.contains(marker), "{encoded}");
}

#[cfg(windows)]
#[test]
fn locked_file_reports_busy_without_repair() {
    use std::os::windows::fs::OpenOptionsExt;
    let tree = TempTree::new();
    let file = tree.path().join("locked.jsonl");
    fs::write(&file, "{\"role\":\"user\"}\n").unwrap();
    let _hold = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let err = audit_isolated_copy(tree.path(), "codex").unwrap_err();
    assert!(matches!(err, GuardError::SourceBusy { .. }), "{err}");
    assert!(file.is_file());
}

#[test]
fn audit_text_redacts_email_token_and_username() {
    let raw = "mail user@example.com zzlocal9f3a@example.com Bearer abcdefghijklmnop sk-zzzzzzzzzzzzzzzz C:\\Users\\alice\\.codex\\auth.json";
    let out = redact_audit_text(raw);
    assert!(!out.contains("user@example.com"), "{out}");
    assert!(!out.contains("zzlocal9f3a"), "{out}");
    assert!(!out.contains("abcdefghijklmnop"), "{out}");
    assert!(!out.contains("zzzzzzzzzzzzzzzz"), "{out}");
    assert!(!out.contains("alice"), "{out}");
    assert!(out.contains("[redacted-email]"));
    assert!(out.contains("[redacted-secret]"));
    assert!(out.contains("[redacted-user]"));
}

#[test]
fn opt_in_env_absent_does_not_select_a_default_profile() {
    match std::env::var("GROK_SOURCE_COPY_AUDIT") {
        Ok(raw) if !raw.trim().is_empty() => {
            let report = audit_isolated_copy(Path::new(raw.trim()), "unspecified")
                .expect("explicit source copy audit must succeed");
            assert!(report.source_unchanged);
            let encoded = redact_audit_text(&serde_json::to_string(&report).expect("report json"));
            assert!(!encoded.contains("sk-"), "{encoded}");
            println!("{encoded}");
        }
        _ => {
            assert!(matches!(
                classify_requested_root(Path::new("")),
                Err(GuardError::MissingSourceCopy)
            ));
        }
    }
}

#[test]
fn provider_hint_with_secret_text_is_replaced() {
    let tree = TempTree::new();
    fs::write(tree.path().join("empty.txt"), b"ok").unwrap();
    let report = audit_isolated_copy(tree.path(), "Bearer sk-zzzzzzzzzzzzzzzz").unwrap();
    assert_eq!(report.provider_hint, "unspecified");
}

fn make_dir_link(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}
