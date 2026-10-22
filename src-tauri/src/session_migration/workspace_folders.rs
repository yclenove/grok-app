//! Strict `.code-workspace` folder schema, separate from JSONC normalization.

use super::{classify_root, infer_syntax, jsonc, source_paths, uri, PathSyntax, WorkspaceRoot};

pub(super) fn parse(text: &str, workspace_dir: &str) -> Result<Vec<WorkspaceRoot>, String> {
    let stripped = jsonc::normalize(text)?;
    let value = serde_json::from_str::<serde_json::Value>(&stripped)
        .map_err(|error| format!("invalid workspace JSONC: {error}"))?;
    let object = value.as_object().ok_or("workspace must be an object")?;
    let folders = match object.get("folders") {
        None => return Ok(Vec::new()),
        Some(value) => value
            .as_array()
            .ok_or("workspace folders must be an array")?,
    };
    if folders.len() > 256 {
        return Err("workspace exceeds the 256-root limit".into());
    }
    let mut roots = Vec::with_capacity(folders.len());
    for (i, folder) in folders.iter().enumerate() {
        let folder = folder
            .as_object()
            .ok_or_else(|| format!("workspace folder {i} must be an object"))?;
        let mut root = match (folder.get("path"), folder.get("uri")) {
            (Some(path), None) => path_root(
                path.as_str().ok_or("folder path must be a string")?,
                workspace_dir,
            )?,
            (None, Some(value)) => {
                uri::parse(value.as_str().ok_or("folder URI must be a string")?)?
            }
            _ => {
                return Err(format!(
                    "workspace folder {i} must have exactly one path or URI"
                ))
            }
        };
        let name = match folder.get("name") {
            Some(value) => value
                .as_str()
                .ok_or("folder name must be a string")?
                .to_string(),
            None => source_paths::file_name(&root.original_path, root.path_syntax)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("root-{i}")),
        };
        if name.len() > 1024 || name.contains('\0') {
            return Err(format!("workspace folder {i} has an invalid name"));
        }
        root.display_name = Some(name);
        roots.push(root);
    }
    Ok(roots)
}

fn path_root(path: &str, workspace_dir: &str) -> Result<WorkspaceRoot, String> {
    if path.is_empty() || path.len() > 8192 || path.contains('\0') {
        return Err("workspace folder has an invalid path".into());
    }
    let resolved = if source_paths::is_absolute(path) {
        path.to_string()
    } else {
        let syntax = infer_syntax(workspace_dir);
        let separators: &[char] = if syntax == PathSyntax::Windows {
            &['/', '\\']
        } else {
            &['/']
        };
        format!(
            "{}{}{}",
            workspace_dir.trim_end_matches(separators),
            if syntax == PathSyntax::Windows {
                "\\"
            } else {
                "/"
            },
            path
        )
    };
    if resolved.len() > 8192 {
        return Err("resolved workspace folder exceeds the import limit".into());
    }
    Ok(classify_root(&resolved))
}

pub(super) fn deduplicate(roots: Vec<WorkspaceRoot>) -> Vec<WorkspaceRoot> {
    let mut unique: Vec<WorkspaceRoot> = Vec::new();
    for root in roots {
        if let Some(previous) = unique.iter_mut().find(|row| row.root_id == root.root_id) {
            if previous.source_uri.is_none() {
                previous.source_uri = root.source_uri;
            }
            for evidence in root.evidence {
                if !previous.evidence.contains(&evidence) {
                    previous.evidence.push(evidence);
                }
            }
        } else {
            unique.push(root);
        }
    }
    unique
}
