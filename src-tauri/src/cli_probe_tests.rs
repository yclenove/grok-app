use super::*;

#[test]
fn extracts_version_from_common_banners() {
    assert_eq!(
        extract_version_token("grok 0.2.112").as_deref(),
        Some("0.2.112")
    );
    assert_eq!(
        extract_version_token("grok-cli v0.2.112").as_deref(),
        Some("0.2.112")
    );
    assert_eq!(
        extract_version_token("grok 0.2.112 (a1b2c3d)").as_deref(),
        Some("0.2.112")
    );
    assert_eq!(extract_version_token("0.2.101").as_deref(), Some("0.2.101"));
    assert_eq!(extract_version_token("1.0").as_deref(), Some("1.0"));
}

#[test]
fn version_extraction_ignores_non_version_tokens() {
    // A git hash or plain words must not be mistaken for a version.
    assert_eq!(extract_version_token("grok"), None);
    assert_eq!(extract_version_token("build a1b2c3d"), None);
    assert_eq!(extract_version_token(""), None);
}

#[test]
fn version_gate_flags_old_cli() {
    // The exact pairing reported in the field: 0.2.101 fails, 0.2.112 passes.
    assert_eq!(cli_version_supported("grok 0.2.101"), Some(false));
    assert_eq!(cli_version_supported("grok 0.2.112"), Some(true));
    assert_eq!(cli_version_supported("grok 0.2.200"), Some(true));
    assert_eq!(cli_version_supported("grok 0.3.0"), Some(true));
    assert_eq!(cli_version_supported("grok 1.0.0"), Some(true));
    assert_eq!(cli_version_supported("grok 0.1.999"), Some(false));
}

#[test]
fn unparseable_version_is_not_treated_as_too_old() {
    // Fail open: an unknown banner must not block a working CLI.
    assert_eq!(cli_version_supported("grok"), None);
    assert_eq!(cli_version_supported(""), None);
}

#[test]
fn min_version_string_matches_constant() {
    assert_eq!(min_cli_version_str(), "0.2.112");
}

#[test]
fn recommended_line_is_1_0_0() {
    assert_eq!(recommended_cli_version_str(), "1.0.0");
    assert_eq!(cli_meets_recommended("grok 1.0.0"), Some(true));
    assert_eq!(cli_meets_recommended("grok 1.0.0 (abc)"), Some(true));
    assert_eq!(cli_meets_recommended("grok 0.2.120"), Some(false));
    assert_eq!(cli_meets_recommended("grok 0.2.112"), Some(false));
    assert_eq!(cli_meets_recommended("nope"), None);
}

#[test]
fn version_tokens_skew_detects_mismatch() {
    assert!(!version_tokens_skew(None, Some("1.0.0")));
    assert!(!version_tokens_skew(Some("grok 1.0.0"), Some("1.0.0")));
    assert!(version_tokens_skew(
        Some("grok 1.0.0"),
        Some("grok 0.2.118")
    ));
    assert!(!version_tokens_skew(Some("grok"), Some("1.0.0")));
}

#[test]
fn acp_agent_version_cache_drives_probe_skew() {
    // Exercise the same cache operations on a local cache: ACP fixture
    // tests may concurrently record banners in the production global.
    let cache = Mutex::new(None);
    assert!(cached_agent_version(&cache).is_none());
    record_agent_version_in(&cache, None);
    assert!(cached_agent_version(&cache).is_none());
    record_agent_version_in(&cache, Some("  "));
    assert!(cached_agent_version(&cache).is_none());
    record_agent_version_in(&cache, Some("  grok 0.2.118 (deadbeef)  "));
    assert_eq!(
        cached_agent_version(&cache).as_deref(),
        Some("grok 0.2.118 (deadbeef)")
    );
    assert!(version_tokens_skew(
        Some("grok 1.0.0"),
        cached_agent_version(&cache).as_deref()
    ));
    assert!(!version_tokens_skew(
        Some("grok 0.2.118"),
        cached_agent_version(&cache).as_deref()
    ));
    record_agent_version_in(&cache, None);
    record_agent_version_in(&cache, Some(" "));
    assert_eq!(
        cached_agent_version(&cache).as_deref(),
        Some("grok 0.2.118 (deadbeef)")
    );
    record_agent_version_in(&cache, Some("grok 1.0.0"));
    assert_eq!(cached_agent_version(&cache).as_deref(), Some("grok 1.0.0"));
}

