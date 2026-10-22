use super::*;
use crate::session_migration::{test_home::IsolatedAppHome, workspace_import as import};

fn workspace(folders: serde_json::Value) -> SourceWorkspace {
    source_workspace_from_code_workspace_checked(
        &serde_json::json!({ "folders": folders }).to_string(),
        "/source/app.code-workspace",
    )
    .unwrap()
}

#[test]
fn file_uris_decode_once_and_preserve_source_syntax_on_every_host() {
    for (uri, path, syntax) in [
        (
            "file:///C:/%E4%B8%AD%E6%96%87%20repo",
            r"C:\中文 repo",
            PathSyntax::Windows,
        ),
        (
            "file:///home/%E4%B8%AD%E6%96%87%20repo",
            "/home/中文 repo",
            PathSyntax::Posix,
        ),
        (
            "file://localhost/home/a%2520b",
            "/home/a%20b",
            PathSyntax::Posix,
        ),
        ("file:/home/a%23b%3Fc", "/home/a#b?c", PathSyntax::Posix),
        (
            "file:///home/repo%5Cname",
            r"/home/repo\name",
            PathSyntax::Posix,
        ),
        ("file:///home/repo%20", "/home/repo ", PathSyntax::Posix),
        ("file:///home/a/../b", "/home/a/../b", PathSyntax::Posix),
    ] {
        let result = workspace(serde_json::json!([{ "uri": uri, "name": "chosen" }]));
        let root = &result.roots[0];
        assert_eq!(root.original_path, path, "{uri}");
        assert_eq!(root.path_syntax, syntax);
        assert_eq!(root.source_uri.as_deref(), Some(uri));
        assert_eq!(root.display_name.as_deref(), Some("chosen"));
        assert!(!root.unsupported_uri);
        assert_eq!(root.root_id, root_id_for(HostKind::Local, path));
        if !can_probe_natively(syntax) {
            assert_eq!(root.availability, RootAvailability::Unresolved);
        }
    }
}

#[test]
fn local_file_uri_existing_directory_is_checked_without_reading_its_contents() {
    let home = IsolatedAppHome::acquire("workspace-uri-local");
    let path = home.path.join("中文 空格");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("do-not-import.json"), "not a transcript").unwrap();
    let uri = url::Url::from_directory_path(&path).unwrap();
    let parsed = workspace(serde_json::json!([{ "uri": uri.as_str() }]));
    assert_eq!(parsed.roots[0].availability, RootAvailability::Exists);
    assert!(!catalog_exists());
}

fn catalog_exists() -> bool {
    crate::session_migration::catalog::catalog_path().exists()
}

#[test]
fn uri_and_path_duplicates_keep_the_uri_evidence_without_extra_mapping_rows() {
    let parsed = workspace(serde_json::json!([
        { "path": "/source/repo", "name": "first" },
        { "uri": "file:///source/repo", "name": "second" },
        { "uri": "file://localhost/source/repo" }
    ]));
    assert_eq!(parsed.roots.len(), 1);
    assert_eq!(parsed.roots[0].display_name.as_deref(), Some("first"));
    assert_eq!(
        parsed.roots[0].source_uri.as_deref(),
        Some("file:///source/repo")
    );
    assert!(parsed.roots[0]
        .evidence
        .iter()
        .any(|e| e == "source-uri:file://localhost/source/repo"));
}

#[test]
fn remote_authorities_and_distros_keep_distinct_identities_without_local_probing() {
    let inputs = [
        "vscode-remote://ssh-remote+alpha/home/repo",
        "vscode-remote://ssh-remote+beta/home/repo",
        "vscode-remote://wsl+Ubuntu/home/repo",
        "vscode-remote://wsl+Debian/home/repo",
        "ssh://me@alpha:2222/home/repo",
    ];
    let parsed = workspace(serde_json::json!(
        inputs.map(|uri| serde_json::json!({ "uri": uri }))
    ));
    assert_eq!(parsed.roots.len(), inputs.len());
    let mut ids = std::collections::BTreeSet::new();
    for (root, uri) in parsed.roots.iter().zip(inputs) {
        assert_eq!(root.original_path, "/home/repo");
        assert_eq!(root.source_uri.as_deref(), Some(uri));
        assert_eq!(root.availability, RootAvailability::Offline);
        assert!(root.host_identity.is_some());
        assert!(ids.insert(&root.root_id));
        assert_ne!(root.root_id, root_id_for_path("/home/repo"));
    }
    assert_eq!(parsed.roots[0].host, HostKind::Ssh);
    assert_eq!(parsed.roots[2].host, HostKind::Wsl);
    assert_eq!(parsed.roots[2].host_identity.as_deref(), Some("wsl+Ubuntu"));
}

