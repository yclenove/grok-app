//! Best-effort remote-execution and local-console gate.
//!
//! `data/exec_surfaces.txt` is a keyword list, not a complete detector. A hit
//! forces observation only. A miss does not mean the surface is safe: native
//! terminal, console, REPL, debugger, and task-runner roles are a separate
//! gate, and a mixed window with no resolvable target hands off.
//!
//! Matching is synchronous. It does not block, time out, cancel, or read the
//! filesystem at runtime.

use super::text::{fold, phrase_in};
use super::{ActionIntent, PolicyWindow};
use crate::tree::ProcessedTree;
use std::sync::OnceLock;

/// How the target relates to an execution surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceGate {
    /// Title or URL matched the best-effort remote table.
    Remote,
    /// The target is a known local execution surface.
    Execution,
    /// A mutating action has no target and the window mixes execution UI with other UI,
    /// or there is no tree to classify.
    Uncertain,
    /// No execution-surface evidence for this action.
    Clear,
}

pub(crate) struct SurfaceList {
    phrases: Vec<String>,
}

impl SurfaceList {
    pub(crate) fn builtin() -> &'static Self {
        static LIST: OnceLock<SurfaceList> = OnceLock::new();
        LIST.get_or_init(|| Self {
            phrases: load(include_str!("../../data/exec_surfaces.txt")),
        })
    }

    fn hit(&self, texts: &[&str]) -> bool {
        let folded: Vec<String> = texts
            .iter()
            .filter(|text| !text.is_empty())
            .map(|text| fold(text))
            .collect();
        self.phrases
            .iter()
            .any(|phrase| folded.iter().any(|text| phrase_in(text, phrase)))
    }
}

fn load(source: &str) -> Vec<String> {
    let mut phrases = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let folded = fold(line);
        if !folded.is_empty() && !phrases.contains(&folded) {
            phrases.push(folded);
        }
    }
    phrases
}

/// Classify the action's surface.
///
/// Remote keywords are checked first, on the window title, the window URL, and
/// node URLs. They are not applied to every node name, so an IDE titled
/// without those keywords stays on the native-console gate.
///
/// Blocking: none. Timeout: none. Cancellation: none. Thread safety: shared
/// references only.
pub(crate) fn gate(
    list: &SurfaceList,
    window: Option<&PolicyWindow>,
    tree: Option<&ProcessedTree>,
    intent: &ActionIntent,
) -> SurfaceGate {
    if remote(list, window, tree) {
        return SurfaceGate::Remote;
    }
    if let Some(tree) = tree {
        if let Some(resolved) = super::target::resolve(tree, intent) {
            if resolved.execution && intent.tool.is_mutating() {
                return SurfaceGate::Execution;
            }
            return SurfaceGate::Clear;
        }
    }
    if !intent.tool.is_mutating() {
        return SurfaceGate::Clear;
    }
    let Some(tree) = tree else {
        return SurfaceGate::Uncertain;
    };
    let (execution, other) = super::target::execution_mix(tree);
    if execution && other {
        SurfaceGate::Uncertain
    } else if execution {
        SurfaceGate::Execution
    } else {
        SurfaceGate::Clear
    }
}

fn remote(list: &SurfaceList, window: Option<&PolicyWindow>, tree: Option<&ProcessedTree>) -> bool {
    let mut texts = Vec::new();
    if let Some(window) = window {
        texts.push(window.title.as_str());
        if let Some(url) = window.url.as_deref() {
            texts.push(url);
        }
    }
    let mut owned = Vec::new();
    if let Some(tree) = tree {
        for (_, node) in tree.nodes() {
            if let Some(url) = node.node.url.as_deref() {
                owned.push(url.to_string());
            }
        }
    }
    let mut borrowed: Vec<&str> = texts;
    for url in &owned {
        borrowed.push(url);
    }
    list.hit(&borrowed)
}
