//! Unified write layer for App agent-home `config.toml`.
//!
//! Independent session-data mode (`GROK_HOME` = App agent-home) may write
//! allowlisted keys. Shared mode always refuses path resolve / strict write so
//! the App never rewrites the user's personal `~/.grok/config.toml`.
//!
//! Pure TOML helpers are line-oriented upserts (no full parse) so unrelated
//! sections and secrets stay intact.
//!
//! ## Safety
//! - Key match is **exact** (never `starts_with`) so `yolo` cannot clobber `yolo_mode`.
//! - Table headers accept trailing comments (`[ui] # note`).
//! - Runtime permission/compat overlays use the structural provider snapshot;
//!   malformed configuration is rejected without guessing a repair.

#[cfg(test)]
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::paths::{agent_config_toml, ensure_app_dirs};

/// Serialize independent agent-home config writers (cascade sync + heal + prefs).
static CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Run `f` while holding the agent-home config write lock.
pub fn with_config_write_lock<T>(f: impl FnOnce() -> T) -> T {
    let _guard = CONFIG_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

/// Normalize session_data_mode to `independent` | `shared`.
pub fn normalize_mode(session_data_mode: &str) -> &'static str {
    if session_data_mode.trim().eq_ignore_ascii_case("shared") {
        "shared"
    } else {
        "independent"
    }
}

/// Error message when shared mode refuses a write-path resolve.
pub const SHARED_MODE_REFUSED: &str =
    "shared session mode: agent-home config.toml writes refused (never rewrite ~/.grok)";

/// Resolve App agent-home `config.toml` for writes.
///
/// - independent → `…/agent-home/config.toml` (ensures app dirs)
/// - shared → `Err` (refuse; do not return `~/.grok/config.toml`)
pub fn resolve_writable_config_path(session_data_mode: &str) -> Result<PathBuf, String> {
    if normalize_mode(session_data_mode) == "shared" {
        return Err(SHARED_MODE_REFUSED.into());
    }
    let _ = ensure_app_dirs();
    Ok(agent_config_toml())
}

