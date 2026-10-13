use super::SnapshotError;
use crate::{
    model::Fingerprint,
    render::{render_text, RenderOpts},
    tree::ProcessedTree,
};
use std::collections::HashMap;

/// One actually delivered node line, with native identity and structural position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedNode {
    /// Native fingerprint (alignment key; index is not identity).
    pub fingerprint: Fingerprint,
    /// Stable window element index.
    pub index: u32,
    /// Parent fingerprint, independent of index assignment.
    pub parent: Option<Fingerprint>,
    /// Sanitized rendered line, including indentation and trailing newline.
    pub line: String,
}
/// Full rendered baseline, including header/focus/truncation/action facts.
/// Stores only rendered text, never a RawNode or secret-bearing native payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedState {
    header: String,
    window_id: String,
    nodes: Vec<RenderedNode>,
    footer: String,
}
impl RenderedState {
    /// Capture the renderer's delivered lines (including its 20 KB transport budget).
    /// Input must come from Pipeline with stable SnapshotStore-assigned indices.
    pub fn from_tree(tree: &ProcessedTree, opts: &RenderOpts) -> Self {
        let text = render_text(tree, opts);
        let by_index: HashMap<_, _> = tree
            .nodes()
            .into_iter()
            .map(|(_, n)| (n.element_index, n))
            .collect();
        let mut state = Self {
            header: String::new(),
            window_id: opts.window.id.escape_default().to_string(),
            nodes: Vec::new(),
            footer: String::new(),
        };
        for (position, line) in text.split_inclusive('\n').enumerate() {
            if position < 2 {
                state.header.push_str(line);
                continue;
            }
            let index = line
                .trim_start()
                .split(' ')
                .next()
                .and_then(|n| n.parse::<u32>().ok());
            if let Some(node) = index.and_then(|i| by_index.get(&i)) {
                state.nodes.push(RenderedNode {
                    fingerprint: node.node.fingerprint,
                    index: node.element_index,
                    parent: tree
                        .element_index
                        .get(&node.element_index)
                        .and_then(|e| e.parent)
                        .and_then(|i| tree.element_index.get(&i))
                        .map(|e| e.fingerprint),
                    line: line.into(),
                });
            } else {
                state.footer.push_str(line);
            }
        }
        state
    }
    /// Reconstruct the whole delivered full state, in tree preorder.
    pub fn text(&self) -> String {
        let mut text = self.header.clone();
        for node in &self.nodes {
            text.push_str(&node.line);
        }
        text.push_str(&self.footer);
        text
    }
    /// Actually rendered nodes in tree order (may be fewer than native index entries).
    pub fn nodes(&self) -> &[RenderedNode] {
        &self.nodes
    }
    pub(crate) fn memory_bytes(&self) -> usize {
        self.header.capacity()
            + self.window_id.capacity()
            + self.footer.capacity()
            + self.nodes.capacity() * std::mem::size_of::<RenderedNode>()
            + self.nodes.iter().map(|n| n.line.capacity()).sum::<usize>()
    }
}

