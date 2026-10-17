//! Export `computer-use/ui/locales/*.json` into the App `computer-use` catalog.
//!
//! English in those JSON files is the key authority. This module does not read
//! `src/i18n` and does not invent a second copy of the strings.

use std::collections::BTreeMap;

/// TypeScript binding for one catalog id (`pt-BR` → `ptBRComputerUse`).
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn binding_name(locale: &str) -> String {
    let mut parts = locale.split('-');
    let mut name = parts.next().unwrap_or(locale).to_string();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            name.extend(first.to_uppercase());
            name.push_str(chars.as_str());
        }
    }
    name.push_str("ComputerUse");
    name
}

/// One `computer-use.ts` module. Keys are sorted. Values use JS string escapes.
///
/// Returns an error when a value is not a JSON string. Blocking: none.
/// Timeout: none. Cancellation: none. Thread safety: pure.
pub fn export_module(locale: &str, source: &str) -> Result<String, &'static str> {
    let value: serde_json::Value = serde_json::from_str(source).map_err(|_| "parse")?;
    let object = value.as_object().ok_or("object")?;
    let mut rows = BTreeMap::new();
    for (key, item) in object {
        let text = item.as_str().ok_or("string")?;
        rows.insert(key.clone(), text.to_string());
    }
    Ok(render_module(locale, &rows))
}

fn render_module(locale: &str, rows: &BTreeMap<String, String>) -> String {
    let binding = binding_name(locale);
    let mut out = format!(
        "/** {locale} messages — domain: computer-use. Generated from computer-use/ui/locales/{locale}.json by `cargo xtask ui export-i18n`. Do not edit. */\n"
    );
    out.push_str(&format!("export const {binding} = {{\n"));
    for (key, value) in rows {
        out.push_str("  ");
        out.push_str(&js_string(key));
        out.push_str(": ");
        out.push_str(&js_string(value));
        out.push_str(",\n");
    }
    out.push_str("} as const;\n");
    out
}

/// JSON-compatible JS string literal.
#[must_use]
pub fn js_string(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{code:04x}", code = u32::from(control)));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}