fn bool_lit(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

fn finish_join(original: &str, lines: &[String]) -> String {
    let mut joined = lines.join("\n");
    if (original.ends_with('\n') || original.is_empty()) && !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

fn finish_join_always_nl(_original: &str, lines: &[String]) -> String {
    let mut joined = lines.join("\n");
    if !joined.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

/// 定位结束表名的 `]`，跳过**引号内**的 `]`。
///
/// TOML 表名可以带引号包住任意字符：`[model."claude-glm-5.3[1M]"]` 的**第一个**
/// `]` 属于名字本身。早先用 `find(']')` 取到的就是它，于是名字被截断成
/// `model."claude-glm-5.3[1M]`、剩余部分是 `"]` —— 既不是空也不是注释，整个表头
/// 被判定为**不是表头**，这张表的字段就被算进了上一张表的 scope，去重时把上一张
/// 表的同名赋值当成重复项删掉（用户的 `[model.ada-anthropic]` 正是这样丢掉了
/// `model` / `base_url` / `api_key` / `api_backend`）。
fn closing_bracket_index(t: &str, is_array: bool) -> Option<usize> {
    let bytes = t.as_bytes();
    let mut in_quotes = false;
    let mut i = if is_array { 2 } else { 1 };
    while i < bytes.len() {
        match bytes[i] {
            b'"' => in_quotes = !in_quotes,
            b']' if !in_quotes => {
                if is_array {
                    if bytes.get(i + 1) == Some(&b']') {
                        return Some(i);
                    }
                } else {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Parse a TOML table / array-table header.
///
/// Accepts trailing comments: `[ui] # note`, `[[hooks]] # x`.
/// Returns `(is_array_table, table_name)`.
pub fn parse_table_header(trimmed: &str) -> Option<(bool, &str)> {
    let t = trimmed.trim();
    if !t.starts_with('[') {
        return None;
    }
    let is_array = t.starts_with("[[");
    let end = closing_bracket_index(t, is_array)?;
    let (name, after_close) = if is_array {
        (t[2..end].trim(), t[end + 2..].trim())
    } else {
        (t[1..end].trim(), t[end + 1..].trim())
    };
    if name.is_empty() {
        return None;
    }
    // Only whitespace / comments allowed after the closing bracket.
    if !after_close.is_empty() && !after_close.starts_with('#') {
        return None;
    }
    Some((is_array, name))
}

/// Key of a simple `key = value` assignment line (None for headers / comments / blank).
pub fn assignment_key(trimmed: &str) -> Option<&str> {
    let t = trimmed.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let eq = t.find('=')?;
    let key = t[..eq].trim();
    if key.is_empty() {
        return None;
    }
    Some(key)
}

/// Upsert a bare top-level `key = value` assignment (not inside a `[table]`).
pub fn set_top_level_assignment(text: &str, key: &str, value: &str) -> String {
    let line_val = format!("{key} = {value}");
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut in_table = false;
    let mut first_table_idx: Option<usize> = None;

    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if parse_table_header(trimmed).is_some() {
            if first_table_idx.is_none() {
                first_table_idx = Some(i);
            }
            in_table = true;
            continue;
        }
        if in_table {
            continue;
        }
        if assignment_key(trimmed) == Some(key) {
            lines[i] = line_val;
            return finish_join(text, &lines);
        }
    }

    if let Some(idx) = first_table_idx {
        lines.insert(idx, line_val);
        return finish_join_always_nl(text, &lines);
    }

    let base = text.trim_end();
    if base.is_empty() {
        format!("{line_val}\n")
    } else {
        format!("{base}\n{line_val}\n")
    }
}

/// Upsert top-level `key = true|false`.
pub fn set_top_level_bool(text: &str, key: &str, value: bool) -> String {
    set_top_level_assignment(text, key, bool_lit(value))
}

/// Upsert top-level `key = "value"` (quoted string).
#[allow(dead_code)]
pub fn set_top_level_string(text: &str, key: &str, value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    set_top_level_assignment(text, key, &format!("\"{escaped}\""))
}

/// Upsert `key = value` under `[table]` without touching other sections/keys.
///
/// Header match is by table **name**, so `[ui] # comment` counts as `[ui]`.
/// Key match is **exact** (not prefix).
pub fn set_table_key(text: &str, table: &str, key: &str, value: &str, quoted: bool) -> String {
    let header = format!("[{table}]");
    let line_val = if quoted {
        format!(
            "{key} = \"{}\"",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else {
        format!("{key} = {value}")
    };
    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut in_table = false;
    let mut table_start: Option<usize> = None;
    for i in 0..lines.len() {
        let trimmed = lines[i].trim();
        if let Some((is_array, name)) = parse_table_header(trimmed) {
            if !is_array && name == table {
                in_table = true;
                table_start = Some(i);
            } else if in_table {
                lines.insert(i, line_val);
                return finish_join_always_nl(text, &lines);
            } else {
                in_table = false;
            }
            continue;
        }
        if in_table && assignment_key(trimmed) == Some(key) {
            lines[i] = line_val;
            return finish_join(text, &lines);
        }
    }
    if let Some(start) = table_start {
        lines.insert(start + 1, line_val);
        return finish_join_always_nl(text, &lines);
    }
    let block = format!("\n{header}\n{line_val}\n");
    let base = text.trim_end();
    if base.is_empty() {
        format!("{header}\n{line_val}\n")
    } else {
        format!("{base}{block}")
    }
}

/// Upsert `[table] key = true|false`.
pub fn set_table_bool(text: &str, table: &str, key: &str, value: bool) -> String {
    set_table_key(text, table, key, bool_lit(value), false)
}

/// Pin App agent-home so Grok does **not** merge Claude/Cursor MCP catalogs.
///
/// Without this, `~/.claude.json` mcpServers (Playwright, open-websearch, …)
/// are imported by default and stall first-tool discovery for ~30s.
pub fn ensure_compat_mcp_disabled(text: &str) -> Result<String, String> {
    crate::providers::compat_text(text)
}

/// Write `[compat.claude/cursor] mcps = false` into independent agent-home.
pub fn apply_compat_mcp_disabled(session_data_mode: &str) -> Result<(), String> {
    crate::providers::sync_compat_runtime(session_data_mode)
}

/// Upsert `[table] key = "value"`.
pub fn set_table_string(text: &str, table: &str, key: &str, value: &str) -> String {
    set_table_key(text, table, key, value, true)
}

/// Parse a TOML bool literal (`true` / `false`).
pub fn parse_toml_bool(raw: &str) -> Option<bool> {
    match raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Parse a TOML string / bare value (strip surrounding quotes; drop inline `#`).
pub fn parse_toml_scalar(raw: &str) -> String {
    let s = raw.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        return s[1..s.len() - 1].to_string();
    }
    s.split('#').next().unwrap_or(s).trim().to_string()
}

fn value_for_key_in_scope<'a>(text: &'a str, table: Option<&str>, key: &str) -> Option<&'a str> {
    let mut current: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((is_array, name)) = parse_table_header(trimmed) {
            // Array tables are not used for simple get helpers.
            current = if is_array { None } else { Some(name) };
            continue;
        }
        let Some(k) = assignment_key(trimmed) else {
            continue;
        };
        if k != key {
            continue;
        }
        let in_scope = match table {
            None => current.is_none(),
            Some(t) => current == Some(t),
        };
        if in_scope {
            let eq = trimmed.find('=')?;
            return Some(trimmed[eq + 1..].trim());
        }
    }
    None
}

/// Read top-level bool when present.
#[allow(dead_code)]
pub fn get_top_level_bool(text: &str, key: &str) -> Option<bool> {
    value_for_key_in_scope(text, None, key).and_then(parse_toml_bool)
}

/// Read `[table].key` bool when present.
pub fn get_table_bool(text: &str, table: &str, key: &str) -> Option<bool> {
    value_for_key_in_scope(text, Some(table), key).and_then(parse_toml_bool)
}

/// Read top-level string / scalar when present.
#[allow(dead_code)]
pub fn get_top_level_string(text: &str, key: &str) -> Option<String> {
    value_for_key_in_scope(text, None, key)
        .map(parse_toml_scalar)
        .filter(|s| !s.is_empty())
}

/// Read `[table].key` string / scalar when present.
pub fn get_table_string(text: &str, table: &str, key: &str) -> Option<String> {
    value_for_key_in_scope(text, Some(table), key)
        .map(parse_toml_scalar)
        .filter(|s| !s.is_empty())
}

// ── Duplicate-key scan / heal ────────────────────────────────────────────────

/// Scope id for duplicate detection.
///
/// - Top-level: `""`
/// - Named table `[ui]`: `"ui"` (all `[ui]` sections share one scope — TOML
///   merges them and rejects duplicate keys across fragments)
/// - Array table instance `[[hooks]]`: `"[[hooks]]#N"` so each element is
///   independent (keys may repeat across elements)
#[cfg(test)]
fn scope_id_for_header(
    is_array: bool,
    name: &str,
    array_ordinals: &mut HashMap<String, usize>,
) -> String {
    if is_array {
        let n = array_ordinals.entry(name.to_string()).or_insert(0);
        let id = format!("[[{name}]]#{n}");
        *n += 1;
        id
    } else {
        name.to_string()
    }
}

/// Count duplicate assignments: same key twice in the same table scope.
///
/// Returns `(duplicate_assignment_lines, examples)` where examples are
/// `"scope.key"` strings (capped).
#[cfg(test)]
pub fn count_duplicate_assignments(text: &str) -> (usize, Vec<String>) {
    let mut scope = String::new();
    let mut array_ordinals: HashMap<String, usize> = HashMap::new();
    let mut last: HashMap<(String, String), usize> = HashMap::new();
    let mut dups = 0usize;
    let mut examples: Vec<String> = Vec::new();

    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some((is_array, name)) = parse_table_header(trimmed) {
            scope = scope_id_for_header(is_array, name, &mut array_ordinals);
            continue;
        }
        let Some(key) = assignment_key(trimmed) else {
            continue;
        };
        let map_key = (scope.clone(), key.to_string());
        if last.insert(map_key, i).is_some() {
            dups += 1;
            if examples.len() < 8 {
                let label = if scope.is_empty() {
                    key.to_string()
                } else {
                    format!("{scope}.{key}")
                };
                if !examples.contains(&label) {
                    examples.push(label);
                }
            }
        }
    }
    (dups, examples)
}

/// Strict write: read → transform → write agent-home config.toml.
/// Shared mode → `Err` via [`resolve_writable_config_path`].
pub fn update_config_toml(
    session_data_mode: &str,
    transform: impl FnOnce(&str) -> String,
) -> Result<PathBuf, String> {
    with_config_write_lock(|| {
        let path = resolve_writable_config_path(session_data_mode)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create agent-home: {e}"))?;
        }
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let next = transform(&existing);
        fs::write(&path, next).map_err(|e| format!("write config: {e}"))?;
        Ok(path)
    })
}

/// Soft-skip shared: `Ok(None)` without touching disk.
/// Independent: apply transform and return `Ok(Some(path))`.
///
/// Prefer this for AppSettings sync helpers that keep the toggle in App state
/// when session mode is shared.
pub fn update_config_toml_if_independent(
    session_data_mode: &str,
    transform: impl FnOnce(&str) -> String,
) -> Result<Option<PathBuf>, String> {
    if normalize_mode(session_data_mode) == "shared" {
        return Ok(None);
    }
    update_config_toml(session_data_mode, transform).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuse_shared_path_resolve() {
        let err = resolve_writable_config_path("shared").unwrap_err();
        assert!(err.contains("shared"), "{err}");
        assert!(err.contains("refused") || err.contains("~/.grok"), "{err}");
        // Case-insensitive.
        assert!(resolve_writable_config_path("SHARED").is_err());
    }

    /// 表名里带 `]` 的引号名（`[model."claude-glm-5.3[1M]"]`）必须被认成表头，
    /// 否则它的字段会被算进上一张表的 scope，去重时把上一张表的同名赋值删掉。
    #[test]
    fn parse_table_header_quoted_name_with_bracket() {
        assert_eq!(
            parse_table_header(r#"[model."claude-glm-5.3[1M]"]"#),
            Some((false, r#"model."claude-glm-5.3[1M]""#))
        );
        assert_eq!(
            parse_table_header(r#"[model."a[b].c"] # note"#),
            Some((false, r#"model."a[b].c""#))
        );
        // 数组表名同样适用
        assert_eq!(
            parse_table_header(r#"[[model."x[1]"]]"#),
            Some((true, r#"model."x[1]""#))
        );
    }

    /// 别名表排在 provider 表之后时，provider 的传输字段不能被当成重复项删掉。
    #[test]
    fn runtime_overlay_keeps_previous_table_when_next_name_has_bracket() {
        let text = "\
[model.ada-anthropic]
model = \"claude-glm-5.3-flash[1M]\"
base_url = \"http://relay/anthropic/v1\"
api_key = \"sk-test\"
api_backend = \"messages\"

[model.\"claude-glm-5.3[1M]\"]
model = \"claude-glm-5.3[1M]\"
base_url = \"http://relay/anthropic/v1\"
api_key = \"sk-test\"
app_model_for = \"ada-anthropic\"
";
        let (dup, examples) = count_duplicate_assignments(text);
        assert_eq!(dup, 0, "不同表之间不应被判为重复键: {examples:?}");
        // Runtime config no longer guesses duplicate-key repairs. Exercise the
        // real strict overlay, retaining both provider and quoted alias tables.
        let updated = ensure_compat_mcp_disabled(text).unwrap();
        assert_eq!(count_duplicate_assignments(&updated).0, 0);
        assert_eq!(
            get_table_string(&updated, "model.ada-anthropic", "api_backend").as_deref(),
            Some("messages")
        );
        assert_eq!(
            get_table_string(&updated, r#"model."claude-glm-5.3[1M]""#, "app_model_for").as_deref(),
            Some("ada-anthropic")
        );
    }

    #[test]
    fn parse_table_header_trailing_comment() {
        assert_eq!(parse_table_header("[ui]"), Some((false, "ui")));
        assert_eq!(parse_table_header("[ui] # note"), Some((false, "ui")));
        assert_eq!(
            parse_table_header("[compat.claude]"),
            Some((false, "compat.claude"))
        );
        assert_eq!(parse_table_header("[[hooks]]"), Some((true, "hooks")));
        assert_eq!(parse_table_header("[[hooks]] # x"), Some((true, "hooks")));
        assert!(parse_table_header("[ui] junk").is_none());
        assert!(parse_table_header("not a header").is_none());
    }

    #[test]
    fn assignment_key_exact() {
        assert_eq!(assignment_key("yolo = true"), Some("yolo"));
        assert_eq!(assignment_key("yolo_mode = false"), Some("yolo_mode"));
        assert_eq!(assignment_key("# yolo = true"), None);
        assert_eq!(assignment_key("[ui]"), None);
    }

    #[test]
    fn set_and_get_top_level_bool() {
        let t = set_top_level_bool("", "auto_wake_enabled", true);
        assert!(t.contains("auto_wake_enabled = true"));
        assert_eq!(get_top_level_bool(&t, "auto_wake_enabled"), Some(true));

        let existing = "[ui]\nyolo = false\n\n[subagents]\nenabled = true\n";
        let next = set_top_level_bool(existing, "workflows_enabled", false);
        assert_eq!(get_top_level_bool(&next, "workflows_enabled"), Some(false));
        let ui_pos = next.find("[ui]").unwrap();
        let key_pos = next.find("workflows_enabled").unwrap();
        assert!(key_pos < ui_pos);
        assert!(next.contains("yolo = false"));

        let again = set_top_level_bool(&next, "workflows_enabled", true);
        assert_eq!(get_top_level_bool(&again, "workflows_enabled"), Some(true));
        assert_eq!(again.matches("workflows_enabled").count(), 1);
    }

    #[test]
    fn set_table_key_exact_not_prefix() {
        // Historical bug: starts_with("yolo") rewrote yolo_mode and left a
        // second yolo= line → CLI "duplicate key" at spawn.
        let base = "[ui]\npermission_mode = \"always-approve\"\nyolo_mode = false\nyolo = true\n";
        let next = set_table_bool(base, "ui", "yolo", true);
        assert!(next.contains("yolo_mode = false"), "{next}");
        assert_eq!(next.matches("yolo =").count(), 1, "{next}");
        assert!(next.contains("yolo = true"), "{next}");
        assert_eq!(count_duplicate_assignments(&next).0, 0);
    }

    #[test]
    fn set_table_key_matches_header_with_comment() {
        let base = "[ui] # perms\nyolo = false\n";
        let next = set_table_string(base, "ui", "permission_mode", "always-approve");
        // Must update existing [ui] fragment — not append a second [ui].
        assert_eq!(
            next.lines()
                .filter(|l| parse_table_header(l.trim()).is_some_and(|(_, n)| n == "ui"))
                .count(),
            1,
            "{next}"
        );
        assert!(
            next.contains("permission_mode = \"always-approve\""),
            "{next}"
        );
        assert!(next.contains("yolo = false"), "{next}");
        assert_eq!(count_duplicate_assignments(&next).0, 0, "{next}");
    }

    #[test]
    fn ensure_compat_mcp_disabled_sets_both() {
        let t = ensure_compat_mcp_disabled("").unwrap();
        assert_eq!(get_table_bool(&t, "compat.claude", "mcps"), Some(false));
        assert_eq!(get_table_bool(&t, "compat.cursor", "mcps"), Some(false));
        let again = ensure_compat_mcp_disabled(&t).unwrap();
        assert_eq!(get_table_bool(&again, "compat.claude", "mcps"), Some(false));
    }

    #[test]
    fn set_and_get_table_bool_and_string() {
        let t = set_table_bool("", "memory", "enabled", true);
        assert!(t.contains("[memory]"));
        assert!(t.contains("enabled = true"));
        assert_eq!(get_table_bool(&t, "memory", "enabled"), Some(true));

        let t2 = set_table_string(&t, "ui", "permission_mode", "acceptEdits");
        assert_eq!(
            get_table_string(&t2, "ui", "permission_mode").as_deref(),
            Some("acceptEdits")
        );
        assert_eq!(get_table_bool(&t2, "memory", "enabled"), Some(true));

        let t3 = set_table_bool(&t2, "memory", "enabled", false);
        assert_eq!(get_table_bool(&t3, "memory", "enabled"), Some(false));
        assert_eq!(t3.matches("enabled =").count(), 1);
    }

    #[test]
    fn top_level_string_roundtrip() {
        let t = set_top_level_string("", "note", "plain");
        assert!(t.contains("note = \"plain\""));
        assert_eq!(get_top_level_string(&t, "note").as_deref(), Some("plain"));
    }

    #[test]
    fn invalid_configuration_is_rejected_without_rewriting_or_backup() {
        let _fixture = crate::session_migration::test_home::IsolatedAppHome::acquire(
            "grok-agent-config-rejection",
        );
        let cfg = crate::paths::agent_config_toml();
        for text in [
            "[ui]\nyolo = true\nyolo = false\n",
            "[ui]\nyolo = false\n[ui]\nyolo = true\n",
            "compat = false\n",
        ] {
            fs::write(&cfg, text).unwrap();
            assert!(apply_compat_mcp_disabled("independent").is_err());
            assert_eq!(fs::read_to_string(&cfg).unwrap(), text);
            assert!(!cfg.with_file_name("config.toml.tmp").exists());
            assert!(!fs::read_dir(cfg.parent().unwrap())
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("config.toml.bak")));
        }
        fs::write(&cfg, "ui = { yolo_mode = false }\n").unwrap();
        apply_compat_mcp_disabled("independent").unwrap();
        let valid = fs::read_to_string(&cfg).unwrap();
        assert!(valid.contains("yolo_mode = false"));
        apply_compat_mcp_disabled("independent").unwrap();
        assert_eq!(fs::read_to_string(&cfg).unwrap(), valid);
        apply_compat_mcp_disabled("shared").unwrap();
        assert_eq!(fs::read_to_string(&cfg).unwrap(), valid);
    }

    #[test]
    fn shared_soft_skip_and_independent_write() {
        let _fixture = crate::session_migration::test_home::IsolatedAppHome::acquire(
            "grok-agent-config-write",
        );

        assert_eq!(
            update_config_toml_if_independent("shared", |t| set_top_level_bool(
                t,
                "auto_wake_enabled",
                true
            ))
            .unwrap(),
            None
        );
        // Strict refuse.
        assert!(update_config_toml("shared", |t| t.to_string()).is_err());

        let path = update_config_toml_if_independent("independent", |t| {
            set_top_level_bool(t, "auto_wake_enabled", true)
        })
        .unwrap()
        .expect("path");
        assert!(path.ends_with("config.toml"));
        let disk = fs::read_to_string(&path).unwrap();
        assert_eq!(get_top_level_bool(&disk, "auto_wake_enabled"), Some(true));
    }

    #[test]
    fn fixture_restores_inherited_home_child() {
        crate::session_migration::test_home::run_fixture_home_contract(|| {
            invalid_configuration_is_rejected_without_rewriting_or_backup();
            shared_soft_skip_and_independent_write();
        });
    }
    #[test]
    fn fixture_restores_inherited_home_in_fresh_process() {
        crate::session_migration::test_home::assert_fixture_home_contract(
            "agent_home_config::tests::fixture_restores_inherited_home_child",
        );
    }
}
