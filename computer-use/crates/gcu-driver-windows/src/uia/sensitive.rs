//! Best-effort, non-exhaustive terminal/IDE/embedded execution-surface hints.
//!
//! Only marks native roles for core's existing TERMINAL_LIKE/denylist pipeline.
//! No policy, redaction, or action refusal is performed in this driver.
use super::cache::Properties;

const CORE_TERMS: &str = include_str!("../../../gcu-core/data/exec_surfaces.txt");
const LOCAL_TERMS: &[&str] = &[
    "terminal",
    "console",
    "repl",
    "powershell",
    "bash",
    "cmd",
    "conhost",
    "shell",
    "command prompt",
    "debug",
    "run task",
    "run build task",
    "run code",
    "run selection",
    "run file",
    "run cell",
    "run python",
    "start debugging",
    "run and debug",
    "debugger",
    "debug repl",
    "execute task",
    "execute cell",
    "execute selection",
    "task runner",
    "终端",
    "控制台",
    "运行任务",
    "调试控制台",
    "运行代码",
    "開始偵錯",
    "執行工作",
];

/// Recognize role/name/class/ID hints, including execution buttons in IDE menus.
/// Short ASCII terms use word boundaries to avoid `cmd` inside an ordinary ID.
/// Names are untrusted; a miss does not prove an execution surface safe.
/// Synchronous, no IO or cancellation points.
pub fn is_execution_surface(properties: &Properties) -> bool {
    [
        properties.aria_role.as_deref(),
        properties.name.as_deref(),
        properties.class_name.as_deref(),
        properties.identifier.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|part| {
        let lower = normalize_hint(part);
        if matches!(properties.control_type, 50000 | 50011)
            && matches!(lower.as_str(), "run" | "execute")
        {
            return true;
        }
        LOCAL_TERMS
            .iter()
            .copied()
            .chain(
                CORE_TERMS
                    .lines()
                    .filter(|line| !line.is_empty() && !line.starts_with('#')),
            )
            .any(|term| keyword_hit(&lower, term))
    })
}

fn keyword_hit(text: &str, term: &str) -> bool {
    if term.is_ascii() && (term.len() <= 3 || term == "debug") {
        text.split(|c: char| !c.is_alphanumeric())
            .any(|word| word == term)
    } else {
        text.contains(term)
    }
}

// AutomationIds commonly use dot paths and camelCase (e.g. tasks.runTask).
fn normalize_hint(text: &str) -> String {
    let mut out = String::new();
    let mut lower_before = false;
    for c in text.chars() {
        if c.is_ascii_uppercase() && lower_before {
            out.push(' ');
        }
        if matches!(c, '.' | '_' | '-') {
            out.push(' ');
        } else {
            out.extend(c.to_lowercase());
        }
        lower_before = c.is_ascii_lowercase();
    }
    out.trim().to_owned()
}
