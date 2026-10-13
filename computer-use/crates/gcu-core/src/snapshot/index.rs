use super::SnapshotError;
use crate::{
    model::Fingerprint,
    tree::{IndexedElement, ProcessedNode, ProcessedTree, UNASSIGNED_INDEX},
};
use std::collections::{BTreeMap, HashMap};

/// Snapshot-owned fingerprint→index and index→native-element lookups.
/// Indices are assigned by the window's persistent high-water counter, never reused.
#[derive(Debug, Clone, Default)]
pub struct ElementIndex {
    fingerprints: HashMap<Fingerprint, u32>,
    elements: BTreeMap<u32, IndexedElement>,
}
impl ElementIndex {
    /// Exact fingerprint lookup; no label/similarity matching or hidden retries.
    pub fn index_of(&self, fingerprint: Fingerprint) -> Option<u32> {
        self.fingerprints.get(&fingerprint).copied()
    }
    /// Exact native lookup, for ActionRouter's pre-execution identity revalidation.
    pub fn get(&self, index: u32) -> Option<&IndexedElement> {
        self.elements.get(&index)
    }
    /// All retained element entries, including nodes omitted only by text budgeting.
    pub fn elements(&self) -> &BTreeMap<u32, IndexedElement> {
        &self.elements
    }
    /// Number of retained native elements.
    pub fn len(&self) -> usize {
        self.elements.len()
    }
    /// Whether the observation is canvas-only or has zero retained nodes.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
    pub(crate) fn memory_bytes(&self) -> usize {
        self.elements.len() * (std::mem::size_of::<(u32, IndexedElement)>() + 48)
            + self.fingerprints.capacity() * (std::mem::size_of::<(Fingerprint, u32)>() + 16)
    }
    pub(crate) fn assign(
        tree: &mut ProcessedTree,
        previous: Option<&Self>,
        next: u64,
    ) -> Result<(Self, u64), SnapshotError> {
        let mut index = Self::default();
        let mut next = next;
        let mut remap = HashMap::new();
        for (_, node) in tree.nodes() {
            let fingerprint = node.node.fingerprint;
            if index.fingerprints.contains_key(&fingerprint) {
                return Err(SnapshotError::DuplicateFingerprint);
            }
            let assigned = if let Some(old) = previous.and_then(|p| p.index_of(fingerprint)) {
                old
            } else {
                if next >= u64::from(UNASSIGNED_INDEX) {
                    return Err(SnapshotError::IndexExhausted);
                }
                let allocated = next as u32;
                next += 1;
                allocated
            };
            index.fingerprints.insert(fingerprint, assigned);
            remap.insert(node.element_index, assigned);
        }
        fn walk(node: &mut ProcessedNode, parent: Option<u32>, index: &mut ElementIndex) {
            let assigned = index.fingerprints[&node.node.fingerprint];
            node.element_index = assigned;
            index.elements.insert(
                assigned,
                IndexedElement {
                    handle: node.node.handle.clone(),
                    fingerprint: node.node.fingerprint,
                    role: node.node.role,
                    frame: node.node.frame,
                    flags: node.flags,
                    parent,
                },
            );
            for child in &mut node.children {
                walk(child, Some(assigned), index);
            }
        }
        if let Some(root) = &mut tree.root {
            walk(root, None, &mut index);
        }
        tree.focused = tree.focused.and_then(|i| remap.get(&i).copied());
        for popup in &mut tree.popups {
            if let Some(i) = remap.get(&popup.index) {
                popup.index = *i;
            }
        }
        tree.terminal_like_indices = tree
            .terminal_like_indices
            .iter()
            .filter_map(|i| remap.get(i).copied())
            .collect();
        tree.element_index = index.elements.clone();
        Ok((index, next))
    }
}
