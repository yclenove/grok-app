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

/// Unique popup in the band between radios and the slider.
///
/// egui's combo has no accessibility name. One popup is used when that band
/// cannot be measured. Several popups in the band is ambiguous.
pub fn combo_popup(root: &RawNode) -> Option<&RawNode> {
    let popups = collect(root, &|node| node.role == Role::PopUpButton);
    let Some((low, high)) = combo_band(root) else {
        return (popups.len() == 1).then_some(popups[0]);
    };
    let hits: Vec<_> = popups
        .into_iter()
        .filter(|node| center_y(node).is_some_and(|y| y > low && y < high))
        .collect();
    (hits.len() == 1).then_some(hits[0])
}

/// Point on the middle static-text row beside a press-only scrollbar.
///
/// The virtual list is not an `AXList`. Rows overlap the bar's vertical span
/// and end at its right edge. Two such columns is ambiguous.
pub fn virtual_list_point(root: &RawNode) -> Option<WindowPoint> {
    let mut hits = Vec::new();
    for bar in collect(root, &|node| node.role == Role::ScrollBar) {
        let Some(bar_frame) = bar.frame else {
            continue;
        };
        let mut rows: Vec<Rect> = collect(root, &|node| {
            node.role == Role::StaticText
                && node.frame.is_some_and(|frame| row_on_bar(frame, bar_frame))
        })
        .iter()
        .filter_map(|node| node.frame)
        .collect();
        if rows.len() < 3 {
            continue;
        }
        rows.sort_by(|left, right| left.y.total_cmp(&right.y));
        let mid = rows[rows.len() / 2];
        hits.push(WindowPoint {
            x: mid.x + mid.w / 2.0,
            y: mid.y + mid.h / 2.0,
        });
    }
    (hits.len() == 1).then_some(hits[0])
}

fn combo_band(root: &RawNode) -> Option<(f64, f64)> {
    let radios = collect(root, &|node| {
        node.role == Role::Radio && node.frame.is_some()
    });
    let sliders = collect(root, &|node| {
        node.role == Role::Slider && node.frame.is_some()
    });
    if radios.is_empty() || sliders.is_empty() {
        return None;
    }
    let radio_bottom = radios
        .iter()
        .filter_map(|node| node.frame)
        .map(|frame| frame.y + frame.h)
        .fold(f64::MIN, f64::max);
    let slider_top = sliders
        .iter()
        .filter_map(|node| node.frame)
        .map(|frame| frame.y)
        .fold(f64::MAX, f64::min);
    Some((radio_bottom - 4.0, slider_top + 4.0))
}

fn center_y(node: &RawNode) -> Option<f64> {
    node.frame.map(|frame| frame.y + frame.h / 2.0)
}

fn row_on_bar(row: Rect, bar: Rect) -> bool {
    row.w >= 40.0
        && row.x < bar.x
        && row.x + row.w <= bar.x + bar.w + 8.0
        && row.y < bar.y + bar.h
        && row.y + row.h > bar.y
}

fn collect<'a>(node: &'a RawNode, pred: &impl Fn(&RawNode) -> bool) -> Vec<&'a RawNode> {
    let mut out = Vec::new();
    fn walk<'a>(node: &'a RawNode, pred: &impl Fn(&RawNode) -> bool, out: &mut Vec<&'a RawNode>) {
        if pred(node) {
            out.push(node);
        }
        for child in &node.children {
            walk(child, pred, out);
        }
    }
    walk(node, pred, &mut out);
    out
}
