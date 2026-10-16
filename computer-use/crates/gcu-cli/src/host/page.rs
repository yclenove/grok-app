//! Approval document. HTML and CSS are compiled in; nothing is loaded at runtime.
use super::dialog::DialogCopy;
use gcu_host_api::{AppInfo, HandoffReason, ImageRef, Risk, SignatureState};
use gcu_mcp::{fill, sanitize_plain, text};

const HTML: &str = include_str!("../../../../ui/approval.html");
const CSS: &str = include_str!("../../../../ui/window.css");

/// Render the approval document for `locale`. Dynamic text is escaped. The icon is auxiliary.
pub fn approval_page(
    locale: &str,
    app: &AppInfo,
    reason: Option<&str>,
    risk: Risk,
    always_enabled: bool,
) -> String {
    let locale = gcu_mcp::locale_id(locale);
    let name = sanitize_plain(&app.name);
    let question = escape(&fill(
        locale,
        "approval.question",
        &[("name", name.as_str())],
    ));
    let mut slots = vec![
        ("slot:lang", locale.to_string()),
        ("slot:css", CSS.to_string()),
        ("slot:question", question),
        ("slot:risk", escape(&text(locale, risk_key(risk)))),
        (
            "slot:signature",
            escape(&signature_text(locale, app.identity.signature)),
        ),
        ("slot:publisher", escape(&publisher_text(locale, app))),
        (
            "slot:path",
            escape(&path_text(locale, &app.identity.image_path)),
        ),
        ("slot:name", escape(&name)),
        ("slot:icon", icon_html(locale, app.icon.as_ref())),
        ("slot:reason", escape(&reason_text(locale, reason))),
        (
            "slot:always_attr",
            if always_enabled {
                String::new()
            } else {
                "hidden".into()
            },
        ),
    ];
    for key in [
        "approval.title",
        "approval.identity_heading",
        "approval.signature_label",
        "approval.publisher_label",
        "approval.path_label",
        "approval.name_label",
        "approval.icon_label",
        "approval.plugin_note",
        "approval.reason_label",
        "approval.hint",
        "approval.choice.turn",
        "approval.choice.session",
        "approval.choice.always",
        "approval.choice.deny",
    ] {
        slots.push((key, escape(&text(locale, key))));
    }
    render(HTML, &slots)
}

/// Contract problems in a rendered document. Empty means the page is safe to show.
pub fn window_problems(html: &str) -> Vec<&'static str> {
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
    if !html.contains("class=\"aux\"") {
        problems.push("aux");
    }
    problems
}

pub(crate) fn app_prompt(
    app: &AppInfo,
    reason: Option<&str>,
    risk: Risk,
    always_enabled: bool,
    timeout_secs: u64,
) -> DialogCopy {
    let locale = gcu_mcp::active_locale();
    let name = sanitize_plain(&app.name);
    let mut rows = vec![
        ("deny".into(), text(locale, "approval.choice.deny")),
        ("turn".into(), text(locale, "approval.choice.turn")),
        ("session".into(), text(locale, "approval.choice.session")),
    ];
    if always_enabled {
        rows.push(("always".into(), text(locale, "approval.choice.always")));
    }
    let body = [
        fill(locale, "approval.question", &[("name", name.as_str())]),
        text(locale, "approval.identity_heading"),
        format!(
            "{}: {}",
            text(locale, "approval.signature_label"),
            signature_text(locale, app.identity.signature)
        ),
        format!(
            "{}: {}",
            text(locale, "approval.publisher_label"),
            publisher_text(locale, app)
        ),
        format!(
            "{}: {}",
            text(locale, "approval.path_label"),
            path_text(locale, &app.identity.image_path)
        ),
        format!("{}: {name}", text(locale, "approval.name_label")),
        text(locale, "approval.icon_label"),
        format!(
            "{}: {}",
            text(locale, "approval.reason_label"),
            reason_text(locale, reason)
        ),
        text(locale, risk_key(risk)),
        text(locale, "approval.plugin_note"),
    ]
    .join("\n");
    DialogCopy {
        title: text(locale, "approval.title"),
        text: cap(&body),
        rows,
        timeout_secs,
    }
}

