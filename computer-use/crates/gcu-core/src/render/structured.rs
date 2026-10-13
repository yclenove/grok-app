use super::{format, limit::field, text, RenderOpts};
use crate::tree::ProcessedTree;
use serde_json::{json, Value};

/// Render compact architecture §4.3 facts. Full detail is opt-in, and intentionally
/// excludes *all* node values/text extras (including password values). No raw nodes,
/// native handles, or secret-bearing node debug data escape. Action results supplied
/// by the router must already be redacted, as must screenshot references.
/// Pure and synchronous; no IO, waiting, or cancellation points.
pub fn render_structured(tree: &ProcessedTree, opts: &RenderOpts) -> Value {
    let omitted = text::layout(tree, opts).omitted;
    let mut result = json!({
        "app": { "id": opts.app.id, "name": field(&opts.app.name, opts.text_limit), "pid": opts.app.pid },
        "window": {"id": opts.window.id, "title": field(&opts.window.title, opts.text_limit),
            "frame": opts.window.frame, "scale": opts.window.scale},
        "snapshot_id": opts.snapshot_id.as_deref().unwrap_or_default(),
        "screenshot": opts.screenshot,
        "focused": tree.focused,
        "truncated": (omitted > 0).then(|| json!({"omitted_nodes": omitted})),
        "action": opts.action,
    });
    if opts.full_detail {
        result["elements"] = Value::Array(tree.nodes().into_iter().map(|(_, node)| {
            json!({"index": node.element_index, "role": node.node.role,
                "name": node.node.name.as_deref().map(|s| field(s, opts.text_limit)),
                "states": format::states(&node.node),
                "actions": node.node.actions.iter().map(format::action).map(|s| field(&s, opts.text_limit)).collect::<Vec<_>>(),
                "frame": node.node.frame})
        }).collect());
    }
    result
}
