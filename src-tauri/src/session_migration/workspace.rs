//! Source workspace roots and mapping. Paths keep the source platform syntax.

use serde::{Deserialize, Serialize};

use crate::session_migration::hash::sha256_bytes;

#[path = "workspace_paths.rs"]
mod source_paths;

#[path = "workspace_jsonc.rs"]
mod jsonc;

#[path = "workspace_folders.rs"]
mod folders;

#[path = "workspace_uri.rs"]
mod uri;

#[cfg(test)]
#[path = "workspace_uri_tests.rs"]
mod uri_tests;

#[cfg(test)]
#[path = "workspace_path_tests.rs"]
mod path_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathSyntax {
    Windows,
    Posix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HostKind {
    Local,
    Wsl,
    Ssh,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootAvailability {
    Exists,
    Missing,
    Unresolved,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRoot {
    pub root_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub path_syntax: PathSyntax,
    pub host: HostKind,
    pub original_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_uri: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_identity: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsupported_uri: bool,
    pub availability: RootAvailability,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceWorkspace {
    pub source_workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<super::workspace_identity::WorkspaceSourceIdentity>,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_file: Option<String>,
    pub roots: Vec<WorkspaceRoot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_root_id: Option<String>,
    pub evidence: Vec<String>,
    /// Source metadata only; these choices do not grant execution permissions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapping_choices: Vec<WorkspaceRootChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRootChoice {
    pub root_id: String,
    pub target_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explicit_target: Option<String>,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMapping {
    pub root_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<super::mapping_scope::MappingSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit_target: Option<String>,
    pub unassigned: bool,
    pub user_choice_version: u32,
    #[serde(default)]
    pub primary: bool,
    /// Cwd suggestion kept after the user picks a different target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_target: Option<String>,
    /// Create a new Grok project record later. Never mutates an existing one.
    #[serde(default)]
    pub create_project: bool,
    /// If association fails, import history unassigned instead of leaving the item pending.
    #[serde(default)]
    pub import_unassigned_anyway: bool,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingDecision {
    ExistingProject { project_id: String },
    NewRecord,
    ExplicitPath { path: String },
    Unassigned,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssociationResolution {
    Existing { project_id: String },
    CreateProject,
    ExplicitPath { path: String },
    Unassigned { reported: bool },
}

/// Stable id for a source cwd/root. Tests assert distinct paths map to distinct ids,
/// not a particular hash string.
pub fn root_id_for_path(path: &str) -> String {
    root_id_for(infer_host(path), path)
}

/// Host participates in identity so the same path text on two SSH hosts stays distinct.
pub fn root_id_for(host: HostKind, path: &str) -> String {
    let host = match host {
        HostKind::Local => "local",
        HostKind::Wsl => "wsl",
        HostKind::Ssh => "ssh",
        HostKind::Unknown => "unknown",
    };
    format!(
        "root:{}",
        sha256_bytes(format!("{host}\0{path}").as_bytes())
    )
}

/// Classify a source path without rewriting it to the current OS.
pub fn classify_root(original_path: &str) -> WorkspaceRoot {
    classify_root_with_host(original_path, infer_host(original_path))
}

pub fn classify_root_with_host(original_path: &str, host: HostKind) -> WorkspaceRoot {
    let original_path = original_path.to_string();
    let path_syntax = infer_syntax(&original_path);
    let availability = probe_availability(&original_path, path_syntax, host);
    let evidence = vec![
        format!("syntax:{path_syntax:?}"),
        format!("host:{host:?}"),
        format!("availability:{availability:?}"),
    ];
    let host_identity = source_paths::unc_parts(&original_path).map(|(server, share)| {
        if host == HostKind::Wsl {
            format!("wsl+{share}")
        } else {
            format!("unc:{server}")
        }
    });
    WorkspaceRoot {
        root_id: root_id_for(host, &original_path),
        display_name: None,
        path_syntax,
        host,
        original_path,
        source_uri: None,
        host_identity,
        unsupported_uri: false,
        availability,
        evidence,
    }
}

fn probe_availability(path: &str, syntax: PathSyntax, host: HostKind) -> RootAvailability {
    // Reading workspace metadata must never initiate a UNC/WSL network probe.
    if source_paths::is_network_path(path) {
        return RootAvailability::Offline;
    }
    match host {
        HostKind::Ssh | HostKind::Wsl => RootAvailability::Offline,
        HostKind::Unknown => RootAvailability::Unresolved,
        HostKind::Local => {
            if !can_probe_natively(syntax) {
                return RootAvailability::Unresolved;
            }
            if std::path::Path::new(path).is_dir() {
                RootAvailability::Exists
            } else {
                RootAvailability::Missing
            }
        }
    }
}

fn can_probe_natively(syntax: PathSyntax) -> bool {
    match syntax {
        PathSyntax::Windows => cfg!(windows),
        PathSyntax::Posix => cfg!(unix),
    }
}

/// A folder is a workspace even when it contains no imported sessions.
pub fn source_workspace_from_folder(original_path: &str) -> SourceWorkspace {
    let root = classify_root(original_path);
    let display_name = source_paths::file_name(original_path, root.path_syntax)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| original_path.to_string());
    SourceWorkspace {
        source_workspace_id: format!("ws:{}", sha256_bytes(original_path.as_bytes())),
        source_identity: None,
        display_name,
        workspace_file: None,
        default_root_id: Some(root.root_id.clone()),
        evidence: vec![format!("folder:{original_path}")],
        roots: vec![root],
        mapping_choices: Vec::new(),
        source_fingerprint: None,
    }
}

/// Build a SourceWorkspace from a `.code-workspace` file. Missing folders stay
/// in `roots` with Missing/Unresolved availability.
#[cfg(test)]
pub fn source_workspace_from_code_workspace(text: &str, workspace_file: &str) -> SourceWorkspace {
    source_workspace_from_code_workspace_checked(text, workspace_file)
        .unwrap_or_else(|_| workspace_from_roots(workspace_file, Vec::new()))
}

fn workspace_from_roots(file: &str, roots: Vec<WorkspaceRoot>) -> SourceWorkspace {
    let display_name = source_paths::file_name(file, infer_syntax(file))
        .map(source_paths::file_stem)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "workspace".into());
    let default_root_id = roots.first().map(|r| r.root_id.clone());
    SourceWorkspace {
        source_workspace_id: format!("ws:{}", sha256_bytes(file.as_bytes())),
        source_identity: None,
        display_name,
        workspace_file: Some(file.to_string()),
        default_root_id,
        evidence: vec![format!("code-workspace:{file}")],
        roots,
        mapping_choices: Vec::new(),
        source_fingerprint: None,
    }
}

/// Strict production parser. Empty workspaces stay empty and malformed input
/// cannot silently become the parent directory.
pub fn source_workspace_from_code_workspace_checked(
    text: &str,
    workspace_file: &str,
) -> Result<SourceWorkspace, String> {
    let file = workspace_file;
    let separators: &[char] = if infer_syntax(file) == PathSyntax::Windows {
        &['/', '\\']
    } else {
        &['/']
    };
    let dir = match file.rsplit_once(separators) {
        Some((d, _)) => d,
        None => file,
    };
    let roots = folders::deduplicate(folders::parse(text, dir)?);
    Ok(workspace_from_roots(file, roots))
}

/// One mapping per distinct cwd. Empty cwd list yields a single unassigned row.
#[cfg(test)]
pub fn propose_mappings(
    cwds: &[Option<String>],
    fallback_project: Option<String>,
) -> Vec<WorkspaceMapping> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for cwd in cwds {
        let Some(path) = cwd.as_deref().filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        if !seen.insert(path.to_string()) {
            continue;
        }
        let assigned = fallback_project.is_some();
        out.push(WorkspaceMapping {
            source: None,
            root_id: root_id_for_path(path),
            target_project_id: fallback_project.clone(),
            explicit_target: None,
            unassigned: !assigned,
            user_choice_version: 1,
            primary: out.is_empty(),
            suggested_target: Some(path.to_string()),
            create_project: false,
            import_unassigned_anyway: false,
        });
    }
    if out.is_empty() {
        out.push(WorkspaceMapping {
            source: None,
            root_id: "unassigned".into(),
            target_project_id: fallback_project,
            explicit_target: None,
            unassigned: true,
            user_choice_version: 1,
            primary: true,
            suggested_target: None,
            create_project: false,
            import_unassigned_anyway: false,
        });
    } else if !out.iter().any(|m| m.primary) {
        out[0].primary = true;
    }
    out
}

/// User choice replaces the mapping for one root. The original cwd suggestion stays.
#[cfg(test)]
pub fn apply_user_choice(
    mappings: &mut [WorkspaceMapping],
    root_id: &str,
    decision: MappingDecision,
) {
    let Some(row) = mappings.iter_mut().find(|m| m.root_id == root_id) else {
        return;
    };
    row.user_choice_version = row.user_choice_version.saturating_add(1);
    match decision {
        MappingDecision::ExistingProject { project_id } => {
            row.target_project_id = Some(project_id);
            row.explicit_target = None;
            row.unassigned = false;
            row.create_project = false;
        }
        MappingDecision::NewRecord => {
            row.target_project_id = None;
            // Choosing a new record confirms the visible target directory.
            if row.explicit_target.is_none() {
                row.explicit_target = row.suggested_target.clone();
            }
            row.unassigned = false;
            row.create_project = true;
        }
        MappingDecision::ExplicitPath { path } => {
            row.explicit_target = Some(path);
            row.target_project_id = None;
            row.unassigned = false;
            row.create_project = false;
        }
        MappingDecision::Unassigned => {
            row.target_project_id = None;
            row.explicit_target = None;
            row.unassigned = true;
            row.create_project = false;
        }
    }
}

#[cfg(test)]
pub fn set_primary_root(mappings: &mut [WorkspaceMapping], root_id: &str) {
    for row in mappings.iter_mut() {
        row.primary = row.root_id == root_id;
    }
    if !mappings.iter().any(|m| m.primary) {
        if let Some(first) = mappings.first_mut() {
            first.primary = true;
        }
    }
}

/// Reading imported history does not require the source worktree.
#[cfg(test)]
pub fn reading_allowed(_root: &WorkspaceRoot) -> bool {
    true
}

/// Tool execution needs a real directory. Missing worktrees are not created.
#[cfg(test)]
pub fn execution_directory(
    mapping: &WorkspaceMapping,
    root: &WorkspaceRoot,
) -> Result<String, crate::session_migration::errors::ImportError> {
    use crate::session_migration::errors::{ErrorCode, ImportError};
    let explicit = mapping.explicit_target.as_deref().filter(|s| !s.is_empty());
    let path = explicit.unwrap_or(root.original_path.as_str());
    if mapping.unassigned {
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "root is unassigned; choose a project directory before running tools",
        ));
    }
    if explicit.is_none()
        && (root.host != HostKind::Local
            || root.unsupported_uri
            || root.availability == RootAvailability::Offline
            || !can_probe_natively(root.path_syntax))
    {
        return Err(ImportError::new(
            ErrorCode::WorkspaceUnresolved,
            "remote root is not a local worktree",
        ));
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(ImportError::new(
            ErrorCode::WorkspaceUnresolved,
            "choose an absolute local target directory",
        ));
    }
    if !std::path::Path::new(path).is_dir() {
        return Err(ImportError::new(
            ErrorCode::SourceNotFound,
            "mapped directory is missing; import history is still readable",
        ));
    }
    Ok(path.to_string())
}

/// Decide how a root associates with a Grok project. Missing projects stay
/// pending unless the user already allowed unassigned import.
#[cfg(test)]
pub fn resolve_association(
    mapping: &WorkspaceMapping,
    project_exists: impl Fn(&str) -> bool,
) -> Result<AssociationResolution, crate::session_migration::errors::ImportError> {
    use crate::session_migration::errors::{ErrorCode, ImportError};
    if mapping.unassigned {
        return Ok(AssociationResolution::Unassigned { reported: false });
    }
    if mapping.create_project {
        return Ok(AssociationResolution::CreateProject);
    }
    if let Some(id) = mapping
        .target_project_id
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        if project_exists(id) {
            return Ok(AssociationResolution::Existing {
                project_id: id.to_string(),
            });
        }
        if mapping.import_unassigned_anyway {
            return Ok(AssociationResolution::Unassigned { reported: true });
        }
        return Err(ImportError::new(
            ErrorCode::MappingConflict,
            "mapped project is missing; choose another project or allow unassigned import",
        ));
    }
    if let Some(path) = mapping.explicit_target.as_deref().filter(|s| !s.is_empty()) {
        return Ok(AssociationResolution::ExplicitPath {
            path: path.to_string(),
        });
    }
    if mapping.import_unassigned_anyway {
        return Ok(AssociationResolution::Unassigned { reported: true });
    }
    Err(ImportError::new(
        ErrorCode::MappingConflict,
        "root has no project mapping",
    ))
}

pub fn mapping_for_cwd<'a>(
    mappings: &'a [WorkspaceMapping],
    cwd: Option<&str>,
) -> Option<&'a WorkspaceMapping> {
    if let Some(cwd) = cwd.filter(|s| !s.trim().is_empty()) {
        let rid = root_id_for_path(cwd);
        if let Some(m) = mappings.iter().find(|m| m.root_id == rid) {
            return Some(m);
        }
        if let Some(m) = mappings
            .iter()
            .find(|m| m.explicit_target.as_deref() == Some(cwd))
        {
            return Some(m);
        }
    }
    mappings.iter().find(|m| m.root_id == "unassigned")
}