pub(crate) fn confirm_prompt(
    app: &AppInfo,
    summary: &str,
    crop_attached: bool,
    timeout_secs: u64,
) -> DialogCopy {
    let locale = gcu_mcp::active_locale();
    let name = sanitize_plain(&app.name);
    let summary = sanitize_plain(summary);
    let mut lines = vec![
        fill(
            locale,
            "approval.confirm.question",
            &[("name", name.as_str())],
        ),
        format!(
            "{}: {summary}",
            text(locale, "approval.confirm.summary_label")
        ),
    ];
    if crop_attached {
        lines.push(text(locale, "approval.crop_attached"));
    }
    DialogCopy {
        title: text(locale, "approval.confirm.title"),
        text: cap(&lines.join("\n")),
        rows: vec![
            ("deny".into(), text(locale, "approval.confirm.deny")),
            ("allow".into(), text(locale, "approval.confirm.allow")),
        ],
        timeout_secs,
    }
}

pub(crate) fn handoff_prompt(
    app: &AppInfo,
    reason: &HandoffReason,
    timeout_secs: u64,
) -> DialogCopy {
    let locale = gcu_mcp::active_locale();
    let name = sanitize_plain(&app.name);
    let body = match reason {
        HandoffReason::Login => fill(locale, "approval.handoff.login", &[("name", name.as_str())]),
        HandoffReason::Captcha => fill(
            locale,
            "approval.handoff.captcha",
            &[("name", name.as_str())],
        ),
        HandoffReason::SystemPrompt => fill(
            locale,
            "approval.handoff.system_prompt",
            &[("name", name.as_str())],
        ),
        HandoffReason::Other(detail) => format!(
            "{}: {}",
            text(locale, "approval.handoff.detail_label"),
            sanitize_plain(detail)
        ),
    };
    DialogCopy {
        title: text(locale, "approval.handoff.title"),
        text: cap(&body),
        rows: vec![("dismiss".into(), text(locale, "approval.handoff.dismiss"))],
        timeout_secs,
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
    let start = html.find("<script>")?;
    let rest = &html[start + "<script>".len()..];
    let end = rest.find("</script>")?;
    Some(&rest[..end])
}

fn signature_text(locale: &str, signature: SignatureState) -> String {
    let key = match signature {
        SignatureState::Valid => "approval.signature.valid",
        SignatureState::Invalid => "approval.signature.invalid",
        SignatureState::Unsigned => "approval.signature.unsigned",
        SignatureState::Unknown => "approval.signature.unknown",
    };
    text(locale, key)
}

fn publisher_text(locale: &str, app: &AppInfo) -> String {
    if let Some(publisher) = app
        .identity
        .publisher
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return sanitize_plain(publisher);
    }
    if let Some(team) = app
        .identity
        .team_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        return sanitize_plain(team);
    }
    text(locale, "approval.publisher_missing")
}

fn path_text(locale: &str, path: &str) -> String {
    if path.is_empty() {
        text(locale, "approval.path_missing")
    } else {
        sanitize_plain(path)
    }
}

fn reason_text(locale: &str, reason: Option<&str>) -> String {
    match reason.map(sanitize_plain).filter(|text| !text.is_empty()) {
        Some(text) => text,
        None => text(locale, "approval.reason_empty"),
    }
}

fn risk_key(risk: Risk) -> &'static str {
    match risk {
        Risk::High => "approval.risk.high",
        Risk::Low => "approval.risk.low",
    }
}

fn icon_html(locale: &str, icon: Option<&ImageRef>) -> String {
    let Some(icon) = icon else {
        return escape(&text(locale, "approval.publisher_missing"));
    };
    let mime = match icon.mime.as_str() {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        _ => return escape(&text(locale, "approval.publisher_missing")),
    };
    if icon.as_bytes().is_empty() || icon.as_bytes().len() > 65_536 {
        return escape(&text(locale, "approval.publisher_missing"));
    }
    format!(
        "<img class=\"aux\" alt=\"{}\" width=\"{}\" height=\"{}\" src=\"data:{mime};base64,{}\">",
        escape(&text(locale, "approval.icon_label")),
        icon.width,
        icon.height,
        base64(icon.as_bytes())
    )
}

fn escape(input: &str) -> String {
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

fn cap(text: &str) -> String {
    text.chars().take(1200).collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let value = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | bytes[index + 2] as u32;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
        out.push(TABLE[(value & 63) as usize] as char);
        index += 3;
    }
    let rest = bytes.len() - index;
    if rest == 1 {
        let value = (bytes[index] as u32) << 16;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let value = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}