#[test]
fn encoded_remote_authority_aliases_are_not_discarded_by_url_userinfo_parsing() {
    let parsed = workspace(serde_json::json!([
        { "uri": "vscode-remote://ssh-remote+me%40server/home/中文" },
        { "uri": "vscode-remote://wsl+My%20Distro/home/中文" },
        { "uri": "ssh://me@[::1]:2222/home/中文" }
    ]));
    assert_eq!(
        parsed.roots[0].host_identity.as_deref(),
        Some("ssh-remote+me@server")
    );
    assert_eq!(
        parsed.roots[1].host_identity.as_deref(),
        Some("wsl+My Distro")
    );
    assert_eq!(
        parsed.roots[2].host_identity.as_deref(),
        Some("ssh:me@[::1]:2222")
    );
}

#[test]
fn network_share_uris_are_offline_metadata_and_never_local_posix_folders() {
    let parsed =
        workspace(serde_json::json!([{ "uri": "file://example.invalid/share/中文%20repo" }]));
    let root = &parsed.roots[0];
    assert_eq!(root.original_path, r"\\example.invalid\share\中文 repo");
    assert_eq!(root.path_syntax, PathSyntax::Windows);
    assert_eq!(root.availability, RootAvailability::Offline);
    assert_eq!(root.host_identity.as_deref(), Some("unc:example.invalid"));
    assert_eq!(
        classify_root(r"\\example.invalid\share").availability,
        RootAvailability::Offline
    );
    assert_eq!(classify_root("/mnt/c/repo").host, HostKind::Local);
}

#[test]
fn wsl_file_shares_keep_the_distro_and_do_not_confuse_local_verbatim_paths() {
    let parsed = workspace(serde_json::json!([
        { "uri": "file://wsl.localhost/Ubuntu/home/repo" },
        { "uri": "file://wsl$/Debian/home/repo" }
    ]));
    for (root, identity) in parsed.roots.iter().zip(["wsl+Ubuntu", "wsl+Debian"]) {
        assert_eq!(root.host, HostKind::Wsl);
        assert_eq!(root.host_identity.as_deref(), Some(identity));
        assert_eq!(root.availability, RootAvailability::Offline);
    }
    assert!(!source_paths::is_network_path(r"\\?\C:\repo"));
    assert!(source_paths::is_network_path(r"\\?\UNC\server\share"));
    assert_eq!(
        classify_root(r"\\WSL.LOCALHOST\Ubuntu\home\repo").host,
        HostKind::Wsl
    );
    let home = IsolatedAppHome::acquire("workspace-uri-verbatim");
    let canonical = home.path.canonicalize().unwrap();
    let root = classify_root(&canonical.to_string_lossy());
    assert_eq!(root.host, HostKind::Local);
    assert_eq!(root.availability, RootAvailability::Exists);
}

#[test]
fn unknown_uri_kinds_are_preserved_as_unresolved_and_can_be_mapped_later() {
    for uri in [
        "custom://authority/repo",
        "vscode-remote://dev-container+abc/workspace",
        "untitled:workspace",
    ] {
        let parsed = workspace(serde_json::json!([{ "uri": uri }]));
        let root = &parsed.roots[0];
        assert_eq!(root.original_path, uri);
        assert_eq!(root.source_uri.as_deref(), Some(uri));
        assert_eq!(root.host, HostKind::Unknown);
        assert_eq!(root.availability, RootAvailability::Unresolved);
        assert!(root.unsupported_uri);
        assert!(reading_allowed(root));
    }
}

#[test]
fn malformed_uris_and_ambiguous_folder_shapes_are_rejected() {
    for uri in [
        "relative/path",
        "file:relative",
        "file://host",
        "file:///repo?query",
        "file:///repo#fragment",
        "file:///repo%",
        "file:///repo%GG",
        "file:///repo%00",
        "file:///repo%FF",
        "file:///repo\n",
        "file://user@host/share",
        "ssh://user:password@host/repo",
        "vscode-remote://wsl+ubuntu%2Fother/repo",
        "file://host/",
        r"file:///C:\repo",
    ] {
        assert!(uri::parse(uri).is_err(), "{uri}");
    }
    for folder in [
        serde_json::json!({ "path": "a", "uri": "file:///b" }),
        serde_json::json!({ "uri": null }),
        serde_json::json!({ "uri": 42 }),
        serde_json::json!({ "path": "a", "name": [] }),
        serde_json::json!({ "path": "a\0b" }),
    ] {
        assert!(source_workspace_from_code_workspace_checked(
            &serde_json::json!({ "folders": [folder] }).to_string(),
            "/workspace/app.code-workspace"
        )
        .is_err());
    }
    assert!(uri::parse(&format!("custom:{}", "a".repeat(8192))).is_err());
}

