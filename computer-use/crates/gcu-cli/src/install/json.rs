//! JSON config edits that keep existing key order.
//!
//! `serde_json` is built with `preserve_order`. A missing file and a
//! whitespace-only file are empty objects. Any other parse failure, or a root
//! that is not an object, is [`InstallError::Unparseable`] or
//! [`InstallError::UnexpectedShape`] and does not produce a [`FileChange`].
//! Uninstall leaves an empty parent object in place.

use std::path::Path;

use serde_json::{Map, Value};

use super::error::{unexpected, unparseable, InstallError};
use super::fsutil::read_bytes;
use super::plan::FileChange;
use super::SERVER_NAME;

/// How the `computer-use` object is spelled for one client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServerShape {
    /// `{ "command": "<gcu>", "args": ["mcp"] }`.
    CommandArgs,
    /// Command and args plus `"type": "stdio"`.
    Stdio,
    /// OpenCode local server: `"type": "local"`, `"command": ["<gcu>", "mcp"]`.
    LocalCommand,
}

enum Loaded {
    Missing,
    Blank(Vec<u8>),
    Object { bytes: Vec<u8>, value: Value },
}

/// Insert or refresh `computer-use` under `parent_key`. `Ok(None)` keeps the file bytes.
pub(crate) fn plan_upsert(
    path: &Path,
    parent_key: &str,
    shape: ServerShape,
    gcu: &str,
) -> Result<Option<FileChange>, InstallError> {
    edit_json(path, |map| upsert(map, path, parent_key, shape, gcu))
}

/// Remove only the `computer-use` key. An empty parent object is left in place.
pub(crate) fn plan_remove(
    path: &Path,
    parent_key: &str,
) -> Result<Option<FileChange>, InstallError> {
    edit_json(path, |map| remove(map, path, parent_key))
}

/// `Ok(None)` for a missing or blank file. Errors on a non-object or invalid JSON.
pub(crate) fn server_present(path: &Path, parent_key: &str) -> Result<bool, InstallError> {
    let Some(value) = load_object(path)? else {
        return Ok(false);
    };
    let Some(parent) = value.get(parent_key) else {
        return Ok(false);
    };
    let parent = parent
        .as_object()
        .ok_or_else(|| unexpected(path, format!("{parent_key} must be an object")))?;
    Ok(parent.contains_key(SERVER_NAME))
}

fn load_object(path: &Path) -> Result<Option<Value>, InstallError> {
    match load(path)? {
        Loaded::Missing | Loaded::Blank(_) => Ok(None),
        Loaded::Object { value, .. } => Ok(Some(value)),
    }
}

pub(crate) fn edit_json(
    path: &Path,
    mutate: impl FnOnce(&mut Map<String, Value>) -> Result<(), InstallError>,
) -> Result<Option<FileChange>, InstallError> {
    let loaded = load(path)?;
    let (before, mut value, blank) = match loaded {
        Loaded::Missing => (None, empty_object(), false),
        Loaded::Blank(bytes) => (Some(bytes), empty_object(), true),
        Loaded::Object { bytes, value } => (Some(bytes), value, false),
    };
    let original = value.clone();
    let map = value
        .as_object_mut()
        .ok_or_else(|| unexpected(path, "root must be an object"))?;
    mutate(map)?;
    if value == original {
        return Ok(None);
    }
    if blank && value == empty_object() {
        return Ok(None);
    }
    if before.is_none() && value == empty_object() {
        return Ok(None);
    }
    let bytes = to_pretty(&value)?;
    Ok(Some(FileChange::rewrite(path, before, bytes, None)))
}

