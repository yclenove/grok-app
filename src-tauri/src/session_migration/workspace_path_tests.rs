use super::*;

#[test]
fn absolute_roots_keep_their_source_syntax_on_every_host() {
    for base in [r"C:\配置", "/home/config"] {
        for path in [
            r"D:\中文 目录\repo",
            "E:/work/repo",
            r"\\server\share\repo",
            "/home/中文 目录/repo",
        ] {
            let text = serde_json::json!({ "folders": [{ "path": path }] }).to_string();
            let folders = parse_code_workspace(&text, base);
            assert_eq!(folders, vec![("repo".into(), path.into())], "base={base}");
        }
    }
}

#[test]
fn foreign_workspace_and_folder_names_are_not_full_paths() {
    for folder in [
        r"C:\中文 目录\repo",
        "C:/中文 目录/repo",
        "/home/中文 目录/repo",
    ] {
        let workspace = source_workspace_from_folder(folder);
        assert_eq!(workspace.display_name, "repo");
        assert_eq!(workspace.roots[0].original_path, folder);
    }
    for file in [
        r"C:\中文 目录\app.code-workspace",
        "C:/中文 目录/app.code-workspace",
        "/home/中文 目录/app.code-workspace",
    ] {
        let workspace =
            source_workspace_from_code_workspace(r#"{"folders":[{"path":"child"}]}"#, file);
        assert_eq!(workspace.display_name, "app");
    }
}

#[test]
fn relative_roots_use_source_separator_and_keep_explicit_names() {
    let text = r#"{"folders":[{"path":"子目录","name":"chosen name"}]}"#;
    for (base, expected) in [
        (r"C:\配置", r"C:\配置\子目录"),
        ("/home/config", "/home/config/子目录"),
    ] {
        assert_eq!(
            parse_code_workspace(text, base),
            vec![("chosen name".into(), expected.into())]
        );
    }
}

#[test]
fn foreign_absolute_roots_remain_unresolved_until_mapping() {
    let path = if cfg!(windows) {
        "/not-a-local-volume/source/repo"
    } else {
        r"Z:\not-a-local-volume\source\repo"
    };
    let text = serde_json::json!({ "folders": [{ "path": path }] }).to_string();
    let workspace = source_workspace_from_code_workspace(&text, "/exports/app.code-workspace");
    let root = &workspace.roots[0];
    assert_eq!(root.original_path, path);
    assert_eq!(root.availability, RootAvailability::Unresolved);
    assert!(reading_allowed(root));
}

#[test]
fn posix_absolute_names_keep_literal_backslashes() {
    let path = r"/home/source/repo\name";
    let workspace = source_workspace_from_folder(path);
    assert_eq!(workspace.display_name, r"repo\name");
    assert_eq!(workspace.roots[0].original_path, path);
    assert_eq!(workspace.roots[0].path_syntax, PathSyntax::Posix);
    let workspace = source_workspace_from_code_workspace(
        r#"{"folders":[{"path":"child"}]}"#,
        r"/home/source/repo\name.code-workspace",
    );
    assert_eq!(workspace.display_name, r"repo\name");
}

#[test]
fn relative_names_use_the_workspace_syntax_not_the_host() {
    let text = serde_json::json!({ "folders": [{ "path": r"repo\name" }] }).to_string();
    assert_eq!(
        parse_code_workspace(&text, "/home/source"),
        vec![(r"repo\name".into(), r"/home/source/repo\name".into())]
    );
    assert_eq!(
        parse_code_workspace(&text, r"C:\source"),
        vec![("name".into(), r"C:\source\repo\name".into())]
    );
}
