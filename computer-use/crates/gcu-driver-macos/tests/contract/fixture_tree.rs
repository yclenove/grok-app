//! Exact fixture selectors; navigation labels must never stand in for controls.

use gcu_core::model::{RawNode, RawTree, Rect, Role, WindowPoint};

pub const READY_CONTROLS: &[&str] = &[
    "counter_button",
    "name_field",
    "agree_checkbox",
    "slider_field",
    "grid_canvas",
    "open_modal_button",
    "drag_source",
    "drop_target",
];

pub fn find<'a>(node: &'a RawNode, pred: &impl Fn(&RawNode) -> bool) -> Option<&'a RawNode> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|child| find(child, pred))
}

pub fn exact(node: &RawNode, id: &str) -> bool {
    node.identifier.as_deref() == Some(id) || node.name.as_deref() == Some(id)
}

pub fn missing(raw: &RawTree) -> Vec<&'static str> {
    READY_CONTROLS
        .iter()
        .copied()
        .filter(|id| {
            raw.root
                .as_ref()
                .is_none_or(|root| find(root, &|n| exact(n, id)).is_none())
        })
        .collect()
}

pub fn count(raw: &RawTree) -> usize {
    fn nodes(node: &RawNode) -> usize {
        1 + node.children.iter().map(nodes).sum::<usize>()
    }
    raw.root.as_ref().map_or(0, nodes)
}

pub fn canvas(root: &RawNode) -> Option<&RawNode> {
    find(root, &|node| {
        exact(node, "grid_canvas") && matches!(node.role, Role::Image | Role::Canvas)
    })
}

/// Pick an actual screenshot pixel of a specific painted cell inside the
/// canvas AX frame. Its allocated width can exceed the painted grid width.
pub fn cell_pixel(
    rgba: &[u8],
    width: u32,
    height: u32,
    canvas: Rect,
    cell: [u32; 2],
) -> Result<WindowPoint, String> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) * 4 != rgba.len() as u64 {
        return Err("grid screenshot must contain full RGBA pixels".into());
    }
    let [col, row] = cell;
    if col >= 8 || row >= 6 {
        return Err("grid cell out of bounds".into());
    }
    let color = [
        ((col * 31 + 45) % 180 + 50) as u8,
        ((row * 41 + 65) % 180 + 50) as u8,
        (((col + row) * 29 + 85) % 180 + 50) as u8,
    ];
    let hits: Vec<_> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(i, px)| {
            let at = WindowPoint {
                x: (i % width as usize) as f64 + 0.5,
                y: (i / width as usize) as f64 + 0.5,
            };
            (px[..3] == color
                && at.x >= canvas.x
                && at.x < canvas.x + canvas.w
                && at.y >= canvas.y
                && at.y < canvas.y + canvas.h)
                .then_some(at)
        })
        .collect();
    hits.get(hits.len() / 2).copied().ok_or_else(|| {
        format!("screenshot missing grid cell {cell:?} color {color:?} inside canvas {canvas:?}")
    })
}
