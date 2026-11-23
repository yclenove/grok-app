//! Sensitive-action risk.
//!
//! A mutation inside a browser or a web area is high risk. A safe-looking name,
//! a disguised name, or an icon with no text does not lower that risk. Outside
//! those contexts, high risk is a sensitive term, a password field, or a
//! sensitive chord. Unnamed native targets are no longer elevated (owner
//! decision 2026-11-15: usability first; revisit with the safety work).
//!
//! Web areas use strict confirmation unless the setting is Off. Off is a real
//! opt-out, including for web areas. Assessment is synchronous and does not
//! block, time out, or cancel.

use super::guard::{classify_chord, ChordClass};
use super::target::{self, Resolved};
use super::terms::TermIndex;
use super::{ActionIntent, Tool};
use crate::model::{Role, States};
use crate::tree::ProcessedTree;
use gcu_host_api::{AppInfo, ConfirmationLevel};

/// Risk facts the engine turns into a confirm-or-allow decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RiskAssessment {
    /// The action is high risk under architecture §7.5.
    pub high: bool,
    /// The target is inside a web area. Effective confirmation is strict unless Off.
    pub web: bool,
}

/// Assess `intent`.
///
/// `None` from the user-activity clock is not consulted here. Blocking: none.
/// Timeout: none. Cancellation: none. Thread safety: shared references only.
pub(crate) fn assess(
    terms: &TermIndex,
    app: &AppInfo,
    window_url: Option<&str>,
    tree: Option<&ProcessedTree>,
    intent: &ActionIntent,
) -> RiskAssessment {
    assess_with_browser(terms, browser(app, window_url), tree, intent, false)
}

pub(crate) fn clipboard_high_risk(
    terms: &TermIndex,
    app: &gcu_protocol::AppInfo,
    tree: &ProcessedTree,
    intent: &ActionIntent,
) -> bool {
    let Some(identity) = &app.identity else {
        return true;
    };
    let Some(image) = identity.executable.as_deref() else {
        return true;
    };
    assess_with_browser(
        terms,
        browser_identity_strings(&identity.id, image),
        Some(tree),
        intent,
        true,
    )
    .high
}

fn assess_with_browser(
    terms: &TermIndex,
    browser: bool,
    tree: Option<&ProcessedTree>,
    intent: &ActionIntent,
    unknown_elevates: bool,
) -> RiskAssessment {
    let resolved = tree.and_then(|tree| target::resolve(tree, intent));
    let web = resolved.as_ref().is_some_and(|hit| hit.in_web);
    // Confirmations judge native targets by terms, password fields and chords
    // alone (usability first, 2026-11). The clipboard path still treats an
    // unknown-semantic target as elevated. Web areas and browsers stay elevated.
    let elevated = web || browser || (unknown_elevates && unknown(resolved.as_ref()));
    let signal = term_hit(terms, resolved.as_ref())
        || password(tree, resolved.as_ref(), intent.tool)
        || chord_signal(intent, elevated)
        || newline_text(intent, elevated);
    let high = if elevated && intent.tool.is_mutating() {
        true
    } else {
        signal
    };
    RiskAssessment { high, web }
}

/// Whether this mutation needs its own confirmation.
///
/// Off confirms nothing. Standard confirms high-risk actions, and every
/// mutation inside a web area. Strict confirms every mutation.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: pure.
pub(crate) fn needs_confirm(
    level: ConfirmationLevel,
    assessment: RiskAssessment,
    mutating: bool,
) -> bool {
    match level {
        ConfirmationLevel::Off => false,
        ConfirmationLevel::Standard if assessment.web => mutating,
        ConfirmationLevel::Standard => assessment.high,
        ConfirmationLevel::Strict => mutating,
    }
}

fn term_hit(terms: &TermIndex, resolved: Option<&Resolved<'_>>) -> bool {
    resolved.is_some_and(|hit| {
        let labels = target::labels(hit.node);
        terms.hit(&labels)
    })
}

fn password(tree: Option<&ProcessedTree>, resolved: Option<&Resolved<'_>>, tool: Tool) -> bool {
    if resolved.is_some_and(|hit| hit.node.node.states.contains(States::PASSWORD)) {
        return true;
    }
    if !matches!(tool, Tool::TypeText | Tool::SetValue) {
        return false;
    }
    tree.and_then(target::focused)
        .is_some_and(|node| node.node.states.contains(States::PASSWORD))
}

fn chord_signal(intent: &ActionIntent, elevated: bool) -> bool {
    let Some(key) = intent.key.as_deref() else {
        return false;
    };
    match classify_chord(key) {
        ChordClass::Sensitive => true,
        ChordClass::Activate | ChordClass::Custom => elevated,
        ChordClass::Ordinary | ChordClass::Blacklisted | ChordClass::Invalid => false,
    }
}

fn newline_text(intent: &ActionIntent, elevated: bool) -> bool {
    elevated
        && intent
            .text
            .as_deref()
            .is_some_and(|text| text.contains('\n') || text.contains('\r'))
}

fn unknown(resolved: Option<&Resolved<'_>>) -> bool {
    // A point with no element under it (a canvas, a drawing surface) in a native
    // app is not treated as unknown: confirming every such click made those
    // apps unusable. Web and browser targets stay elevated by their own rules.
    let Some(hit) = resolved else {
        return false;
    };
    if hit.execution {
        return false;
    }
    let node = &hit.node.node;
    // A pop-up or combo box often has no name, only its current value.
    let value_label = match (&node.value, node.role) {
        (Some(crate::model::NodeValue::Text(text)), Role::PopUpButton | Role::ComboBox) => {
            text.as_str()
        }
        _ => "",
    };
    let name = node
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(value_label)
        .trim();
    let named_control = !name.is_empty()
        && matches!(
            hit.node.node.role,
            Role::Button
                | Role::Link
                | Role::MenuItem
                | Role::CheckBox
                | Role::Radio
                | Role::Switch
                | Role::TextField
                | Role::SearchField
                | Role::TextArea
                | Role::ComboBox
                | Role::PopUpButton
                | Role::Tab
                | Role::ListItem
                | Role::TreeItem
                | Role::Slider
        );
    if named_control {
        return false;
    }
    name.is_empty()
        || matches!(
            hit.node.node.role,
            Role::Unknown | Role::Canvas | Role::Image | Role::Group
        )
}

fn browser(app: &AppInfo, window_url: Option<&str>) -> bool {
    window_url.is_some_and(is_http) || browser_identity(app)
}

fn is_http(url: &str) -> bool {
    let folded = url.trim().to_ascii_lowercase();
    folded.starts_with("http://") || folded.starts_with("https://")
}

fn browser_identity(app: &AppInfo) -> bool {
    browser_identity_strings(&app.identity.key, &app.identity.image_path)
}
fn browser_identity_strings(key: &str, image: &str) -> bool {
    FRAGMENTS
        .iter()
        .any(|fragment| contains_ignore(key, fragment) || contains_ignore(image, fragment))
        || TOKENS
            .iter()
            .any(|token| word(key, token) || word(image, token))
}

fn contains_ignore(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(needle)
}

fn word(haystack: &str, token: &str) -> bool {
    super::text::ascii_word(haystack, token)
}

const FRAGMENTS: &[&str] = &[
    "com.apple.safari",
    "org.mozilla",
    "microsoft-edge",
    "company.thebrowser",
    "msedge",
];

const TOKENS: &[&str] = &[
    "chrome",
    "firefox",
    "safari",
    "brave",
    "chromium",
    "thebrowser",
];