#[test]
fn probe_returns_structure() {
    let r = probe_candidates(
        vec![PathBuf::from("missing-grok")],
        false,
        false,
        |_| false,
        |_| panic!("missing candidate must not execute"),
        |_| panic!("missing CLI must not discover a sidecar"),
        || None,
    );
    assert_eq!(r.candidates_tried, vec!["missing-grok"]);
    assert!(!r.found);
    assert_eq!(r.source, "not_found");
    assert!(r.path.is_none());
    assert!(r.version.is_none());
    assert!(!r.cli_auth_present);
    assert_eq!(r.min_version, "0.2.112");
    assert_eq!(r.recommended_version, "1.0.0");
    assert!(r.version_supported.is_none());
    assert!(r.meets_recommended.is_none());
    assert!(r.agent_path.is_none());
    assert!(r.agent_version.is_none());
    assert!(!r.agent_binary_skew);
    assert!(!r.acp_agent_version_skew);
}

#[test]
fn probe_finds_synthetic_install() {
    let root = test_support::LauncherDir::new();
    let bin = root
        .path()
        .join(".grok")
        .join("bin")
        .join("CLI fixture 中文");
    fs::create_dir_all(&bin).unwrap();
    let path = test_support::version_launcher(&bin, "grok 1.0.0", false, 0);
    let r = probe_candidates(
        vec![path.clone()],
        false,
        true,
        is_executable,
        read_version,
        |_| (None, None),
        || None,
    );
    assert!(r.found);
    assert_eq!(r.path.as_deref(), path.to_str());
    assert_eq!(r.version.as_deref(), Some("grok 1.0.0"));
    assert_eq!(r.source, "common_path");
    assert!(r.cli_auth_present);
    assert_eq!(r.version_supported, Some(true));
    assert_eq!(r.meets_recommended, Some(true));
}

fn fixture_probe(candidates: &[(&str, bool, Option<&str>)], manual: bool) -> CliProbeResult {
    probe_candidates(
        candidates
            .iter()
            .map(|(path, _, _)| PathBuf::from(path))
            .collect(),
        manual,
        false,
        |path| {
            candidates
                .iter()
                .find(|(p, _, _)| Path::new(p) == path)
                .unwrap()
                .1
        },
        |path| {
            candidates
                .iter()
                .find(|(p, _, _)| Path::new(p) == path)
                .unwrap()
                .2
                .map(str::to_owned)
        },
        |_| (None, None),
        || None,
    )
}

#[test]
fn probe_manual_versioned_candidate_wins_without_executing_later_paths() {
    let mut executed = Vec::new();
    let r = probe_candidates(
        vec![PathBuf::from("manual"), PathBuf::from("automatic")],
        true,
        false,
        |_| true,
        |path| {
            executed.push(path.to_path_buf());
            Some("grok 1.0.0".into())
        },
        |_| (None, None),
        || None,
    );
    assert_eq!(r.path.as_deref(), Some("manual"));
    assert_eq!(r.source, "manual");
    assert_eq!(executed, vec![PathBuf::from("manual")]);
    assert_eq!(r.candidates_tried, vec!["manual", "automatic"]);
}

#[test]
fn probe_versioned_candidate_wins_over_manual_fallback() {
    let r = fixture_probe(
        &[
            ("manual", true, None),
            (".grok/bin/grok", true, Some("grok 1.0.0")),
        ],
        true,
    );
    assert_eq!(r.path.as_deref(), Some(".grok/bin/grok"));
    assert_eq!(r.source, "common_path");
    assert_eq!(r.version.as_deref(), Some("grok 1.0.0"));
}

