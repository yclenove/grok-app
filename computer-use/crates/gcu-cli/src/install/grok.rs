//! Grok Build MCP config and Stop hook.
//!
//! `grok mcp add` rewrites `config.toml` and strips comments (probed on grok
//! 1.0.52), so this editor uses `toml_edit` instead of the CLI. Grok has no
//! Codex-style `notify`. ADR 0001 installs a `Stop` hook and the script
//! forwards stdin only when `reason` is `end_turn`. The hook lives under
//! `<home>/.grok/hooks` for both scopes. Apply asks before it changes anything
//! under that directory.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::error::{unexpected, InstallError};
use super::fsutil::read_bytes;
use super::json::edit_json;
use super::plan::{FileChange, Plan, PlanAction};
use super::toml::{self, edit_toml};
use super::{
    gcu_string, push_change, require_context, status_of, ClientId, ClientInstaller, ClientStatus,
    InstallCtx, Scope,
};

const MARKER: &str = "# gcu-computer-use-stop";
const HOOK_JSON: &str = "computer-use-stop.json";
const HOOK_SCRIPT: &str = "computer-use-turn-ended.sh";

pub(crate) struct GrokInstaller;

impl ClientInstaller for GrokInstaller {
    fn id(&self) -> ClientId {
        ClientId::Grok
    }

    fn plan(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let gcu = gcu_string(ctx)?;
        let script = script_path(ctx.home);
        let command = hook_command(
            script
                .to_str()
                .ok_or(InstallError::PathNotUnicode { what: "stop hook" })?,
        );
        let mut files = Vec::new();
        push_change(
            &mut files,
            edit_toml(&config_path(ctx), |doc| {
                toml::install_mcp(doc, &config_path(ctx), &gcu, true)
            })?,
        );
        push_change(
            &mut files,
            edit_json(&hook_json_path(ctx.home), |map| {
                upsert_stop(
                    map,
                    &hook_json_path(ctx.home),
                    &script_display(&script)?,
                    &command,
                )
            })?,
        );
        push_change(&mut files, plan_script(&script, &gcu)?);
        Ok(Plan::new(
            ClientId::Grok,
            PlanAction::Install,
            files,
            Vec::new(),
        ))
    }

    fn plan_removal(&self, ctx: &InstallCtx<'_>) -> Result<Plan, InstallError> {
        require_context(ctx)?;
        let script = script_path(ctx.home);
        let command = hook_command(&script_display(&script)?);
        let mut files = Vec::new();
        push_change(
            &mut files,
            edit_toml(&config_path(ctx), |doc| {
                toml::remove_mcp(doc, &config_path(ctx))
            })?,
        );
        let hook = edit_json(&hook_json_path(ctx.home), |map| {
            remove_stop(
                map,
                &hook_json_path(ctx.home),
                &script_display(&script)?,
                &command,
            )
        })?;
        push_change(&mut files, delete_if_empty_object(hook));
        push_change(&mut files, plan_script_removal(&script)?);
        Ok(Plan::new(
            ClientId::Grok,
            PlanAction::Uninstall,
            files,
            Vec::new(),
        ))
    }

    fn status(&self, ctx: &InstallCtx<'_>) -> Result<ClientStatus, InstallError> {
        let present = grok_present(ctx)?;
        let plan = self.plan(ctx)?;
        Ok(status_of(present, present && plan.is_noop()))
    }
}

fn config_path(ctx: &InstallCtx<'_>) -> PathBuf {
    match ctx.scope {
        Scope::User => ctx.home.join(".grok/config.toml"),
        Scope::Project => ctx.project_dir.join(".grok/config.toml"),
    }
}

fn hook_dir(home: &Path) -> PathBuf {
    home.join(".grok/hooks")
}

fn hook_json_path(home: &Path) -> PathBuf {
    hook_dir(home).join(HOOK_JSON)
}

fn script_path(home: &Path) -> PathBuf {
    hook_dir(home).join(HOOK_SCRIPT)
}

fn script_display(path: &Path) -> Result<String, InstallError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(InstallError::PathNotUnicode { what: "stop hook" })
}

