//! Onboarding document. The HTML and CSS are compiled in; nothing is loaded later.
use super::onboarding::{CardStatus, OnboardingView};
use gcu_mcp::{locale_id, text};

const DOCUMENT: &str = include_str!("onboarding.embed.html");

/// Render the onboarding document. Dynamic text is escaped. The deny button stays focused.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn onboarding_page(locale: &str, view: &OnboardingView) -> String {
    let locale = locale_id(locale);
    let mut slots = vec![
        ("slot:lang", escape_html(locale)),
        (
            "slot:ax_status",
            escape_html(&status_text(locale, view.accessibility)),
        ),
        (
            "slot:ax_class",
            escape_html(status_class(view.accessibility)),
        ),
        (
            "slot:screen_status",
            escape_html(&status_text(locale, view.screen_recording)),
        ),
        (
            "slot:screen_class",
            escape_html(status_class(view.screen_recording)),
        ),
        ("slot:restart_hint", escape_html(&hint_text(locale, view))),
        ("slot:restart_hint_attr", attr_hidden(view.restart_enabled)),
        ("slot:restart_error", escape_html(&error_text(locale, view))),
        (
            "slot:restart_error_attr",
            attr_hidden(view.restart_error.is_some()),
        ),
        ("slot:restart_attr", attr_hidden(view.restart_enabled)),
    ];
    for key in [
        "onboarding.title",
        "onboarding.lead",
        "onboarding.accessibility.title",
        "onboarding.accessibility.body",
        "onboarding.screen_recording.title",
        "onboarding.screen_recording.body",
        "onboarding.open_settings",
        "onboarding.restart",
        "onboarding.hint",
        "onboarding.dismiss",
    ] {
        slots.push((key, escape_html(&text(locale, key))));
    }
    render(DOCUMENT, &slots)
}

/// Contract problems in a rendered document. Empty means the page is safe to show.
///
/// The script must not treat Enter as a click. Blocking: none. Timeout: none.
/// Cancellation: none. Thread safety: pure.
#[must_use]
pub fn onboarding_problems(html: &str) -> Vec<&'static str> {
    let mut problems = Vec::new();
    let lower = html.to_ascii_lowercase();
    if lower.contains("<form") {
        problems.push("form");
    }
    if lower.contains("<select") {
        problems.push("select");
    }
    if lower.contains("<input") {
        problems.push("input");
    }
    let script = script_body(html).unwrap_or("");
    if script.is_empty() {
        problems.push("script");
    }
    if script.contains("Enter") || script.contains("keyCode") || script.contains("which") {
        problems.push("enter");
    }
    if !script.contains("Escape") {
        problems.push("escape");
    }
    if script.contains("alert(") || script.contains("confirm(") || script.contains("prompt(") {
        problems.push("dialog");
    }
    if !html.contains("id=\"deny\"") || !html.contains("autofocus") {
        problems.push("focus");
    }
    problems
}

/// Script that copies status text into the page without parsing it as HTML.
///
/// `<` is escaped so a catalog string cannot close a script element. Blocking:
/// none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn status_script(locale: &str, view: &OnboardingView) -> String {
    let locale = locale_id(locale);
    format!(
        "(function(){{var ax=document.getElementById(\"ax-status\");var screen=document.getElementById(\"screen-status\");var hint=document.getElementById(\"restart-hint\");var error=document.getElementById(\"restart-error\");var restart=document.getElementById(\"restart\");if(!ax||!screen||!hint||!error||!restart)return;ax.textContent={ax};ax.className={ax_class};screen.textContent={screen_text};screen.className={screen_class};hint.textContent={hint};hint.hidden={hint_hidden};error.textContent={error};error.hidden={error_hidden};restart.hidden={restart_hidden};}})();",
        ax = js_embed(&status_text(locale, view.accessibility)),
        ax_class = js_embed(&format!("status {}", status_class(view.accessibility))),
        screen_text = js_embed(&status_text(locale, view.screen_recording)),
        screen_class = js_embed(&format!("status {}", status_class(view.screen_recording))),
        hint = js_embed(&hint_text(locale, view)),
        hint_hidden = bool_js(!view.restart_enabled),
        error = js_embed(&error_text(locale, view)),
        error_hidden = bool_js(view.restart_error.is_none()),
        restart_hidden = bool_js(!view.restart_enabled),
    )
}

/// HTML-escape text that is inserted into the document.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// JS string literal. `<` and `>` cannot close an HTML script element.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
#[must_use]
pub fn js_embed(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
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

/// Allow the initial document and refuse remote, file, and Settings navigations.
///
/// `true` means the webview may navigate. Blocking: none. Timeout: none.
/// Cancellation: none. Thread safety: pure.
#[must_use]
pub fn navigation_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    !(lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("file:")
        || lower.starts_with("javascript:")
        || lower.starts_with("x-apple."))
}

fn status_text(locale: &str, status: CardStatus) -> String {
    let key = match status {
        CardStatus::Missing => "onboarding.status.missing",
        CardStatus::Granted => "onboarding.status.granted",
        CardStatus::NeedsRestart => "onboarding.status.needs_restart",
    };
    text(locale, key)
}

fn status_class(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Missing => "missing",
        CardStatus::Granted | CardStatus::NeedsRestart => "granted",
    }
}

fn hint_text(locale: &str, view: &OnboardingView) -> String {
    if view.restart_enabled {
        text(locale, "onboarding.restart_hint")
    } else {
        String::new()
    }
}

fn error_text(locale: &str, view: &OnboardingView) -> String {
    if view.restart_error.is_some() {
        text(locale, "onboarding.restart_failed")
    } else {
        String::new()
    }
}

fn attr_hidden(visible: bool) -> String {
    if visible {
        String::new()
    } else {
        "hidden".into()
    }
}

fn bool_js(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn render(template: &str, slots: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find("}}") {
            let key = &after[..end];
            if let Some((_, value)) = slots.iter().find(|(name, _)| *name == key) {
                out.push_str(value);
            }
            rest = &after[end + 2..];
            continue;
        }
        out.push_str(&rest[start..]);
        break;
    }
    out.push_str(rest);
    out
}

fn script_body(html: &str) -> Option<&str> {
    let start = html.find("<script>")? + "<script>".len();
    let end = html[start..].find("</script>")? + start;
    Some(&html[start..end])
}