#[test]
fn probe_all_version_failures_keep_first_runnable_fallback() {
    let r = fixture_probe(
        &[
            ("missing", false, None),
            ("first", true, None),
            ("last", true, None),
        ],
        true,
    );
    assert!(r.found);
    assert_eq!(r.path.as_deref(), Some("first"));
    assert_eq!(r.source, "path");
    assert!(r.version.is_none());
    assert!(r.version_supported.is_none());
    assert!(r.meets_recommended.is_none());
}

#[test]
fn probe_manual_fallback_retains_manual_source() {
    let r = fixture_probe(&[("manual", true, None)], true);
    assert!(r.found);
    assert_eq!(r.source, "manual");
}

#[test]
fn probe_old_and_unknown_version_banners_remain_found() {
    for (banner, supported, recommended) in [
        ("grok 0.2.101", Some(false), Some(false)),
        ("grok 0.2.112", Some(true), Some(false)),
        ("unknown build", None, None),
    ] {
        let r = fixture_probe(&[("candidate", true, Some(banner))], false);
        assert!(r.found);
        assert_eq!(r.version.as_deref(), Some(banner));
        assert_eq!(r.version_supported, supported);
        assert_eq!(r.meets_recommended, recommended);
    }
}

#[test]
fn probe_sidecar_and_acp_skew_use_explicit_inputs() {
    for (sidecar, acp, sidecar_skew, acp_skew) in [
        (Some("grok 0.2.118"), Some("grok 0.2.120"), true, true),
        (Some("1.0.0"), Some("grok 1.0.0 (hash)"), false, false),
        (None, Some("unknown build"), false, false),
    ] {
        let r = probe_candidates(
            vec![PathBuf::from("candidate")],
            false,
            false,
            |_| true,
            |_| Some("grok 1.0.0".into()),
            |path| {
                assert_eq!(path, Some("candidate"));
                (Some("synthetic-agent".into()), sidecar.map(str::to_owned))
            },
            || acp.map(str::to_owned),
        );
        assert_eq!(r.agent_path.as_deref(), Some("synthetic-agent"));
        assert_eq!(r.agent_version.as_deref(), sidecar);
        assert_eq!(r.acp_agent_version.as_deref(), acp);
        assert_eq!(r.agent_binary_skew, sidecar_skew);
        assert_eq!(r.acp_agent_version_skew, acp_skew);
    }
}

#[test]
fn probe_empty_candidate_list_is_not_found() {
    let r = probe_candidates(
        vec![],
        true,
        true,
        |_| panic!("no candidates"),
        |_| panic!("no candidates"),
        |_| panic!("no sidecar"),
        || None,
    );
    assert!(!r.found);
    assert!(r.candidates_tried.is_empty());
    assert!(r.cli_auth_present);
}

#[test]
fn version_probe_executes_only_synthetic_launchers() {
    for (banner, stderr, exit_code, expected) in [
        ("grok 1.0.0", false, 0, Some("grok 1.0.0")),
        ("grok 0.2.112", true, 1, Some("grok 0.2.112")),
        ("not a CLI banner", true, 1, None),
        ("grok 1.0.0", false, 1, None),
    ] {
        let root = test_support::LauncherDir::new();
        let path = test_support::version_launcher(root.path(), banner, stderr, exit_code);
        assert_eq!(read_version(&path).as_deref(), expected);
    }
    let root = test_support::LauncherDir::new();
    assert!(read_version(&root.path().join("missing-launcher")).is_none());
}

#[test]
fn version_cache_does_not_keep_a_removed_synthetic_launcher_available() {
    let root = test_support::LauncherDir::new();
    let path = test_support::version_launcher(root.path(), "grok 1.0.0", false, 0);
    assert_eq!(read_version_of(&path).as_deref(), Some("grok 1.0.0"));
    assert_eq!(read_version_of(&path).as_deref(), Some("grok 1.0.0"));
    fs::remove_file(&path).unwrap();
    assert!(read_version_of(&path).is_none());
    assert!(read_version(&path).is_none());
}

