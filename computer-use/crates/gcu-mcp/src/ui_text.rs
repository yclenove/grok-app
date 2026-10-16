//! Approval copy. `computer-use/ui/locales/*.json` is the source until P4-02
//! exports `src/i18n/messages/<locale>/computer-use.ts` over these files.
//! English is the key authority. Locales stay lockstep with `en`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Locale ids, in the same order as the app `LOCALES` list.
pub const LOCALES: [&str; 15] = [
    "en", "de", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW",
];

const FILES: [(&str, &str); 15] = [
    ("en", include_str!("../../../ui/locales/en.json")),
    ("de", include_str!("../../../ui/locales/de.json")),
    ("es", include_str!("../../../ui/locales/es.json")),
    ("fil", include_str!("../../../ui/locales/fil.json")),
    ("fr", include_str!("../../../ui/locales/fr.json")),
    ("id", include_str!("../../../ui/locales/id.json")),
    ("it", include_str!("../../../ui/locales/it.json")),
    ("ja", include_str!("../../../ui/locales/ja.json")),
    ("ko", include_str!("../../../ui/locales/ko.json")),
    ("pt-BR", include_str!("../../../ui/locales/pt-BR.json")),
    ("ru", include_str!("../../../ui/locales/ru.json")),
    ("ta", include_str!("../../../ui/locales/ta.json")),
    ("uk", include_str!("../../../ui/locales/uk.json")),
    ("zh", include_str!("../../../ui/locales/zh.json")),
    ("zh-TW", include_str!("../../../ui/locales/zh-TW.json")),
];

type Catalogs = Vec<(&'static str, BTreeMap<String, String>)>;

fn catalogs() -> &'static Result<Catalogs, String> {
    static CACHE: OnceLock<Result<Catalogs, String>> = OnceLock::new();
    CACHE.get_or_init(load_catalogs)
}

fn load_catalogs() -> Result<Catalogs, String> {
    let mut loaded = Vec::with_capacity(FILES.len());
    for (locale, source) in FILES {
        let value: serde_json::Value =
            serde_json::from_str(source).map_err(|_| format!("parse {locale}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| format!("object {locale}"))?;
        let mut rows = BTreeMap::new();
        for (key, item) in object {
            let text = item
                .as_str()
                .ok_or_else(|| format!("string {locale} {key}"))?;
            rows.insert(key.clone(), text.to_string());
        }
        loaded.push((locale, rows));
    }
    Ok(loaded)
}

/// Map a POSIX locale tag to one shipped catalog. Unknown tags become `en`.
pub fn locale_id(tag: &str) -> &'static str {
    let head = tag.trim().split('.').next().unwrap_or(tag.trim());
    let normalized = head.replace('_', "-");
    for id in LOCALES {
        if id.eq_ignore_ascii_case(&normalized) {
            return id;
        }
    }
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("zh") {
        if lower.contains("hant")
            || lower.contains("-tw")
            || lower.contains("-hk")
            || lower.contains("-mo")
        {
            return "zh-TW";
        }
        return "zh";
    }
    if lower.starts_with("pt") {
        return "pt-BR";
    }
    if lower == "tl" || lower.starts_with("fil") {
        return "fil";
    }
    let language = lower.split('-').next().unwrap_or(lower.as_str());
    for id in LOCALES {
        if id.eq_ignore_ascii_case(language) {
            return id;
        }
    }
    "en"
}

/// Catalog for `LC_ALL`, then `LC_MESSAGES`, then `LANG`. `C` and `POSIX` are skipped.
///
/// This does not read `~/.grok` or any other user config file.
pub fn active_locale() -> &'static str {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if value.is_empty() || value == "C" || value == "POSIX" {
                continue;
            }
            return locale_id(&value);
        }
    }
    "en"
}

/// Catalog string for `key`. Empty when the catalogs failed to load or the key is absent.
pub fn text(locale: &str, key: &str) -> String {
    let Ok(rows) = catalogs() else {
        return String::new();
    };
    let locale = locale_id(locale);
    let Some((_, map)) = rows
        .iter()
        .find(|(id, _)| *id == locale)
        .or_else(|| rows.iter().find(|(id, _)| *id == "en"))
    else {
        return String::new();
    };
    map.get(key).cloned().unwrap_or_default()
}

/// Replace `{token}` pairs once. Replacements are not scanned again.
pub fn fill(locale: &str, key: &str, pairs: &[(&str, &str)]) -> String {
    let template = text(locale, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template.as_str();
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let token = &after[..end];
            if let Some((_, replacement)) = pairs.iter().find(|(name, _)| *name == token) {
                out.push_str(replacement);
                rest = &after[end + 1..];
                continue;
            }
        }
        out.push('{');
        rest = &rest[start + 1..];
    }
    out.push_str(rest);
    out
}

/// Plain text for a model reason or any other displayed string.
///
/// Bidirectional controls and other C0/C1 controls become U+FFFD. Newlines and
/// tabs become spaces. The result is at most 280 scalar values.
pub fn sanitize_plain(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if out.chars().count() >= 280 {
            break;
        }
        if matches!(ch, '\n' | '\r' | '\t') {
            out.push(' ');
            continue;
        }
        if is_hidden_control(ch) {
            out.push('\u{FFFD}');
            continue;
        }
        out.push(ch);
    }
    out
}

fn is_hidden_control(ch: char) -> bool {
    let code = u32::from(ch);
    code < 0x20
        || (0x7F..=0x9F).contains(&code)
        || matches!(
            code,
            0x61C | 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069
        )
}

/// Empty when every catalog matches `en` and the required translations exist.
pub fn audit() -> Vec<String> {
    let mut problems = Vec::new();
    let Ok(rows) = catalogs() else {
        problems.push("catalogs failed to load".into());
        return problems;
    };
    if rows.len() != LOCALES.len() {
        problems.push("locale count".into());
    }
    let Some((_, english)) = rows.iter().find(|(id, _)| *id == "en") else {
        problems.push("missing en".into());
        return problems;
    };
    for (locale, map) in rows {
        if map.len() != english.len() || map.keys().ne(english.keys()) {
            problems.push(format!("keys {locale}"));
        }
        for (key, value) in map {
            if value.trim().is_empty() {
                problems.push(format!("empty {locale} {key}"));
            }
            if value.contains('<') || value.contains('>') || value.contains("{{") {
                problems.push(format!("markup {locale} {key}"));
            }
            let expected = english.get(key).map(String::as_str).unwrap_or("");
            if placeholder_counts(value) != placeholder_counts(expected) {
                problems.push(format!("placeholders {locale} {key}"));
            }
        }
        if *locale != "en" {
            if map.get("approval.reason_label") == english.get("approval.reason_label") {
                problems.push(format!("untranslated reason {locale}"));
            }
            if map.get("approval.choice.deny") == english.get("approval.choice.deny") {
                problems.push(format!("untranslated deny {locale}"));
            }
        }
    }
    if text("zh", "approval.reason_label") != "模型提供" {
        problems.push("zh reason label".into());
    }
    if text("zh", "approval.elicitation") != "允许 Grok Computer Use 使用「{name}」吗？" {
        problems.push("zh elicitation".into());
    }
    problems
}

fn placeholder_counts(value: &str) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            let token = &after[..end];
            if !token.is_empty()
                && token
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                *counts.entry(token.to_string()).or_insert(0) += 1;
                rest = &after[end + 1..];
                continue;
            }
        }
        rest = &rest[start + 1..];
    }
    counts
}
