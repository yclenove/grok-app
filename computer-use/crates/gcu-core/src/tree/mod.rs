//! Owned, deterministic data flow: model::RawTree → Pipeline → ProcessedTree.
//! Every transform is a synchronous pure function; no IO, waits, or cancellation
//! points. Driver reads have their own deadlines. Consumers may cancel between reads.

mod budget;
mod index;
mod prepare;
#[cfg(test)]
pub(crate) mod test_support;
/// Independent pure transforms in the fixed architecture §4.1 order.
pub mod transforms;
mod types;
pub use crate::driver::TreeOpts;
pub use crate::model::RawTree;
pub use types::*;

/// Uniform owned transform signature. Inputs are unchanged unless moved by the caller.
pub type Transform = fn(ProcessedTree, &TreeOpts) -> ProcessedTree;

/// Fixed 13-step architecture order; the only pipeline sequencing table.
pub const TRANSFORMS: [(&str, Transform); 13] = [
    ("prune_offscreen", transforms::prune_offscreen::transform),
    ("filter_actions", transforms::filter_actions::transform),
    (
        "associate_title_elements",
        transforms::associate_title_elements::transform,
    ),
    (
        "prune_empty_disabled",
        transforms::prune_empty_disabled::transform,
    ),
    (
        "prune_non_descriptive",
        transforms::prune_non_descriptive::transform,
    ),
    (
        "flatten_into_selectable_ancestor",
        transforms::flatten_into_selectable_ancestor::transform,
    ),
    (
        "merge_single_item_groups",
        transforms::merge_single_item_groups::transform,
    ),
    (
        "flatten_redundant_hierarchy",
        transforms::flatten_redundant_hierarchy::transform,
    ),
    (
        "merge_text_only_siblings",
        transforms::merge_text_only_siblings::transform,
    ),
    (
        "flatten_repetitive_static_text",
        transforms::flatten_repetitive_static_text::transform,
    ),
    (
        "flatten_links_into_markdown",
        transforms::flatten_links_into_markdown::transform,
    ),
    ("budget", transforms::budget::transform),
    ("assign_indices", transforms::assign_indices::transform),
];

/// Tree processor with optional native title relations; stores no snapshots or indices.
#[derive(Debug, Clone, Default)]
pub struct Pipeline {
    title_elements: Vec<TitleAssociation>,
}
impl Pipeline {
    /// Supply native label relations for this observation. No geometric/name guessing.
    /// Adapters can pass native relations without changing Driver/RawNode interfaces.
    pub fn with_title_elements(title_elements: Vec<TitleAssociation>) -> Self {
        Self { title_elements }
    }
    /// Collect raw sensitive regions, sanitize passwords, then execute all 13 steps.
    /// Synchronous and deterministic; no IO, blocking native calls, or hidden retries.
    pub fn run(&self, raw: RawTree, opts: &TreeOpts) -> ProcessedTree {
        let mut tree = prepare::prepare(raw);
        tree.title_elements.clone_from(&self.title_elements);
        for (_, transform) in TRANSFORMS {
            tree = transform(tree, opts);
        }
        tree
    }
}

pub(crate) fn post_order(tree: &mut ProcessedTree, apply: &impl Fn(&mut ProcessedNode)) {
    fn walk(node: &mut ProcessedNode, apply: &impl Fn(&mut ProcessedNode)) {
        for child in &mut node.children {
            walk(child, apply);
        }
        apply(node);
    }
    if let Some(root) = &mut tree.root {
        walk(root, apply);
    }
}

pub(crate) use budget::priority as render_priority;
