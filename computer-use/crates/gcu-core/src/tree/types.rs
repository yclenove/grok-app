use crate::model::*;
use std::collections::BTreeMap;

/// Index sentinel used only between transforms; never present after Pipeline::run.
pub const UNASSIGNED_INDEX: u32 = u32::MAX;

/// Native label relation supplied by an AXTitleUIElement/UIA LabeledBy/AT-SPI adapter.
/// Fingerprints refer to the untransformed raw observation, never to rendered names.
#[derive(Debug, Clone, Copy)]
pub struct TitleAssociation {
    /// Control receiving the accessible label.
    pub control: Fingerprint,
    /// Static-text/label node supplying the text.
    pub label: Fingerprint,
}

/// A retained tree node. `node.children` is always empty; use `children` instead.
/// Native handle/fingerprint are preserved for SnapshotStore and ActionRouter.
#[derive(Debug, Clone)]
pub struct ProcessedNode {
    /// Snapshot-local preorder index, assigned by the final transform.
    pub element_index: u32,
    /// Sanitized node properties, including its original native identity.
    pub node: RawNode,
    /// Retained children in native order.
    pub children: Vec<ProcessedNode>,
    /// Core safety and prioritization annotations.
    pub flags: NodeFlags,
    /// Descendants omitted by visibility/budget truncation beneath this node.
    pub omitted_nodes: usize,
}
impl ProcessedNode {
    pub(crate) fn accounted_count(&self) -> usize {
        1 + self.omitted_nodes
            + self
                .children
                .iter()
                .map(Self::accounted_count)
                .sum::<usize>()
    }
    pub(crate) fn interactive(&self) -> bool {
        !self.node.actions.is_empty()
            || self.node.states.intersects(
                States::FOCUSABLE | States::SETTABLE | States::EDITABLE | States::SELECTABLE,
            )
            || matches!(
                self.node.role,
                Role::Button
                    | Role::Link
                    | Role::CheckBox
                    | Role::Radio
                    | Role::Switch
                    | Role::TextField
                    | Role::SearchField
                    | Role::TextArea
                    | Role::ComboBox
                    | Role::PopUpButton
                    | Role::Slider
                    | Role::Stepper
                    | Role::MenuItem
                    | Role::Tab
            )
    }
    pub(crate) fn protected(&self) -> bool {
        !self.flags.is_empty()
            || self
                .node
                .states
                .intersects(States::PASSWORD | States::FOCUSED | States::MODAL)
    }
    pub(crate) fn display_text(&self) -> Option<&str> {
        self.node
            .name
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(match &self.node.value {
                Some(NodeValue::Text(text)) if !text.is_empty() => Some(text.as_str()),
                _ => None,
            })
    }
    pub(crate) fn plain_text(&self) -> bool {
        self.node.role == Role::StaticText
            && !self.interactive()
            && !self.protected()
            && self.children.is_empty()
            && self.omitted_nodes == 0
            && self.node.description.is_none()
            && self.node.help.is_none()
            && self.node.identifier.is_none()
            && self.node.url.is_none()
    }
}

/// Exact retained element lookup; IDs are deterministic for identical raw trees.
/// Cross-snapshot index preservation belongs to SnapshotStore's fingerprint alignment.
#[derive(Debug, Clone)]
pub struct IndexedElement {
    /// Original driver handle; never serialized.
    pub handle: NativeHandle,
    /// Original driver fingerprint for stale checks and diff alignment.
    pub fingerprint: Fingerprint,
    /// Normalized role.
    pub role: Role,
    /// Original physical-pixel frame.
    pub frame: Option<Rect>,
    /// Core annotations used by routing policies.
    pub flags: NodeFlags,
    /// Parent index; None for the window root.
    pub parent: Option<u32>,
}

/// Sensitive location collected from the raw tree before any pruning or budget.
#[derive(Debug, Clone)]
pub struct SensitiveRegion {
    /// Raw node identity, even when absent from the final element index.
    pub fingerprint: Fingerprint,
    /// Window-relative physical pixels. None means redaction cannot locate the field.
    pub frame: Option<Rect>,
}

/// Retained modal popup metadata for observation and routing.
#[derive(Debug, Clone)]
pub struct PopupInfo {
    /// Popup's retained element index.
    pub index: u32,
    /// Dialog, sheet, or Unknown for native alerts.
    pub role: Role,
    /// Window-relative physical-pixel frame.
    pub frame: Option<Rect>,
}

/// Pure pipeline output consumed by renderers, SnapshotStore, and ActionRouter.
/// Values of password nodes are sanitized before transformation. Sensitive locations
/// outlive removed nodes. This type owns no OS resources beyond opaque driver handles.
#[derive(Debug, Clone, Default)]
pub struct ProcessedTree {
    /// Retained accessibility root; None for canvas-only or zero-budget observations.
    pub root: Option<ProcessedNode>,
    /// Deterministic snapshot-local indices and native identity lookup.
    pub element_index: BTreeMap<u32, IndexedElement>,
    /// Total visibility/budget omissions, including when the entire root was omitted.
    pub omitted_nodes: usize,
    /// Retained focused element's index, if available.
    pub focused: Option<u32>,
    /// Retained modal dialogs/sheets/native alerts.
    pub popups: Vec<PopupInfo>,
    /// Raw password regions; collected before pruning, independent of element_index.
    pub sensitive_regions: Vec<SensitiveRegion>,
    /// False if any password region is missing, empty, or non-finite.
    /// Screenshot delivery must fail closed when this is false.
    pub sensitive_regions_reliable: bool,
    /// Terminal-like nodes and descendants, for P0-08 policy gates.
    pub terminal_like_indices: Vec<u32>,
    pub(crate) title_elements: Vec<TitleAssociation>,
}
impl ProcessedTree {
    /// Borrow nodes in preorder with their zero-based nesting depth; no IO or blocking.
    pub fn nodes(&self) -> Vec<(usize, &ProcessedNode)> {
        fn walk<'a>(
            node: &'a ProcessedNode,
            depth: usize,
            out: &mut Vec<(usize, &'a ProcessedNode)>,
        ) {
            out.push((depth, node));
            for child in &node.children {
                walk(child, depth + 1, out);
            }
        }
        let mut nodes = Vec::new();
        if let Some(root) = &self.root {
            walk(root, 0, &mut nodes);
        }
        nodes
    }
}