fn grok_present(ctx: &InstallCtx<'_>) -> Result<bool, InstallError> {
    require_context(ctx)?;
    if toml::mcp_present(&config_path(ctx))? {
        return Ok(true);
    }
    let script = script_path(ctx.home);
    if read_bytes(&script)?.is_some_and(|bytes| is_ours(&bytes)) {
        return Ok(true);
    }
    let json_path = hook_json_path(ctx.home);
    let Some(bytes) = read_bytes(&json_path)? else {
        return Ok(false);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|source| super::error::unparseable(&json_path, source))?;
    if text.trim().is_empty() {
        return Ok(false);
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|source| super::error::unparseable(&json_path, source))?;
    let script_text = script_display(&script)?;
    let command = hook_command(&script_text);
    let Some(map) = value.as_object() else {
        return Err(unexpected(&json_path, "root must be an object"));
    };
    validate_hook_shape(map, &json_path)?;
    Ok(stop_has_ours(map, &script_text, &command))
}

fn plan_script(path: &Path, gcu: &str) -> Result<Option<FileChange>, InstallError> {
    let desired = script_text(gcu).into_bytes();
    match read_bytes(path)? {
        None => Ok(Some(FileChange::rewrite(path, None, desired, Some(0o755)))),
        Some(bytes) if bytes == desired => Ok(None),
        Some(bytes) if is_ours(&bytes) => Ok(Some(FileChange::rewrite(
            path,
            Some(bytes),
            desired,
            Some(0o755),
        ))),
        Some(_) => Err(InstallError::ForeignFile {
            path: path.to_path_buf(),
        }),
    }
}

fn plan_script_removal(path: &Path) -> Result<Option<FileChange>, InstallError> {
    match read_bytes(path)? {
        Some(bytes) if is_ours(&bytes) => Ok(Some(FileChange::delete(path, bytes))),
        _ => Ok(None),
    }
}

fn is_ours(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    text.lines().any(|line| line.trim() == MARKER)
}

fn script_text(gcu: &str) -> String {
    let quoted = shell_single_quote(gcu);
    let mut out = String::from("#!/bin/sh\n");
    out.push_str(MARKER);
    out.push('\n');
    out.push_str("# Forward a Grok Build Stop hook to gcu only when reason is end_turn.\n");
    out.push_str("input=$(cat)\n");
    out.push_str(
        "reason=$(printf '%s\\n' \"$input\" | sed -n 's/.*\"reason\"[[:space:]]*:[[:space:]]*\"\\([^\"]*\\)\".*/\\1/p' | head -n 1)\n",
    );
    out.push_str("if [ \"$reason\" = \"end_turn\" ]; then\n");
    out.push_str("  printf '%s\\n' \"$input\" | ");
    out.push_str(&quoted);
    out.push_str(" turn-ended\nfi\nexit 0\n");
    out
}

fn shell_single_quote(text: &str) -> String {
    let mut out = String::from("'");
    for ch in text.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn hook_command(script: &str) -> String {
    if shell_safe(script) {
        script.to_owned()
    } else {
        format!("\"{}\"", script.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn shell_safe(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '/' | '.' | '_' | '+' | '-' | '@' | '%' | '=' | ',')
        })
}

fn upsert_stop(
    map: &mut Map<String, Value>,
    path: &Path,
    script: &str,
    command: &str,
) -> Result<(), InstallError> {
    validate_hook_shape(map, path)?;
    if stop_has_ours(map, script, command) {
        normalize_ours(map, script, command);
        return Ok(());
    }
    let hooks = ensure_object(map, path, "hooks")?;
    let stop = ensure_array(hooks, path, "Stop")?;
    stop.push(hook_group(command));
    Ok(())
}

fn remove_stop(
    map: &mut Map<String, Value>,
    path: &Path,
    script: &str,
    command: &str,
) -> Result<(), InstallError> {
    validate_hook_shape(map, path)?;
    let empty_hooks = {
        let Some(hooks) = map.get_mut("hooks").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let Some(stop) = hooks.get_mut("Stop").and_then(Value::as_array_mut) else {
            return Ok(());
        };
        for group in stop.iter_mut() {
            if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                list.retain(|hook| !hook_is_ours(hook, script, command));
            }
        }
        stop.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|list| !list.is_empty())
        });
        if stop.is_empty() {
            hooks.remove("Stop");
        }
        hooks.is_empty()
    };
    if empty_hooks {
        map.remove("hooks");
    }
    Ok(())
}