/// Match a session cwd to the mapping the user confirmed. Distinct roots do not
/// inherit the first mapping's project.
#[cfg(test)]
pub fn mapping_project_id(mappings: &[WorkspaceMapping], cwd: Option<&str>) -> Option<String> {
    if let Some(cwd) = cwd.filter(|s| !s.trim().is_empty()) {
        let rid = root_id_for_path(cwd);
        if let Some(m) = mappings.iter().find(|m| m.root_id == rid) {
            return m.target_project_id.clone();
        }
        if let Some(m) = mappings
            .iter()
            .find(|m| m.explicit_target.as_deref() == Some(cwd))
        {
            return m.target_project_id.clone();
        }
    }
    mappings
        .iter()
        .find(|m| m.root_id == "unassigned")
        .and_then(|m| m.target_project_id.clone())
}

pub fn infer_syntax(path: &str) -> PathSyntax {
    let t = path.trim();
    // An absolute POSIX name may contain a literal backslash.
    if t.starts_with('/') {
        return PathSyntax::Posix;
    }
    if t.len() >= 2 && t.as_bytes()[0].is_ascii_alphabetic() && t.as_bytes()[1] == b':' {
        return PathSyntax::Windows;
    }
    if t.starts_with("\\\\") || t.contains('\\') {
        return PathSyntax::Windows;
    }
    PathSyntax::Posix
}