#[test]
fn source_path_whitespace_and_posix_literal_backslashes_do_not_change_identity() {
    let parsed = workspace(
        serde_json::json!([{ "uri": "file:///source/repo" }, { "uri": "file:///source/repo%20" }]),
    );
    assert_eq!(parsed.roots.len(), 2);
    assert_ne!(parsed.roots[0].root_id, parsed.roots[1].root_id);
    let parsed = source_workspace_from_code_workspace_checked(
        r#"{"folders":[{"path":"child"}]}"#,
        r"/source/work\space/app.code-workspace",
    )
    .unwrap();
    assert_eq!(parsed.roots[0].original_path, r"/source/work\space/child");
    let parsed = source_workspace_from_code_workspace_checked(
        r#"{"folders":[{"path":"child"}]}"#,
        r"/source/work\space.code-workspace",
    )
    .unwrap();
    assert_eq!(parsed.roots[0].original_path, "/source/child");
}

#[test]
fn remote_root_needs_an_explicit_local_target_even_if_its_path_exists_here() {
    let home = IsolatedAppHome::acquire("workspace-uri-explicit");
    let path = home.path.to_string_lossy().into_owned();
    let mut remote = classify_root_with_host(&path, HostKind::Ssh);
    remote.host_identity = Some("ssh-remote+other".into());
    let mut mappings = propose_mappings(&[Some(path.clone())], Some("project".into()));
    let id = mappings[0].root_id.clone();
    assert!(mappings[0].explicit_target.is_none());
    assert!(execution_directory(&mappings[0], &remote).is_err());
    apply_user_choice(
        &mut mappings,
        &id,
        MappingDecision::ExplicitPath { path: path.clone() },
    );
    assert!(mappings[0].target_project_id.is_none());
    assert_eq!(execution_directory(&mappings[0], &remote).unwrap(), path);
    mappings[0].explicit_target = Some("relative".into());
    assert!(execution_directory(&mappings[0], &remote).is_err());
    let unknown = uri::parse("custom://other/repo").unwrap();
    mappings[0].explicit_target = None;
    assert!(execution_directory(&mappings[0], &unknown).is_err());
}

#[test]
fn creating_a_project_confirms_the_suggestion_and_switching_project_clears_it() {
    let mut mappings = propose_mappings(&[Some("/source/repo".into())], None);
    let id = mappings[0].root_id.clone();
    assert!(mappings[0].explicit_target.is_none());
    apply_user_choice(&mut mappings, &id, MappingDecision::NewRecord);
    assert!(mappings[0].create_project);
    assert_eq!(mappings[0].explicit_target.as_deref(), Some("/source/repo"));
    apply_user_choice(
        &mut mappings,
        &id,
        MappingDecision::ExistingProject {
            project_id: "other".into(),
        },
    );
    assert!(mappings[0].explicit_target.is_none());
    assert!(!mappings[0].create_project);
    assert_eq!(
        mappings[0].suggested_target.as_deref(),
        Some("/source/repo")
    );
}

#[test]
fn mixed_uri_workspace_roundtrips_offline_without_sessions_or_native_binding() {
    let home = IsolatedAppHome::acquire("workspace-uri-roundtrip");
    let file = home.path.join("mixed.code-workspace");
    let text = r#"{"folders":[{"path":"missing"},{"uri":"vscode-remote://ssh-remote+box/home/repo"},{"uri":"custom://other/repo"}]}"#;
    std::fs::write(&file, text).unwrap();
    let preview = import::preview(import::WorkspaceSource {
        source_id: None,
        path: file.to_string_lossy().into_owned(),
        kind: import::WorkspaceSourceKind::File,
    })
    .unwrap();
    let choices = preview
        .workspace
        .roots
        .iter()
        .enumerate()
        .map(|(i, root)| WorkspaceRootChoice {
            root_id: root.root_id.clone(),
            target_project_id: None,
            explicit_target: None,
            primary: i == 0,
        })
        .collect();
    let saved = import::save(import::WorkspaceSaveRequest {
        source: preview.source,
        expected_fingerprint: preview.fingerprint,
        expected_saved_revision: None,
        choices,
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
    std::fs::remove_file(&file).unwrap();
    let mut reopened =
        crate::session_migration::workspace_edit::get(&saved.source_workspace_id).unwrap();
    assert_eq!(reopened.workspace, saved);
    reopened.workspace.mapping_choices[2].explicit_target =
        Some(home.path.to_string_lossy().into_owned());
    let edited = crate::session_migration::workspace_edit::update(
        crate::session_migration::workspace_edit::WorkspaceUpdateRequest {
            source_id: None,
            source_workspace_id: saved.source_workspace_id,
            expected_revision: reopened.revision,
            choices: reopened.workspace.mapping_choices,
        },
    )
    .unwrap();
    assert_eq!(edited.workspace.roots, saved.roots);
    assert!(!crate::paths::projects_file().exists());
    assert!(!crate::paths::workspaces_file().exists());
    let conn = crate::session_migration::catalog::open_catalog_readonly().unwrap();
    assert!(crate::session_migration::catalog::list_entries(&conn)
        .unwrap()
        .is_empty());
}