fn validate_hook_shape(map: &Map<String, Value>, path: &Path) -> Result<(), InstallError> {
    let Some(hooks) = map.get("hooks") else {
        return Ok(());
    };
    let Some(hooks) = hooks.as_object() else {
        return Err(unexpected(path, "hooks must be an object"));
    };
    if let Some(stop) = hooks.get("Stop") {
        if !stop.is_array() {
            return Err(unexpected(path, "hooks.Stop must be an array"));
        }
    }
    Ok(())
}

fn stop_has_ours(map: &Map<String, Value>, script: &str, command: &str) -> bool {
    let Some(stop) = map
        .get("hooks")
        .and_then(Value::as_object)
        .and_then(|hooks| hooks.get("Stop"))
        .and_then(Value::as_array)
    else {
        return false;
    };
    stop.iter().any(|group| {
        group
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(|hook| hook_is_ours(hook, script, command)))
    })
}

fn normalize_ours(map: &mut Map<String, Value>, script: &str, command: &str) {
    let Some(stop) = map
        .get_mut("hooks")
        .and_then(Value::as_object_mut)
        .and_then(|hooks| hooks.get_mut("Stop"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for group in stop {
        let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        for hook in list {
            if !hook_is_ours(hook, script, command) {
                continue;
            }
            let Some(object) = hook.as_object_mut() else {
                continue;
            };
            if object.get("type").and_then(Value::as_str) != Some("command") {
                object.insert("type".into(), Value::String("command".into()));
            }
            if object.get("command").and_then(Value::as_str) != Some(command) {
                object.insert("command".into(), Value::String(command.into()));
            }
            if !object.get("timeout").is_some_and(is_ten) {
                object.insert("timeout".into(), Value::from(10));
            }
        }
    }
}

fn hook_is_ours(hook: &Value, script: &str, command: &str) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|existing| is_our_command(existing, script, command))
}

fn is_our_command(existing: &str, script: &str, command: &str) -> bool {
    existing == command || existing == script || unquote(existing) == script
}

fn unquote(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 2 {
        let open = bytes[0];
        let close = bytes[bytes.len() - 1];
        if (open == b'"' && close == b'"') || (open == b'\'' && close == b'\'') {
            return &text[1..text.len() - 1];
        }
    }
    text
}

fn is_ten(value: &Value) -> bool {
    value.as_i64() == Some(10) || value.as_u64() == Some(10)
}

fn hook_group(command: &str) -> Value {
    serde_json::json!({
        "hooks": [{
            "type": "command",
            "command": command,
            "timeout": 10
        }]
    })
}

fn ensure_object<'a>(
    map: &'a mut Map<String, Value>,
    path: &Path,
    key: &str,
) -> Result<&'a mut Map<String, Value>, InstallError> {
    if !map.contains_key(key) {
        map.insert(key.to_owned(), Value::Object(Map::new()));
    }
    match map.get_mut(key).and_then(Value::as_object_mut) {
        Some(child) => Ok(child),
        None => Err(unexpected(path, format!("{key} must be an object"))),
    }
}

fn ensure_array<'a>(
    map: &'a mut Map<String, Value>,
    path: &Path,
    key: &str,
) -> Result<&'a mut Vec<Value>, InstallError> {
    if !map.contains_key(key) {
        map.insert(key.to_owned(), Value::Array(Vec::new()));
    }
    match map.get_mut(key).and_then(Value::as_array_mut) {
        Some(child) => Ok(child),
        None => Err(unexpected(path, format!("{key} must be an array"))),
    }
}

fn delete_if_empty_object(change: Option<FileChange>) -> Option<FileChange> {
    let change = change?;
    let empty = change.after.as_ref().is_some_and(|after| {
        serde_json::from_slice::<Value>(after)
            .ok()
            .is_some_and(|value| value.as_object().is_some_and(Map::is_empty))
    });
    if !empty {
        return Some(change);
    }
    let path = change.path;
    change
        .before
        .map(|before| FileChange::delete(&path, before))
}