pub fn infer_host(path: &str) -> HostKind {
    let t = path.trim();
    if t.starts_with("ssh://") || t.starts_with("ssh:") {
        return HostKind::Ssh;
    }
    // user@host:path — not a Windows drive letter (second byte ':').
    if t.contains('@') && t.contains(':') && !(t.len() >= 2 && t.as_bytes()[1] == b':') {
        return HostKind::Ssh;
    }
    if source_paths::unc_parts(t).is_some_and(|(server, _)| {
        server.eq_ignore_ascii_case("wsl$") || server.eq_ignore_ascii_case("wsl.localhost")
    }) {
        return HostKind::Wsl;
    }
    if t.starts_with(r"\\.\") {
        return HostKind::Unknown;
    }
    // A /mnt path is valid on ordinary Linux hosts too. Only explicit source
    // evidence (a WSL URI/UNC authority) establishes a distro identity.
    HostKind::Local
}

/// Parse a `.code-workspace` JSONC `folders` list. Relative roots stay relative
/// to the workspace file directory; missing files become unresolved.
#[cfg(test)]
pub fn parse_code_workspace(text: &str, workspace_dir: &str) -> Vec<(String, String)> {
    parse_code_workspace_checked(text, workspace_dir).unwrap_or_default()
}

/// Parse a JSONC workspace while retaining malformed-input errors for the
/// explicit import path. Relative roots stay relative to the workspace file.
#[cfg(test)]
pub fn parse_code_workspace_checked(
    text: &str,
    workspace_dir: &str,
) -> Result<Vec<(String, String)>, String> {
    Ok(folders::parse(text, workspace_dir)?
        .into_iter()
        .map(|root| (root.display_name.unwrap_or_default(), root.original_path))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_keep_windows_syntax_on_any_host() {
        assert_eq!(infer_syntax(r"C:\中文 目录\repo"), PathSyntax::Windows);
        assert_eq!(infer_syntax("/Users/me/repo"), PathSyntax::Posix);
    }

    #[test]
    fn code_workspace_keeps_missing_relative_root() {
        let text = r#"{
            // comment
            "folders": [
                { "path": "a" },
                { "path": "missing-root", "name": "gone" }
            ]
        }"#;
        let folders = parse_code_workspace(text, r"C:\ws");
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[1].0, "gone");
        assert!(folders[1].1.contains("missing-root"));
    }

    #[test]
    fn distinct_cwds_get_distinct_mappings_and_projects() {
        let rows = propose_mappings(
            &[
                Some(r"C:\repo-a".into()),
                Some(r"C:\repo-b".into()),
                Some(r"C:\repo-a".into()),
            ],
            None,
        );
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].root_id, rows[1].root_id);
        let a = rows
            .iter()
            .find(|m| m.suggested_target.as_deref() == Some(r"C:\repo-a"))
            .unwrap();
        let b = rows
            .iter()
            .find(|m| m.suggested_target.as_deref() == Some(r"C:\repo-b"))
            .unwrap();
        let mut mapped = vec![a.clone(), b.clone()];
        mapped[0].target_project_id = Some("proj-a".into());
        mapped[0].unassigned = false;
        mapped[1].target_project_id = Some("proj-b".into());
        mapped[1].unassigned = false;
        assert_eq!(
            mapping_project_id(&mapped, Some(r"C:\repo-a")).as_deref(),
            Some("proj-a")
        );
        assert_eq!(
            mapping_project_id(&mapped, Some(r"C:\repo-b")).as_deref(),
            Some("proj-b")
        );
        assert_eq!(mapping_project_id(&mapped, None), None);
    }

    #[test]
    fn user_mapping_wins_over_unassigned_fallback() {
        let cwd = r"C:\picked";
        let maps = vec![
            WorkspaceMapping {
                source: None,
                root_id: root_id_for_path(cwd),
                target_project_id: Some("user-picked".into()),
                explicit_target: Some(cwd.into()),
                unassigned: false,
                user_choice_version: 2,
                primary: true,
                suggested_target: Some(r"C:\suggested".into()),
                create_project: false,
                import_unassigned_anyway: false,
            },
            WorkspaceMapping {
                source: None,
                root_id: "unassigned".into(),
                target_project_id: Some("fallback".into()),
                explicit_target: None,
                unassigned: true,
                user_choice_version: 1,
                primary: false,
                suggested_target: None,
                create_project: false,
                import_unassigned_anyway: false,
            },
        ];
        assert_eq!(
            mapping_project_id(&maps, Some(cwd)).as_deref(),
            Some("user-picked")
        );
    }

    #[test]
    fn empty_folder_is_a_workspace_without_sessions() {
        let dir = std::env::temp_dir().join(format!("ws-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let ws = source_workspace_from_folder(&dir.to_string_lossy());
        assert_eq!(ws.roots.len(), 1);
        assert_eq!(ws.roots[0].availability, RootAvailability::Exists);
        assert_eq!(
            ws.default_root_id.as_deref(),
            Some(ws.roots[0].root_id.as_str())
        );
        assert_eq!(ws.roots[0].original_path, dir.to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn code_workspace_keeps_two_roots_and_marks_missing() {
        let dir = std::env::temp_dir().join(format!("中文 目录-{}", uuid::Uuid::new_v4()));
        let present = dir.join("a");
        std::fs::create_dir_all(&present).unwrap();
        let file = dir.join("app.code-workspace");
        let text = r#"{
            "folders": [
                { "path": "a", "name": "alpha" },
                { "path": "missing-root", "name": "gone" }
            ]
        }"#;
        std::fs::write(&file, text).unwrap();
        let ws = source_workspace_from_code_workspace(text, &file.to_string_lossy());
        assert_eq!(ws.roots.len(), 2);
        assert_eq!(ws.display_name, "app");
        assert_eq!(
            ws.default_root_id.as_deref(),
            Some(ws.roots[0].root_id.as_str())
        );
        assert_eq!(ws.roots[0].availability, RootAvailability::Exists);
        assert_eq!(ws.roots[1].availability, RootAvailability::Missing);
        assert!(ws.roots[1].original_path.contains("missing-root"));
        assert_eq!(
            ws.roots[0].path_syntax,
            if cfg!(windows) {
                PathSyntax::Windows
            } else {
                PathSyntax::Posix
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checked_code_workspace_rejects_malformed_jsonc() {
        let error = source_workspace_from_code_workspace_checked(
            r#"{"folders":[{"path":"unterminated}"#,
            r"C:\ws\app.code-workspace",
        )
        .unwrap_err();
        assert!(error.starts_with("invalid workspace JSONC:"), "{error}");

        let error =
            parse_code_workspace_checked(r#"{"folders":{"path":"not-an-array"}}"#, r"C:\ws")
                .unwrap_err();
        assert_eq!(error, "workspace folders must be an array");
    }

    #[test]
    fn windows_chinese_path_is_not_rewritten() {
        let path = r"C:\中文 目录\repo";
        let root = classify_root(path);
        assert_eq!(root.original_path, path);
        assert_eq!(root.path_syntax, PathSyntax::Windows);
        assert_eq!(root.host, HostKind::Local);
    }

    #[test]
    fn same_path_text_different_ssh_hosts_are_distinct_roots() {
        let path = "/home/me/app";
        let a = classify_root_with_host(path, HostKind::Ssh);
        let b = classify_root_with_host(path, HostKind::Local);
        assert_ne!(a.root_id, b.root_id);
        assert_eq!(a.original_path, path);
        assert_eq!(b.original_path, path);
        assert_eq!(a.host, HostKind::Ssh);
        assert_eq!(a.availability, RootAvailability::Offline);
        let remote = classify_root("user@box.example:/home/me/app");
        assert_eq!(remote.host, HostKind::Ssh);
        assert_eq!(remote.original_path, "user@box.example:/home/me/app");
        assert_eq!(remote.availability, RootAvailability::Offline);
    }

    #[test]
    fn posix_path_is_unresolved_on_windows_and_not_rewritten() {
        let path = "/Users/me/Documents/repo";
        let root = classify_root(path);
        assert_eq!(root.original_path, path);
        assert_eq!(root.path_syntax, PathSyntax::Posix);
        if cfg!(windows) {
            assert_eq!(root.availability, RootAvailability::Unresolved);
            assert!(!root.original_path.contains('\\'));
        }
    }

    #[test]
    fn catalog_persists_workspace_without_sessions() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let home = std::env::temp_dir().join(format!("ws-cat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("GROK_APP_HOME", &home);
        let _ = crate::paths::ensure_app_dirs();
        let folder = home.join("project-a");
        std::fs::create_dir_all(&folder).unwrap();
        let ws = source_workspace_from_folder(&folder.to_string_lossy());
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        crate::session_migration::catalog::upsert_workspace(&conn, &ws).unwrap();
        let listed = crate::session_migration::catalog::list_workspaces(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].source_workspace_id, ws.source_workspace_id);
        assert_eq!(listed[0].roots.len(), 1);
        assert!(crate::session_migration::catalog::list_entries(&conn)
            .unwrap()
            .is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    fn choose(maps: &mut [WorkspaceMapping], i: usize, decision: MappingDecision) {
        let id = maps[i].root_id.clone();
        apply_user_choice(maps, &id, decision);
    }

    #[test]
    fn user_choice_beats_cwd_suggestion_and_keeps_evidence() {
        let mut maps = propose_mappings(&[Some(r"C:\src".into())], Some("suggested-proj".into()));
        assert_eq!(maps[0].suggested_target.as_deref(), Some(r"C:\src"));
        assert!(maps[0].primary);
        choose(
            &mut maps,
            0,
            MappingDecision::ExistingProject {
                project_id: "user-proj".into(),
            },
        );
        assert_eq!(maps[0].target_project_id.as_deref(), Some("user-proj"));
        assert_eq!(maps[0].suggested_target.as_deref(), Some(r"C:\src"));
        assert_eq!(maps[0].user_choice_version, 2);
        assert_eq!(
            mapping_project_id(&maps, Some(r"C:\src")).as_deref(),
            Some("user-proj")
        );
    }

    #[test]
    fn each_root_can_be_primary_independently() {
        let mut maps = propose_mappings(&[Some(r"C:\a".into()), Some(r"C:\b".into())], None);
        assert!(maps[0].primary);
        assert!(!maps[1].primary);
        let b = maps[1].root_id.clone();
        set_primary_root(&mut maps, &b);
        assert!(!maps[0].primary);
        assert!(maps[1].primary);
        choose(&mut maps, 0, MappingDecision::Unassigned);
        choose(
            &mut maps,
            1,
            MappingDecision::ExplicitPath {
                path: r"D:\explicit".into(),
            },
        );
        assert!(maps[0].unassigned);
        assert_eq!(maps[1].explicit_target.as_deref(), Some(r"D:\explicit"));
        assert!(maps[1].primary);
    }

    #[test]
    fn mapping_does_not_move_or_unpin_existing_projects() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let home = std::env::temp_dir().join(format!("ws-prot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("GROK_APP_HOME", &home);
        let _ = crate::paths::ensure_app_dirs();
        let proj_dir = home.join("trusted-proj");
        std::fs::create_dir_all(&proj_dir).unwrap();
        let project = crate::store::add_project(proj_dir.to_string_lossy().into(), true).unwrap();
        crate::store::set_project_pinned(&project.id, true).unwrap();
        let before = crate::store::load_projects();
        let kept = before.iter().find(|p| p.id == project.id).unwrap().clone();
        let order: Vec<_> = before.iter().map(|p| p.id.clone()).collect();
        let mut maps = propose_mappings(&[Some(r"C:\imported-src".into())], None);
        choose(
            &mut maps,
            0,
            MappingDecision::ExistingProject {
                project_id: project.id.clone(),
            },
        );
        choose(&mut maps, 0, MappingDecision::NewRecord);
        let after = crate::store::load_projects();
        let still = after.iter().find(|p| p.id == project.id).unwrap();
        assert_eq!(still.name, kept.name);
        assert_eq!(still.path, kept.path);
        assert!(still.pinned && still.trusted);
        let after_order: Vec<_> = after.iter().map(|p| p.id.clone()).collect();
        assert_eq!(after_order, order);
        assert_eq!(after.len(), before.len());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn missing_worktree_still_readable_but_blocks_execution() {
        let missing = r"C:\definitely-missing-worktree-s19";
        let root = classify_root(missing);
        assert!(reading_allowed(&root));
        let mut maps = propose_mappings(&[Some(missing.into())], Some("p".into()));
        maps[0].unassigned = false;
        let err = execution_directory(&maps[0], &root).unwrap_err();
        assert_eq!(
            err.code,
            crate::session_migration::errors::ErrorCode::SourceNotFound
        );
        maps[0].unassigned = true;
        let err = execution_directory(&maps[0], &root).unwrap_err();
        assert_eq!(
            err.code,
            crate::session_migration::errors::ErrorCode::MappingConflict
        );
    }

    #[test]
    fn duplicate_plan_mappings_reuse_root_ids() {
        let a = propose_mappings(&[Some(r"C:\repo".into())], None);
        let b = propose_mappings(&[Some(r"C:\repo".into())], None);
        assert_eq!(a[0].root_id, b[0].root_id);
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn association_failure_stays_pending_unless_user_allows_unassigned() {
        let mut maps = propose_mappings(&[Some(r"C:\src".into())], None);
        choose(
            &mut maps,
            0,
            MappingDecision::ExistingProject {
                project_id: "gone".into(),
            },
        );
        let err = resolve_association(&maps[0], |_| false).unwrap_err();
        assert_eq!(
            err.code,
            crate::session_migration::errors::ErrorCode::MappingConflict
        );
        maps[0].import_unassigned_anyway = true;
        assert_eq!(
            resolve_association(&maps[0], |_| false).unwrap(),
            AssociationResolution::Unassigned { reported: true }
        );
        choose(&mut maps, 0, MappingDecision::Unassigned);
        assert_eq!(
            resolve_association(&maps[0], |_| false).unwrap(),
            AssociationResolution::Unassigned { reported: false }
        );
        choose(&mut maps, 0, MappingDecision::NewRecord);
        assert_eq!(
            resolve_association(&maps[0], |_| false).unwrap(),
            AssociationResolution::CreateProject
        );
    }

    #[test]
    fn missing_worktree_does_not_checkout_or_create_git() {
        let missing = std::env::temp_dir().join(format!("no-wt-s19-{}", uuid::Uuid::new_v4()));
        let path = missing.to_string_lossy().into_owned();
        assert!(!missing.exists());
        let root = classify_root(&path);
        let mut maps = propose_mappings(&[Some(path.clone())], Some("p".into()));
        maps[0].unassigned = false;
        let _ = execution_directory(&maps[0], &root);
        assert!(!missing.exists());
        assert!(!missing.join(".git").exists());
        assert!(!missing.join("HEAD").exists());
    }

    #[test]
    fn catalog_persists_user_mapping_choice() {
        let _lock = crate::session_migration::test_home::AppHomeEnvGuard::acquire();
        let home = std::env::temp_dir().join(format!("ws-map-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("GROK_APP_HOME", &home);
        let _ = crate::paths::ensure_app_dirs();
        let mut maps = propose_mappings(&[Some(r"C:\src".into())], Some("suggested-proj".into()));
        crate::store::ensure_import_project("user-proj", home.to_str().unwrap()).unwrap();
        choose(
            &mut maps,
            0,
            MappingDecision::ExistingProject {
                project_id: "user-proj".into(),
            },
        );
        let conn = crate::session_migration::catalog::open_catalog().unwrap();
        crate::session_migration::catalog::upsert_mapping(&conn, &maps[0]).unwrap();
        let listed = crate::session_migration::catalog::list_mappings(&conn).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].target_project_id.as_deref(), Some("user-proj"));
        assert_eq!(listed[0].suggested_target.as_deref(), Some(r"C:\src"));
        assert_eq!(listed[0].user_choice_version, 2);
        let _ = std::fs::remove_dir_all(&home);
    }
    crate::session_migration::test_home::fixture_home_contract_tests!(
        catalog_persists_workspace_without_sessions,
        mapping_does_not_move_or_unpin_existing_projects,
        catalog_persists_user_mapping_choice,
    );
}