/// Fingerprint-aligned semantic line mutation. The previous line is retained only
/// after redaction; no raw payload or native handle is part of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// New fingerprint, assigned a new never-reused index.
    Added(RenderedNode),
    /// Same fingerprint with changed presentation, parent, or sibling order.
    Modified {
        /// Previously delivered line.
        before: RenderedNode,
        /// New delivered line.
        after: RenderedNode,
    },
    /// Fingerprint no longer rendered.
    Removed(RenderedNode),
}
/// A semantic patch plus full-state fallback when line/structure limits are exceeded.
/// Applying the patch restores preorder and current header/footer facts, including
/// reordered siblings and focus-only changes; alignment never uses numeric indices.
#[derive(Debug, Clone)]
pub struct Diff {
    baseline: RenderedState,
    changes: Vec<Change>,
    unchanged: usize,
    large: bool,
    order: Vec<Fingerprint>,
    header: String,
    window_id: String,
    footer: String,
    full_text: String,
}
impl Diff {
    /// Compare two sanitized rendered observations. More than 60 changed lines or
    /// >30% added/removed/reparented/reordered nodes yields a full-state fallback.
    pub fn between(before: &RenderedState, after: &RenderedState) -> Self {
        let old: HashMap<_, _> = before.nodes.iter().map(|n| (n.fingerprint, n)).collect();
        let new: HashMap<_, _> = after.nodes.iter().map(|n| (n.fingerprint, n)).collect();
        let old_rank: HashMap<_, _> = before
            .nodes
            .iter()
            .filter(|n| new.contains_key(&n.fingerprint))
            .enumerate()
            .map(|(i, n)| (n.fingerprint, i))
            .collect();
        let new_rank: HashMap<_, _> = after
            .nodes
            .iter()
            .filter(|n| old.contains_key(&n.fingerprint))
            .enumerate()
            .map(|(i, n)| (n.fingerprint, i))
            .collect();
        let mut changes = Vec::new();
        let mut unchanged = 0;
        let mut structural = 0;
        for node in &after.nodes {
            match old.get(&node.fingerprint) {
                None => {
                    structural += 1;
                    changes.push(Change::Added(node.clone()));
                }
                Some(previous) => {
                    let moved = previous.parent != node.parent
                        || old_rank.get(&node.fingerprint) != new_rank.get(&node.fingerprint);
                    structural += usize::from(moved);
                    if **previous != *node || moved {
                        changes.push(Change::Modified {
                            before: (**previous).clone(),
                            after: node.clone(),
                        });
                    } else {
                        unchanged += 1;
                    }
                }
            }
        }
        for node in &before.nodes {
            if !new.contains_key(&node.fingerprint) {
                structural += 1;
                changes.push(Change::Removed(node.clone()));
            }
        }
        let denominator = before.nodes.len().max(after.nodes.len());
        let large = changes.len() > 60 || structural * 100 > denominator * 30;
        let mut result = Self {
            baseline: before.clone(),
            changes,
            unchanged,
            large,
            order: after.nodes.iter().map(|n| n.fingerprint).collect(),
            header: after.header.clone(),
            window_id: after.window_id.clone(),
            footer: after.footer.clone(),
            full_text: after.text(),
        };
        if !result.large && result.delta_text().len() > crate::render::text::MAX_TEXT_BYTES {
            result.large = true;
        }
        result
    }
    /// The three semantic change kinds, in new preorder then old removal order.
    pub fn changes(&self) -> &[Change] {
        &self.changes
    }
    /// Number of fingerprint-aligned unchanged node lines.
    pub fn unchanged(&self) -> usize {
        self.unchanged
    }
    /// Whether to deliver `[full state: large change]` plus the whole new state.
    pub fn is_large_change(&self) -> bool {
        self.large
    }
    /// Apply semantic changes to a compatible old rendering; reject stale entries.
    /// This is independent of text display and also works for full-state fallbacks.
    pub fn apply(&self, before: &RenderedState) -> Result<RenderedState, SnapshotError> {
        if before != &self.baseline {
            return Err(SnapshotError::StaleDiff);
        }
        let mut nodes: HashMap<_, _> = before
            .nodes
            .iter()
            .map(|n| (n.fingerprint, n.clone()))
            .collect();
        for change in &self.changes {
            match change {
                Change::Added(node) => {
                    if nodes.insert(node.fingerprint, node.clone()).is_some() {
                        return Err(SnapshotError::StaleDiff);
                    }
                }
                Change::Modified { before, after } => {
                    if nodes.get(&before.fingerprint) != Some(before) {
                        return Err(SnapshotError::StaleDiff);
                    }
                    nodes.insert(after.fingerprint, after.clone());
                }
                Change::Removed(node) => {
                    if nodes.remove(&node.fingerprint).as_ref() != Some(node) {
                        return Err(SnapshotError::StaleDiff);
                    }
                }
            }
        }
        if nodes.len() != self.order.len() {
            return Err(SnapshotError::StaleDiff);
        }
        let nodes = self
            .order
            .iter()
            .map(|f| nodes.remove(f).ok_or(SnapshotError::StaleDiff))
            .collect::<Result<_, _>>()?;
        Ok(RenderedState {
            header: self.header.clone(),
            window_id: self.window_id.clone(),
            nodes,
            footer: self.footer.clone(),
        })
    }
    /// Model text in architecture §4.5 format, with safe header and fresh footer.
    /// Falls back to the full state on large changes or the renderer's byte bound.
    pub fn text(&self) -> String {
        let full = || format!("{}[full state: large change]\n", self.full_text);
        if self.large {
            return full();
        }
        self.delta_text()
    }
    fn delta_text(&self) -> String {
        let mut text = self.header.clone();
        text.push_str(&format!(
            "Changes since last state (window {}):\n",
            self.window_id
        ));
        for change in &self.changes {
            match change {
                Change::Added(node) => text.push_str(&format!("+ {}", node.line)),
                Change::Removed(node) => text.push_str(&format!("- {}", node.line)),
                Change::Modified { before, after } => text.push_str(&format!(
                    "~ {} ← was: {}\n",
                    after.line.trim_end(),
                    before.line.trim_end()
                )),
            }
        }
        text.push_str(&format!(
            "Unchanged: {} nodes. Indices of unchanged nodes are preserved.\n",
            self.unchanged
        ));
        text.push_str(&self.footer);
        text
    }
}
