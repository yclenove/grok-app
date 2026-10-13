//! TOML edits via `toml_edit` 0.25. Comments on keys this editor does not own stay put.
//!
//! A plan serializes the document only after a real structural change. Parsing
//! an unchanged file and writing it back is intentionally avoided so comment
//! placement and spacing survive an idempotent install.

use std::path::{Path, PathBuf};

use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, TableLike, Value};

use super::error::{unexpected, unparseable, InstallError};
use super::fsutil::read_bytes;
use super::plan::FileChange;
use super::SERVER_NAME;

/// Apply `edit`. `Ok(None)` means the file bytes should stay as they are.
pub(crate) fn edit_toml(
    path: &Path,
    edit: impl FnOnce(&mut DocumentMut) -> Result<bool, InstallError>,
) -> Result<Option<FileChange>, InstallError> {
    let Some(bytes) = read_bytes(path)? else {
        let mut doc = DocumentMut::new();
        let changed = edit(&mut doc)?;
        if !changed || doc.as_table().is_empty() {
            return Ok(None);
        }
        return Ok(Some(FileChange::rewrite(path, None, render(&doc), None)));
    };
    let text = std::str::from_utf8(&bytes).map_err(|source| unparseable(path, source))?;
    let mut doc: DocumentMut = text.parse().map_err(|source| unparseable(path, source))?;
    let changed = edit(&mut doc)?;
    if !changed {
        return Ok(None);
    }
    Ok(Some(FileChange::rewrite(
        path,
        Some(bytes),
        render(&doc),
        None,
    )))
}

fn render(doc: &DocumentMut) -> Vec<u8> {
    let mut text = doc.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.into_bytes()
}

/// True when `[mcp_servers.grok-computer-use]` exists, whatever its shape.
pub(crate) fn mcp_present(path: &Path) -> Result<bool, InstallError> {
    let Some(bytes) = read_bytes(path)? else {
        return Ok(false);
    };
    let text = std::str::from_utf8(&bytes).map_err(|source| unparseable(path, source))?;
    if text.trim().is_empty() {
        return Ok(false);
    }
    let doc: DocumentMut = text.parse().map_err(|source| unparseable(path, source))?;
    let Some(item) = doc.get("mcp_servers") else {
        return Ok(false);
    };
    let table =
        as_table_like(item).ok_or_else(|| unexpected(path, "mcp_servers must be a table"))?;
    Ok(table.contains_key(SERVER_NAME))
}

/// Set `[mcp_servers.grok-computer-use]` command and args. Does not add `enabled` unless it is false.
pub(crate) fn install_mcp(
    doc: &mut DocumentMut,
    path: &Path,
    gcu: &str,
    fix_enabled_false: bool,
) -> Result<bool, InstallError> {
    validate_mcp(doc, path, fix_enabled_false)?;
    let mut changed = false;
    if doc.get("mcp_servers").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert("mcp_servers", Item::Table(table));
        changed = true;
    }
    let missing_server = {
        let Some(servers) = doc.get("mcp_servers").and_then(as_table_like) else {
            return Err(unexpected(path, "mcp_servers must be a table"));
        };
        !servers.contains_key(SERVER_NAME)
    };
    let inline_parent = doc
        .get("mcp_servers")
        .and_then(Item::as_value)
        .and_then(Value::as_inline_table)
        .is_some();
    if missing_server {
        let Some(servers) = doc.get_mut("mcp_servers").and_then(as_table_like_mut) else {
            return Err(unexpected(path, "mcp_servers must be a table"));
        };
        let child = if inline_parent {
            Item::Value(Value::InlineTable(InlineTable::new()))
        } else {
            Item::Table(Table::new())
        };
        servers.insert(SERVER_NAME, child);
        changed = true;
    }
    let Some(server) = doc
        .get_mut("mcp_servers")
        .and_then(as_table_like_mut)
        .and_then(|servers| servers.get_mut(SERVER_NAME))
        .and_then(as_table_like_mut)
    else {
        return Err(unexpected(path, "mcp_servers.grok-computer-use must be a table"));
    };
    changed |= set_string(server, "command", gcu);
    changed |= set_strings(server, "args", &["mcp"]);
    if fix_enabled_false && server.get("enabled").and_then(item_bool) == Some(false) {
        server.insert("enabled", toml_edit::value(true));
        changed = true;
    }
    Ok(changed)
}

/// Remove only `mcp_servers.grok-computer-use`. The parent table is left in place.
pub(crate) fn remove_mcp(doc: &mut DocumentMut, path: &Path) -> Result<bool, InstallError> {
    let Some(item) = doc.get("mcp_servers") else {
        return Ok(false);
    };
    if as_table_like(item).is_none() {
        return Err(unexpected(path, "mcp_servers must be a table"));
    }
    let present = as_table_like(item).is_some_and(|table| table.contains_key(SERVER_NAME));
    if !present {
        return Ok(false);
    }
    let Some(servers) = doc.get_mut("mcp_servers").and_then(as_table_like_mut) else {
        return Err(unexpected(path, "mcp_servers must be a table"));
    };
    servers.remove(SERVER_NAME);
    Ok(true)
}

