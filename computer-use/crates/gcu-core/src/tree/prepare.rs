use super::*;
use crate::model::*;

pub(crate) fn modal(node: &RawNode) -> bool {
    node.states.contains(States::MODAL)
        && (matches!(node.role, Role::Dialog | Role::Sheet)
            || node.native_role.to_ascii_lowercase().contains("alert"))
}
fn terminal(node: &RawNode) -> bool {
    let native = node.native_role.to_ascii_lowercase();
    native.contains("terminal")
        || native.contains("console")
        || node.name.as_deref().is_some_and(|name| {
            name.split(|c: char| !c.is_alphanumeric()).any(|word| {
                word.eq_ignore_ascii_case("terminal") || word.eq_ignore_ascii_case("console")
            })
        })
}
fn valid_region(frame: Option<Rect>) -> bool {
    frame
        .is_some_and(|r| [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) && r.w > 0. && r.h > 0.)
}
fn convert(mut node: RawNode, inherited: NodeFlags, tree: &mut ProcessedTree) -> ProcessedNode {
    let mut flags = inherited;
    if terminal(&node) {
        flags |= NodeFlags::TERMINAL_LIKE;
    }
    if modal(&node) {
        flags |= NodeFlags::MODAL_CONTENT | NodeFlags::MODAL_PATH;
    }
    if node.native_role.to_ascii_lowercase().contains("titlebar")
        || node
            .subrole
            .as_deref()
            .is_some_and(|s| s.to_ascii_lowercase().contains("titlebar"))
    {
        flags |= NodeFlags::TITLE_BAR;
    }
    if node.states.contains(States::PASSWORD) {
        tree.sensitive_regions_reliable &= valid_region(node.frame);
        tree.sensitive_regions.push(SensitiveRegion {
            fingerprint: node.fingerprint,
            frame: node.frame,
        });
        // Destroy secret-bearing payloads before any transform can merge them into labels.
        node.value = Some(NodeValue::Text("•••".into()));
        node.text_extras = None;
        node.description = None;
        node.help = None;
        node.url = None;
    }
    let mut children: Vec<_> = std::mem::take(&mut node.children)
        .into_iter()
        .map(|child| {
            convert(
                child,
                flags
                    & (NodeFlags::TERMINAL_LIKE | NodeFlags::MODAL_CONTENT | NodeFlags::TITLE_BAR),
                tree,
            )
        })
        .collect();
    if node.states.contains(States::FOCUSED)
        || children
            .iter()
            .any(|n| n.flags.contains(NodeFlags::FOCUS_PATH))
    {
        flags |= NodeFlags::FOCUS_PATH;
    }
    if children
        .iter()
        .any(|n| n.flags.contains(NodeFlags::MODAL_PATH))
    {
        flags |= NodeFlags::MODAL_PATH;
    }
    let mut omitted_nodes = 0;
    if node.states.contains(States::PASSWORD) {
        // Some native APIs expose protected text again as presentation descendants.
        // Collect their sensitive metadata first, then discard those payloads too.
        omitted_nodes = children.iter().map(ProcessedNode::accounted_count).sum();
        children.clear();
    }
    ProcessedNode {
        element_index: UNASSIGNED_INDEX,
        node,
        children,
        flags,
        omitted_nodes,
    }
}
pub(crate) fn prepare(raw: RawTree) -> ProcessedTree {
    let mut tree = ProcessedTree {
        sensitive_regions_reliable: raw.incomplete.is_none(),
        incomplete: raw.incomplete,
        ..Default::default()
    };
    tree.root = raw
        .root
        .map(|root| convert(root, NodeFlags::empty(), &mut tree));
    tree
}