fn load(path: &Path) -> Result<Loaded, InstallError> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(Loaded::Missing);
    };
    let text = std::str::from_utf8(&bytes).map_err(|source| unparseable(path, source))?;
    if text.trim().is_empty() {
        return Ok(Loaded::Blank(bytes));
    }
    let value: Value = serde_json::from_str(text).map_err(|source| unparseable(path, source))?;
    if !value.is_object() {
        return Err(unexpected(path, "root must be an object"));
    }
    Ok(Loaded::Object { bytes, value })
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn to_pretty(value: &Value) -> Result<Vec<u8>, InstallError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| InstallError::Write {
        path: Path::new("json").to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn upsert(
    root: &mut Map<String, Value>,
    path: &Path,
    parent_key: &str,
    shape: ServerShape,
    gcu: &str,
) -> Result<(), InstallError> {
    let parent = ensure_object(root, path, parent_key)?;
    if let Some(existing) = parent.get(SERVER_NAME) {
        if !existing.is_object() {
            return Err(unexpected(
                path,
                format!("{parent_key}.{SERVER_NAME} must be an object"),
            ));
        }
    } else {
        parent.insert(SERVER_NAME.to_owned(), Value::Object(Map::new()));
    }
    let Some(server) = parent.get_mut(SERVER_NAME).and_then(Value::as_object_mut) else {
        return Err(unexpected(
            path,
            format!("{parent_key}.{SERVER_NAME} must be an object"),
        ));
    };
    match shape {
        ServerShape::CommandArgs => {
            set_string(server, "command", gcu);
            set_string_array(server, "args", &[gcu_arg_mcp()]);
        }
        ServerShape::Stdio => {
            set_string(server, "type", "stdio");
            set_string(server, "command", gcu);
            set_string_array(server, "args", &[gcu_arg_mcp()]);
        }
        ServerShape::LocalCommand => {
            set_string(server, "type", "local");
            set_string_array(server, "command", &[gcu, "mcp"]);
            set_bool(server, "enabled", true);
        }
    }
    Ok(())
}

fn gcu_arg_mcp() -> &'static str {
    "mcp"
}

fn remove(
    root: &mut Map<String, Value>,
    path: &Path,
    parent_key: &str,
) -> Result<(), InstallError> {
    let Some(parent_value) = root.get(parent_key) else {
        return Ok(());
    };
    if !parent_value.is_object() {
        return Err(unexpected(path, format!("{parent_key} must be an object")));
    }
    if !parent_value
        .as_object()
        .is_some_and(|parent| parent.contains_key(SERVER_NAME))
    {
        return Ok(());
    }
    if let Some(parent) = root.get_mut(parent_key).and_then(Value::as_object_mut) {
        parent.remove(SERVER_NAME);
    }
    Ok(())
}

fn ensure_object<'a>(
    root: &'a mut Map<String, Value>,
    path: &Path,
    key: &str,
) -> Result<&'a mut Map<String, Value>, InstallError> {
    if !root.contains_key(key) {
        root.insert(key.to_owned(), Value::Object(Map::new()));
    }
    match root.get_mut(key).and_then(Value::as_object_mut) {
        Some(map) => Ok(map),
        None => Err(unexpected(path, format!("{key} must be an object"))),
    }
}

fn set_string(object: &mut Map<String, Value>, key: &str, value: &str) {
    if object.get(key).and_then(Value::as_str) == Some(value) {
        return;
    }
    object.insert(key.to_owned(), Value::String(value.to_owned()));
}

fn set_bool(object: &mut Map<String, Value>, key: &str, value: bool) {
    if object.get(key).and_then(Value::as_bool) == Some(value) {
        return;
    }
    object.insert(key.to_owned(), Value::Bool(value));
}

fn set_string_array(object: &mut Map<String, Value>, key: &str, values: &[&str]) {
    let matches = object
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|array| {
            array.len() == values.len()
                && array
                    .iter()
                    .zip(values)
                    .all(|(value, expected)| value.as_str() == Some(*expected))
        });
    if matches {
        return;
    }
    object.insert(
        key.to_owned(),
        Value::Array(
            values
                .iter()
                .map(|text| Value::String((*text).to_owned()))
                .collect(),
        ),
    );
}