/// Install `notify = ["<gcu>", "turn-ended", ...]` , chaining an existing string array.
pub(crate) fn install_notify(
    doc: &mut DocumentMut,
    path: &Path,
    gcu: &str,
) -> Result<bool, InstallError> {
    let existing = match doc.get("notify") {
        None => None,
        Some(item) => Some(require_strings(item, path, "notify")?),
    };
    let Some(parts) = existing else {
        write_strings(doc, "notify", &[gcu.to_owned(), "turn-ended".to_owned()]);
        return Ok(true);
    };
    if is_our_notify(&parts) {
        if parts[0] == gcu {
            return Ok(false);
        }
        let mut updated = parts;
        updated[0] = gcu.to_owned();
        write_strings(doc, "notify", &updated);
        return Ok(true);
    }
    let mut wrapped = vec![
        gcu.to_owned(),
        "turn-ended".to_owned(),
        "--previous-notify".to_owned(),
    ];
    wrapped.extend(parts);
    write_strings(doc, "notify", &wrapped);
    Ok(true)
}

/// Restore a chained `notify`, or remove the key when this installer created it.
pub(crate) fn remove_notify(doc: &mut DocumentMut, path: &Path) -> Result<bool, InstallError> {
    let Some(item) = doc.get("notify") else {
        return Ok(false);
    };
    let parts = require_strings(item, path, "notify")?;
    if !is_our_notify(&parts) {
        return Ok(false);
    }
    if let Some(index) = parts.iter().position(|part| part == "--previous-notify") {
        let tail = parts[index + 1..].to_vec();
        write_strings(doc, "notify", &tail);
    } else {
        doc.remove("notify");
    }
    Ok(true)
}

fn validate_mcp(doc: &DocumentMut, path: &Path, fix_enabled: bool) -> Result<(), InstallError> {
    let Some(item) = doc.get("mcp_servers") else {
        return Ok(());
    };
    let Some(servers) = as_table_like(item) else {
        return Err(unexpected(path, "mcp_servers must be a table"));
    };
    let Some(server_item) = servers.get(SERVER_NAME) else {
        return Ok(());
    };
    let Some(server) = as_table_like(server_item) else {
        return Err(unexpected(path, "mcp_servers.grok-computer-use must be a table"));
    };
    if let Some(command) = server.get("command") {
        if item_str(command).is_none() {
            return Err(unexpected(
                path,
                "mcp_servers.grok-computer-use.command must be a string",
            ));
        }
    }
    if let Some(args) = server.get("args") {
        require_strings(args, path, "mcp_servers.grok-computer-use.args")?;
    }
    if fix_enabled {
        if let Some(enabled) = server.get("enabled") {
            if item_bool(enabled).is_none() {
                return Err(unexpected(
                    path,
                    "mcp_servers.grok-computer-use.enabled must be a boolean",
                ));
            }
        }
    }
    Ok(())
}

fn set_string(table: &mut dyn TableLike, key: &str, want: &str) -> bool {
    if table.get(key).and_then(item_str) == Some(want) {
        return false;
    }
    table.insert(key, toml_edit::value(want));
    true
}

fn set_strings(table: &mut dyn TableLike, key: &str, want: &[&str]) -> bool {
    if let Some(item) = table.get(key) {
        if let Ok(have) = string_list(item) {
            if have.iter().map(String::as_str).eq(want.iter().copied()) {
                return false;
            }
        }
    }
    table.insert(key, string_array(want));
    true
}

fn write_strings(doc: &mut DocumentMut, key: &str, parts: &[String]) {
    let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
    doc.insert(key, string_array(&refs));
}

fn string_array(parts: &[&str]) -> Item {
    let mut array = Array::new();
    for part in parts {
        array.push(*part);
    }
    toml_edit::value(array)
}

fn require_strings(item: &Item, path: &Path, detail: &str) -> Result<Vec<String>, InstallError> {
    string_list(item)
        .map_err(|()| unexpected(path, format!("{detail} must be an array of strings")))
}

fn string_list(item: &Item) -> Result<Vec<String>, ()> {
    let Some(array) = item.as_value().and_then(Value::as_array) else {
        return Err(());
    };
    let mut out = Vec::with_capacity(array.len());
    for value in array {
        let Some(text) = value.as_str() else {
            return Err(());
        };
        out.push(text.to_owned());
    }
    Ok(out)
}

pub(crate) fn is_our_notify(parts: &[String]) -> bool {
    parts.len() >= 2 && is_gcu_program(&parts[0]) && parts[1] == "turn-ended"
}

fn is_gcu_program(text: &str) -> bool {
    matches!(
        Path::new(text).file_name().and_then(|name| name.to_str()),
        Some("gcu" | "gcu.exe")
    )
}

fn item_str(item: &Item) -> Option<&str> {
    item.as_value().and_then(Value::as_str)
}

fn item_bool(item: &Item) -> Option<bool> {
    item.as_value().and_then(Value::as_bool)
}

fn as_table_like(item: &Item) -> Option<&dyn TableLike> {
    if let Some(table) = item.as_table() {
        return Some(table);
    }
    item.as_value()
        .and_then(Value::as_inline_table)
        .map(|table| table as &dyn TableLike)
}

fn as_table_like_mut(item: &mut Item) -> Option<&mut dyn TableLike> {
    if item.as_table().is_some() {
        return item.as_table_mut().map(|table| table as &mut dyn TableLike);
    }
    if item.as_value().and_then(Value::as_inline_table).is_some() {
        return item
            .as_value_mut()
            .and_then(Value::as_inline_table_mut)
            .map(|table| table as &mut dyn TableLike);
    }
    None
}

/// `<home>/.codex/config.toml`.
pub(crate) fn codex_user_config(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}