#[test]
fn version_probe_rejects_a_directory_at_a_cached_launcher_path() {
    let root = test_support::LauncherDir::new();
    let path = test_support::version_launcher(root.path(), "grok 1.0.0", false, 0);
    assert_eq!(read_version_of(&path).as_deref(), Some("grok 1.0.0"));
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(read_version_of(&path).is_none());
    assert!(read_version(&path).is_none());
}

#[cfg(unix)]
#[test]
fn version_cache_rechecks_execute_permission_without_an_mtime_change() {
    use std::os::unix::fs::PermissionsExt;

    let root = test_support::LauncherDir::new();
    let path = test_support::version_launcher(root.path(), "grok 1.0.0", false, 0);
    assert_eq!(read_version_of(&path).as_deref(), Some("grok 1.0.0"));
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), mtime);
    assert!(read_version_of(&path).is_none());
    assert!(read_version(&path).is_none());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(read_version_of(&path).as_deref(), Some("grok 1.0.0"));
}

#[test]
fn launcher_directory_creation_failure_preserves_existing_contents() {
    let root = test_support::LauncherDir::new();
    let existing = root.path().join("already-owned");
    fs::create_dir(&existing).unwrap();
    let marker = existing.join("owner-marker");
    fs::write(&marker, b"preserve this owner").unwrap();

    let result = test_support::LauncherDir::create(existing.clone());
    assert!(matches!(result, Err(ref error) if error.kind() == std::io::ErrorKind::AlreadyExists));
    assert!(existing.is_dir());
    assert_eq!(fs::read(marker).unwrap(), b"preserve this owner");

    let occupied = root.path().join("occupied-file");
    fs::write(&occupied, b"preserve this file").unwrap();
    assert!(test_support::LauncherDir::create(occupied.clone()).is_err());
    assert_eq!(fs::read(occupied).unwrap(), b"preserve this file");
}

#[test]
fn launcher_directory_releases_only_its_successfully_created_directory() {
    let root = test_support::LauncherDir::new();
    let owned_path = root.path().join("new-owner");
    let owner = test_support::LauncherDir::create(owned_path.clone()).unwrap();
    fs::write(owner.path().join("synthetic-marker"), b"owned").unwrap();
    drop(owner);
    assert!(!owned_path.exists());
    assert!(root.path().is_dir());
}

#[test]
fn version_launcher_rejects_shell_syntax_before_creating_a_script() {
    for banner in [
        "grok & command",
        "grok 'quote'",
        "grok\ncommand",
        "grok %PATH%",
        "grok !",
    ] {
        let root = test_support::LauncherDir::new();
        assert!(std::panic::catch_unwind(|| {
            test_support::version_launcher(root.path(), banner, false, 0)
        })
        .is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn auth_path_uses_user_home() {
    // Just ensure we don't panic; presence is machine-dependent.
    let _ = cli_auth_json_present();
}

#[test]
fn expand_user_path_tilde() {
    let p = expand_user_path("~/foo/bar");
    assert!(p.starts_with(user_home()));
    assert!(
        p.ends_with(Path::new("foo").join("bar"))
            || p.to_string_lossy().ends_with("foo/bar")
            || p.to_string_lossy().ends_with(r"foo\bar")
    );
}

#[test]
fn expand_user_path_empty() {
    assert!(expand_user_path("  ").as_os_str().is_empty());
}

#[test]
fn candidates_include_platform_defaults() {
    let c = candidate_paths(None);
    let joined = c
        .iter()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    // Both platforms should consider the official bin dir.
    assert!(
        joined.contains(".grok") && (joined.contains("bin") || joined.contains("grok")),
        "candidates missing official layout: {c:?}"
    );
}

#[test]
fn manual_path_is_first_candidate() {
    let fake = if cfg!(target_os = "windows") {
        r"C:\custom\grok.exe"
    } else {
        "/custom/grok"
    };
    let c = candidate_paths(Some(fake));
    assert!(!c.is_empty());
    assert_eq!(c[0], PathBuf::from(fake));
}
