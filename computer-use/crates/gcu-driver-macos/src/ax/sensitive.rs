//! Terminal and in-app execution-surface hints.
//!
//! **尽力而为、非完备.** Role, subrole, and name keywords are a best-effort
//! signal, not a complete detector. A console can be unlabeled, and an ordinary
//! control can reuse a word such as "screen". The driver only marks the node.
//! Refusal and handoff belong to the router and policy, which already treat a
//! `native_role` containing `terminal` as `TERMINAL_LIKE`.

use std::sync::OnceLock;

const LOCAL_KEYWORDS: &str = include_str!("../../data/exec_surfaces.txt");
const CORE_KEYWORDS: &str = include_str!("../../../gcu-core/data/exec_surfaces.txt");

static KEYWORDS: OnceLock<Vec<&'static str>> = OnceLock::new();

fn keywords() -> &'static [&'static str] {
    KEYWORDS.get_or_init(|| {
        let mut out: Vec<&str> = Vec::new();
        for line in LOCAL_KEYWORDS.lines().chain(CORE_KEYWORDS.lines()) {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !out.iter().any(|kept| kept.eq_ignore_ascii_case(line)) {
                out.push(line);
            }
        }
        out
    })
}

/// True when the role, subrole, or name looks like a terminal, REPL, debugger,
/// task runner, or in-browser shell.
///
/// Tokens of three characters or fewer match a whole word so `sh` does not
/// match inside another word. Longer tokens match a case-insensitive substring.
/// This is best-effort and incomplete (尽力而为、非完备).
pub fn is_terminal_like(native_role: &str, subrole: Option<&str>, name: Option<&str>) -> bool {
    for part in [Some(native_role), subrole, name].into_iter().flatten() {
        let lower = part.to_ascii_lowercase();
        if lower.contains("terminal") || lower.contains("console") {
            return true;
        }
        for keyword in keywords() {
            if keyword_hit(&lower, keyword) {
                return true;
            }
        }
    }
    false
}

fn keyword_hit(haystack_lower: &str, keyword: &str) -> bool {
    let keyword = keyword.to_ascii_lowercase();
    if keyword.len() <= 3 {
        haystack_lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word == keyword)
    } else {
        haystack_lower.contains(&keyword)
    }
}
