//! Sensitive execution surface heuristics for Linux AT-SPI.
//!
//! # Heuristic Guarantees
//!
//! **尽力而为、非完备 (Best-effort, non-exhaustive)**:
//!
//! Recognizing execution surfaces such as embedded IDE terminals, REPLs, Web SSH,
//! container consoles, and debug sessions through native AT-SPI roles, native role
//! identifiers, and accessible names is inherently heuristic. Attackers or specialized
//! applications can present terminal surfaces without standard names or roles.
//! Therefore, these heuristics provide defense-in-depth and signal core policies to
//! refuse automation or require handoff, but do not provide a complete safety proof.

use gcu_core::model::NodeFlags;
use std::sync::OnceLock;

const KEYWORDS_RAW: &str = include_str!("../../data/exec_surfaces.txt");

static KEYWORDS: OnceLock<Vec<&'static str>> = OnceLock::new();

fn get_keywords() -> &'static [&'static str] {
    KEYWORDS.get_or_init(|| {
        KEYWORDS_RAW
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect()
    })
}

/// Detect whether an AT-SPI accessible node represents a terminal or console execution surface.
///
/// **Note**: This detection is best-effort and non-exhaustive (尽力而为、非完备).
///
/// Matches against:
/// 1. `atspi::Role::Terminal`
/// 2. Native role containing `"terminal"` or `"console"`
/// 3. Accessible name containing any term from `data/exec_surfaces.txt` (case-insensitive)
pub fn is_terminal_like(role: atspi::Role, native_role: &str, name: Option<&str>) -> bool {
    if role == atspi::Role::Terminal {
        return true;
    }

    let native_lower = native_role.to_ascii_lowercase();
    if native_lower.contains("terminal") || native_lower.contains("console") {
        return true;
    }

    if let Some(name_str) = name {
        let name_lower = name_str.to_ascii_lowercase();

        // Check word-by-word or substring match against keyword catalog
        for keyword in get_keywords() {
            let kw_lower = keyword.to_ascii_lowercase();
            // Single words or short acronyms like "sh" should match on word boundaries
            if kw_lower.len() <= 3 {
                let is_word_match = name_lower
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word == kw_lower);
                if is_word_match {
                    return true;
                }
            } else if name_lower.contains(&kw_lower) {
                return true;
            }
        }
    }

    false
}

/// Compute node flags for sensitive terminal surfaces.
///
/// Returns `NodeFlags::TERMINAL_LIKE` when detected, or `NodeFlags::empty()`.
pub fn terminal_node_flags(role: atspi::Role, native_role: &str, name: Option<&str>) -> NodeFlags {
    if is_terminal_like(role, native_role, name) {
        NodeFlags::TERMINAL_LIKE
    } else {
        NodeFlags::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_role_is_recognized() {
        assert!(is_terminal_like(
            atspi::Role::Terminal,
            "ATSPI_ROLE_TERMINAL",
            None
        ));
        assert_eq!(
            terminal_node_flags(atspi::Role::Terminal, "ATSPI_ROLE_TERMINAL", None),
            NodeFlags::TERMINAL_LIKE
        );
    }

    #[test]
    fn terminal_name_keywords_recognized() {
        assert!(is_terminal_like(
            atspi::Role::Panel,
            "ATSPI_ROLE_PANEL",
            Some("Integrated Terminal")
        ));
        assert!(is_terminal_like(
            atspi::Role::DocumentWeb,
            "ATSPI_ROLE_DOCUMENT_WEB",
            Some("Web SSH Session - Production")
        ));
        assert!(is_terminal_like(
            atspi::Role::Frame,
            "ATSPI_ROLE_FRAME",
            Some("bash - /home/user")
        ));
        assert!(is_terminal_like(
            atspi::Role::Entry,
            "ATSPI_ROLE_ENTRY",
            Some("Debug Console REPL")
        ));
    }

    #[test]
    fn ordinary_controls_not_terminal_like() {
        assert!(!is_terminal_like(
            atspi::Role::Button,
            "ATSPI_ROLE_PUSH_BUTTON",
            Some("Save and Exit")
        ));
        assert!(!is_terminal_like(
            atspi::Role::Entry,
            "ATSPI_ROLE_ENTRY",
            Some("Username")
        ));
        assert!(!is_terminal_like(
            atspi::Role::CheckBox,
            "ATSPI_ROLE_CHECK_BOX",
            Some("Accept Terms")
        ));
    }
}
