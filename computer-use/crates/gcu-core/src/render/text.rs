use super::{format, limit::field, RenderOpts};
use crate::{
    model::*,
    tree::{ProcessedNode, ProcessedTree},
};
use std::collections::{BTreeMap, BTreeSet};

/// Maximum tool text in bytes; the client retains only its first 20,000 bytes.
pub const MAX_TEXT_BYTES: usize = 20_000;
/// Keep focus/truncation facts inside the first 19 KB, leaving room for compact facts.
pub const FACTS_PREFIX_BYTES: usize = 19_000;

pub(crate) struct Layout {
    pub text: String,
    pub omitted: usize,
}
fn header(opts: &RenderOpts, text_limit: Option<usize>) -> String {
    let pid = opts
        .app
        .pid
        .map_or_else(|| "unknown".into(), |pid| pid.to_string());
    format!(
        "App={} (pid {}) — {}\nWindow: \"{}\", id {}, {}×{} @{}x, App: {}.\n",
        field(&opts.app.id, text_limit),
        pid,
        field(&opts.app.name, text_limit),
        field(&opts.window.title, text_limit),
        field(&opts.window.id, text_limit),
        opts.window.frame.w,
        opts.window.frame.h,
        opts.window.scale,
        field(&opts.app.name, text_limit)
    )
}
fn line(depth: usize, node: &ProcessedNode, text_limit: Option<usize>, compact: bool) -> String {
    let raw = &node.node;
    let mut text = format!(
        "{}{} {}",
        "    ".repeat(depth.min(63)),
        node.element_index,
        format::role(raw.role)
    );
    let states: Vec<_> = format::states(raw)
        .into_iter()
        .filter(|state| {
            if compact {
                matches!(*state, "focused" | "settable" | "disabled")
            } else {
                matches!(
                    *state,
                    "selected"
                        | "selectable"
                        | "expanded"
                        | "settable"
                        | "focused"
                        | "disabled"
                        | "checked"
                        | "mixed"
                        | "modal"
                        | "busy"
                        | "required"
                )
            }
        })
        .collect();
    if !states.is_empty() {
        text.push_str(&format!(" ({})", states.join(", ")));
    }
    if let Some(name) = &raw.name {
        if !name.is_empty() {
            text.push(' ');
            text.push_str(&field(name, text_limit));
        }
    }
    let password = raw.states.contains(States::PASSWORD);
    if password {
        text.push_str(" Value: •••");
    } else if !compact || raw.states.contains(States::FOCUSED) {
        // An empty value carries no information for the model; omit the field.
        let shown = if let Some(value) = &raw.value {
            Some(field(&format::value(value), text_limit))
        } else {
            raw.text_extras
                .as_ref()
                .and_then(|extras| extras.text.as_ref())
                .map(|value| field(value, text_limit))
        };
        if let Some(shown) = shown.filter(|shown| !shown.is_empty()) {
            text.push_str(&format!(" Value: {shown}"));
        }
    }
    if !compact {
        for (name, value) in [
            ("Description", &raw.description),
            ("Placeholder", &raw.placeholder),
            ("Help", &raw.help),
            ("ID", &raw.identifier),
        ] {
            if let Some(value) = value {
                text.push_str(&format!(" {name}: {}", field(value, text_limit)));
            }
        }
        let actions = raw
            .actions
            .iter()
            .filter(|a| **a != NodeAction::Press)
            .map(format::action)
            .collect::<Vec<_>>()
            .join(", ");
        if !actions.is_empty() {
            text.push_str(&format!(
                " Secondary Actions: {}",
                field(&actions, text_limit)
            ));
        }
        if let Some(url) = &raw.url {
            text.push_str(&format!(" URL: {}", field(url, text_limit)));
        }
    }
    if node.flags.contains(NodeFlags::SUMMARY) {
        text.push_str(" [background summary]");
    }
    if node.flags.contains(NodeFlags::TERMINAL_LIKE) {
        text.push_str(" [terminal]");
    }
    text.push('\n');
    text
}
fn footer(tree: &ProcessedTree, opts: &RenderOpts, selected: &BTreeSet<u32>) -> String {
    let mut text = String::new();
    let root_index = tree.root.as_ref().map_or(0, |n| n.element_index);
    if let Some(index) = tree.focused {
        if let Some(element) = tree.element_index.get(&index) {
            text.push_str(&format!(
                "The focused UI element is {index} {}.\n",
                format::role(element.role)
            ));
        }
    }
    fn walk(
        node: &ProcessedNode,
        selected: &BTreeSet<u32>,
        parent: (u32, Role),
        omissions: &mut BTreeMap<u32, (Role, usize)>,
    ) {
        let retained = selected.contains(&node.element_index);
        let under = if retained {
            (node.element_index, node.node.role)
        } else {
            parent
        };
        let omitted = node.omitted_nodes + usize::from(!retained);
        if omitted > 0 {
            omissions.entry(under.0).or_insert((under.1, 0)).1 += omitted;
        }
        for child in &node.children {
            walk(child, selected, under, omissions);
        }
    }
    let mut omissions = BTreeMap::new();
    if let Some(root) = &tree.root {
        walk(root, selected, (root_index, Role::Window), &mut omissions);
    } else if tree.omitted_nodes > 0 {
        omissions.insert(0, (Role::Window, tree.omitted_nodes));
    }
    let mut details = String::new();
    for (index, (role, count)) in &omissions {
        details.push_str(&format!(
            "[truncated: {count} nodes omitted under {index} {}]\n",
            format::role(*role)
        ));
    }
    if details.len() > 4096 {
        let count: usize = omissions.values().map(|(_, count)| count).sum();
        text.push_str(&format!(
            "[truncated: {count} nodes omitted under {root_index} standard window]\n"
        ));
    } else {
        text.push_str(&details);
    }
    if let Some(fact) = tree.incomplete {
        let reason = match fact.reason {
            IncompleteReason::Deadline => "deadline",
            IncompleteReason::NodeCap => "node_cap",
            IncompleteReason::PartialError => "partial_error",
            IncompleteReason::DepthCap => "depth_cap",
        };
        let unread = fact
            .unread_nodes
            .map_or_else(|| "unknown".into(), |n| n.to_string());
        text.push_str(&format!(
            "[truncated: native tree incomplete ({reason}); {} nodes read; {unread} nodes unread]\n",
            fact.read_nodes
        ));
    }
    if let Some(action) = &opts.action {
        // Do not echo payload-bearing target labels, notes, or verification strings.
        text.push_str(&format!(
            "Action: {:?}; delivery: {:?}; effect: {:?}.\n",
            action.outcome, action.delivery, action.effect
        ));
    }
    text
}
fn assemble(
    header: &str,
    lines: &BTreeMap<u32, String>,
    selected: &BTreeSet<u32>,
    order: &[u32],
    footer: &str,
    note: &str,
) -> String {
    let mut text = header.to_owned();
    for index in order {
        if let Some(line) = lines.get(index).filter(|_| selected.contains(index)) {
            text.push_str(line);
        }
    }
    text.push_str(footer);
    text.push_str(note);
    text
}
// SnapshotStore exports this only after sanitizing secret-bearing fields. The
// complete rendering is for a session-private overflow file, never inline MCP.
pub(crate) fn file_text(tree: &ProcessedTree, opts: &RenderOpts) -> String {
    let nodes = tree.nodes();
    let order = nodes
        .iter()
        .map(|(_, n)| n.element_index)
        .collect::<Vec<_>>();
    let all = nodes.iter().map(|(_, n)| n.element_index).collect();
    let lines = nodes
        .iter()
        .map(|(d, n)| (n.element_index, line(*d, n, opts.text_limit, false)))
        .collect();
    assemble(
        &header(opts, opts.text_limit),
        &lines,
        &all,
        &order,
        &footer(tree, opts, &all),
        "",
    )
}
pub(crate) fn layout(tree: &ProcessedTree, opts: &RenderOpts) -> Layout {
    let nodes = tree.nodes();
    // Stable historical indices are not preorder positions. Render native order.
    let order: Vec<_> = nodes.iter().map(|(_, n)| n.element_index).collect();
    let all: BTreeSet<_> = nodes.iter().map(|(_, n)| n.element_index).collect();
    let first = file_text(tree, opts);
    if first.len() <= FACTS_PREFIX_BYTES {
        return Layout {
            text: first,
            omitted: tree.omitted_nodes,
        };
    }

    // Shorten fields before removing any low-priority nodes. `max` is subject to
    // this independent transport cap, and any contraction is disclosed at the end.
    let short = Some(opts.text_limit.unwrap_or(64).min(64));
    let mut lines: BTreeMap<_, _> = nodes
        .iter()
        .map(|(d, n)| (n.element_index, line(*d, n, short, false)))
        .collect();
    let head = header(opts, short);
    let note = "[text output limited to 20 KB; fields shortened and low-priority nodes omitted as needed]\n";
    let second = assemble(&head, &lines, &all, &order, &footer(tree, opts, &all), note);
    if second.len() <= FACTS_PREFIX_BYTES {
        return Layout {
            text: second,
            omitted: tree.omitted_nodes,
        };
    }

    let mut selected: BTreeSet<_> = nodes
        .iter()
        .filter(|(d, _)| *d == 0)
        .map(|(_, n)| n.element_index)
        .collect();
    let mut focus = tree.focused;
    while let Some(index) = focus {
        selected.insert(index);
        focus = tree
            .element_index
            .get(&index)
            .and_then(|entry| entry.parent);
    }
    if assemble(
        &head,
        &lines,
        &selected,
        &order,
        &footer(tree, opts, &selected),
        note,
    )
    .len()
        > FACTS_PREFIX_BYTES
    {
        lines = nodes
            .iter()
            .map(|(d, n)| (n.element_index, line(*d, n, Some(8), true)))
            .collect();
    }
    // A caller may request more than the default 64 levels, or a driver may expose
    // multiple focused nodes. Summarize excess ancestor lines while keeping the
    // actual focus and root, so even unusual observations respect the transport cap.
    let root_index = tree.root.as_ref().map_or(0, |n| n.element_index);
    let mut ancestors: Vec<_> = selected
        .iter()
        .copied()
        .filter(|index| Some(*index) != tree.focused && *index != root_index)
        .collect();
    while assemble(
        &head,
        &lines,
        &selected,
        &order,
        &footer(tree, opts, &selected),
        note,
    )
    .len()
        + 4096
        > FACTS_PREFIX_BYTES
    {
        if let Some(index) = ancestors.pop() {
            selected.remove(&index);
        } else {
            break;
        }
    }
    let mut candidates: Vec<_> = nodes
        .iter()
        .map(|(depth, node)| {
            (
                crate::tree::render_priority(node, *depth),
                *depth,
                node.element_index,
            )
        })
        .collect();
    candidates.sort_unstable();
    // Reserve the maximum omission-summary space and all focus/action/footer facts.
    // Actual text is composed only once after selecting nodes, keeping rendering linear.
    let reserve = head.len() + footer(tree, opts, &selected).len() + 4096 + note.len();
    let mut used = reserve
        + selected
            .iter()
            .filter_map(|i| lines.get(i))
            .map(String::len)
            .sum::<usize>();
    for (_, _, index) in candidates {
        let mut path = Vec::new();
        let mut current = Some(index);
        while let Some(id) = current {
            if selected.contains(&id) {
                break;
            }
            path.push(id);
            current = tree.element_index.get(&id).and_then(|n| n.parent);
        }
        let cost: usize = path
            .iter()
            .filter_map(|i| lines.get(i))
            .map(String::len)
            .sum();
        if used + cost <= FACTS_PREFIX_BYTES {
            used += cost;
            selected.extend(path);
        }
    }
    let text = assemble(
        &head,
        &lines,
        &selected,
        &order,
        &footer(tree, opts, &selected),
        note,
    );
    Layout {
        text,
        omitted: tree.omitted_nodes + all.len() - selected.len(),
    }
}

/// Render architecture §4.2 text, including focus and truncation facts in the first
/// 19,000 bytes. Field limits count Unicode scalars; the fixed byte cap also applies
/// to `text_limit=None` (protocol `"max"`). Pure and synchronous; no IO or waits.
pub fn render_text(tree: &ProcessedTree, opts: &RenderOpts) -> String {
    layout(tree, opts).text
}
