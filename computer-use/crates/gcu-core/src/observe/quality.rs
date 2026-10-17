use crate::{
    model::{RawNode, RawTree, Role, States},
    tree::{ProcessedNode, ProcessedTree},
};

/// Evaluation-supplied boundaries. None means not calibrated, never a guessed number.
/// Construction and assessment are pure CPU work with no waits or cancellation.
#[derive(Debug, Clone, Copy, Default)]
pub struct QualityThresholds {
    /// Minimum useful interactive nodes, scoped to popups when present.
    /// Unset keeps auto observations conservative until evaluation supplies a value.
    pub min_interactive_nodes: Option<usize>,
    /// Raw canvas nodes / raw nodes at which a window is canvas-like, in (0, 1].
    /// Unset or invalid conservatively treats any canvas node as canvas-like.
    pub canvas_role_ratio: Option<f64>,
}

/// Payload-free tree quality facts; never a certificate that capture is safe.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TreeQuality {
    /// Native tree has no root (canvas-only windows may still have safe captures).
    pub root_absent: bool,
    /// Raw node count before pruning/budget, including the window root.
    pub raw_nodes: usize,
    /// Raw canvas roles, including those removed from the model's text tree.
    pub canvas_nodes: usize,
    /// Useful retained interactive nodes, excluding disabled/offscreen nodes.
    pub interactive_nodes: usize,
    /// Retained dialog/sheet/popover/menu/native-alert roots.
    pub popup_count: usize,
    /// Interactive nodes inside retained popups, excluding background controls.
    pub popup_interactive_nodes: usize,
    /// Core removed nodes or native enumeration was incomplete.
    pub truncated: bool,
}
impl TreeQuality {
    /// Measure raw visual roles and the actual retained tree's usable controls.
    /// Uses no text content, OS work, blocking, timeout or cancellation.
    pub fn assess(raw: &RawTree, tree: &ProcessedTree) -> Self {
        fn native(node: &RawNode, quality: &mut TreeQuality) {
            quality.raw_nodes += 1;
            quality.canvas_nodes += usize::from(node.role == Role::Canvas);
            for child in &node.children {
                native(child, quality);
            }
        }
        fn retained(node: &ProcessedNode, in_popup: bool, quality: &mut TreeQuality) {
            let popup = matches!(
                node.node.role,
                Role::Dialog | Role::Sheet | Role::Popover | Role::Menu
            ) || (node.node.states.contains(States::MODAL)
                && node.node.native_role.to_ascii_lowercase().contains("alert"));
            quality.popup_count += usize::from(popup);
            let in_popup = in_popup || popup;
            let interactive = node.interactive()
                && node.node.states.contains(States::ENABLED)
                && !node.node.states.contains(States::OFFSCREEN);
            quality.interactive_nodes += usize::from(interactive);
            quality.popup_interactive_nodes += usize::from(interactive && in_popup);
            for child in &node.children {
                retained(child, in_popup, quality);
            }
        }
        let mut quality = Self {
            root_absent: raw.root.is_none(),
            truncated: tree.omitted_nodes > 0 || raw.incomplete.is_some(),
            ..Self::default()
        };
        if let Some(root) = &raw.root {
            native(root, &mut quality);
        }
        if let Some(root) = &tree.root {
            retained(root, false, &mut quality);
        }
        quality
    }

    /// Raw canvas role share, or zero when no native root exists. Pure, nonblocking.
    pub fn canvas_ratio(&self) -> f64 {
        if self.raw_nodes == 0 {
            0.
        } else {
            self.canvas_nodes as f64 / self.raw_nodes as f64
        }
    }

    pub(crate) fn canvas_like(&self, thresholds: &QualityThresholds) -> bool {
        self.root_absent
            || (self.canvas_nodes > 0
                && match thresholds.canvas_role_ratio {
                    Some(ratio) if ratio.is_finite() && ratio > 0. && ratio <= 1. => {
                        self.canvas_ratio() >= ratio
                    }
                    _ => true,
                })
    }

    pub(crate) fn useful_controls(&self) -> usize {
        if self.popup_count > 0 {
            self.popup_interactive_nodes
        } else {
            self.interactive_nodes
        }
    }
}
