//! Pure AX tree walk. No macOS calls.
//!
//! The walk stops at [`TREE_BUILD_TIMEOUT`] and [`MAX_NODES_CEILING`]. A zero
//! node budget returns an empty tree. Cancellation is checked before every
//! source call. Reads report `maybe_executed = false`. A native failure on one
//! field becomes an empty field. `PermissionMissing`, `Cancelled`, `Timeout`,
//! and `Crashed` stop the walk. After the root is read, deadline expiry preserves
//! retained nodes and reports incompleteness; before that it returns `Timeout`.
//!
//! Frames are AX points times `scale`, then shifted so the window origin is
//! `(0, 0)`. The Y axis is not flipped. Text is cut to `text_limit` and to
//! [`RENDER_BYTE_CAP`] bytes. A cut value ends with `…(+N chars)`.

use std::time::Duration;

use gcu_core::{
    driver::{Cancel, DriverError, TreeOpts},
    fingerprint,
    model::{
        Fingerprint, Incomplete, IncompleteReason, NodeAction, NodeValue, RawNode, RawTree, Rect,
        Role, States, TextExtras,
    },
};
use smallvec::SmallVec;
use smol_str::SmolStr;

use super::{
    roles::map_ax_role,
    sensitive::is_terminal_like,
    source::{AxSource, AxStateBits},
};
use gcu_core::driver::Result;

/// Hard cap for one tree walk. The caller's timeout can only be shorter.
pub const TREE_BUILD_TIMEOUT: Duration = Duration::from_secs(2);
/// Hard cap on visited nodes, including the root.
pub const MAX_NODES_CEILING: usize = 5000;
/// Same byte budget the text renderer keeps for one observation.
pub const RENDER_BYTE_CAP: usize = 20 * 1024;

const PASSWORD_PLACEHOLDER: &str = "•••";

/// Build a [`RawTree`] from any [`AxSource`].
///
/// `scale` converts AX points into physical pixels. Non-finite or non-positive
/// values become `1.0`. Blocking native work, if any, stays inside the source.
/// This function itself only awaits those calls and applies the budgets above.
pub async fn build_raw_tree<S: AxSource>(
    source: &S,
    root: &S::Node,
    opts: &TreeOpts,
    cancel: &Cancel,
    scale: f64,
) -> Result<RawTree> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    if opts.max_tree_nodes == 0 {
        return Ok(RawTree {
            root: None,
            incomplete: Some(Incomplete {
                reason: IncompleteReason::NodeCap,
                read_nodes: 0,
                unread_nodes: None,
            }),
        });
    }
    let deadline = tokio::time::Instant::now() + opts.timeout.min(TREE_BUILD_TIMEOUT);
    let mut walk = Walk {
        source,
        opts,
        cancel,
        origin_x: 0.0,
        origin_y: 0.0,
        scale: normalized_scale(scale),
        max_nodes: opts.max_tree_nodes.min(MAX_NODES_CEILING),
        node_count: 0,
        deadline,
        reason: None,
    };
    let root_frame = tokio::time::timeout_at(deadline, with_cancel(cancel, source.frame(root)))
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?;
    if let Some(Some(frame)) = soften(root_frame, &mut walk.reason)? {
        walk.origin_x = frame.x;
        walk.origin_y = frame.y;
    }
    let mut root_node = walk.node(root, 0).await?;
    fingerprint::refresh_tree(&mut root_node, Fingerprint(0), 0);
    Ok(RawTree {
        root: Some(root_node),
        incomplete: walk.reason.map(|reason| Incomplete {
            reason,
            read_nodes: walk.node_count as u32,
            unread_nodes: None,
        }),
    })
}

struct Walk<'a, S> {
    source: &'a S,
    opts: &'a TreeOpts,
    cancel: &'a Cancel,
    origin_x: f64,
    origin_y: f64,
    scale: f64,
    max_nodes: usize,
    node_count: usize,
    deadline: tokio::time::Instant,
    reason: Option<IncompleteReason>,
}

impl<S: AxSource> Walk<'_, S> {
    async fn node(&mut self, node: &S::Node, depth: usize) -> Result<RawNode> {
        let cancel = self.cancel;
        let deadline = self.deadline;
        let (mut raw, children) = with_cancel(cancel, async {
            if tokio::time::Instant::now() >= deadline {
                return Err(DriverError::Timeout {
                    maybe_executed: false,
                });
            }
            tokio::time::timeout_at(deadline, self.read_node(node, depth))
                .await
                .map_err(|_| DriverError::Timeout {
                    maybe_executed: false,
                })?
        })
        .await?;
        self.node_count += 1;
        for child in children {
            if self.node_count >= self.max_nodes {
                self.reason.get_or_insert(IncompleteReason::NodeCap);
                break;
            }
            if depth + 1 >= self.opts.max_tree_depth {
                self.reason.get_or_insert(IncompleteReason::DepthCap);
                break;
            }
            match Box::pin(self.node(&child, depth + 1)).await {
                Ok(child) => raw.children.push(child),
                Err(DriverError::Timeout { .. }) => {
                    self.reason = Some(IncompleteReason::Deadline);
                    break;
                }
                Err(error) => return Err(error),
            }
            if self.reason == Some(IncompleteReason::Deadline) {
                break;
            }
        }
        Ok(raw)
    }

    async fn read_node(&mut self, node: &S::Node, depth: usize) -> Result<(RawNode, Vec<S::Node>)> {
        macro_rules! read {
            ($call:expr) => {
                soften(with_cancel(self.cancel, $call).await, &mut self.reason)?
            };
        }
        let role_name = read!(self.source.role(node)).unwrap_or_else(|| "AXUnknown".into());
        let subrole = read!(self.source.subrole(node)).unwrap_or(None);
        let mapped = map_ax_role(&role_name, subrole.as_deref());
        let bits = read!(self.source.states(node)).unwrap_or_default();
        let states = map_ax_states(&bits, mapped.role, mapped.password, mapped.modal);
        let name = read!(self.source.title(node)).unwrap_or(None);
        let description = read!(self.source.description(node)).unwrap_or(None);
        let placeholder = read!(self.source.placeholder(node)).unwrap_or(None);
        let help = read!(self.source.help(node)).unwrap_or(None);
        let identifier = read!(self.source.identifier(node)).unwrap_or(None);
        let frame = read!(self.source.frame(node)).unwrap_or(None);
        let actions = read!(self.source.actions(node)).unwrap_or_default();
        let selected = read!(self.source.selected_text_range(node)).unwrap_or(None);
        let visible = read!(self.source.visible_text_range(node)).unwrap_or(None);
        let full_chars = read!(self.source.number_of_characters(node)).unwrap_or(None);
        let url = read!(self.source.url(node)).unwrap_or(None);
        let raw_value = read!(self.source.value(node)).unwrap_or(None);
        let (value, text_extras) = bound_value(
            raw_value,
            self.opts.text_limit,
            full_chars,
            selected,
            visible,
        );
        // Password values are replaced and text extras dropped so a range
        // cannot be paired with a secret the source already redacted.
        let (value, text_extras) = if states.contains(States::PASSWORD) {
            (Some(NodeValue::Text(PASSWORD_PLACEHOLDER.into())), None)
        } else {
            (value, text_extras)
        };
        let mut native_role = mapped.native_role;
        if is_terminal_like(
            native_role.as_str(),
            mapped.subrole.as_deref(),
            name.as_deref(),
        ) && !native_role.to_ascii_lowercase().contains("terminal")
        {
            native_role = SmolStr::new(format!("{native_role}:terminal"));
        }
        let frame = frame.map(|rect| {
            window_relative_frame(rect, self.origin_x, self.origin_y, self.scale, depth == 0)
        });
        let children = read!(self.source.children(node)).unwrap_or_default();
        Ok((
            RawNode {
                role: mapped.role,
                native_role,
                subrole: mapped.subrole,
                name,
                value,
                description,
                placeholder,
                help,
                identifier,
                states,
                actions: SmallVec::from_vec(actions),
                frame,
                text_extras,
                url,
                children: Vec::new(),
                handle: self.source.handle(node),
                fingerprint: Fingerprint(0),
            },
            children,
        ))
    }
}

/// Map AX bits onto [`States`].
///
/// Missing `enabled` sets [`States::ENABLED`]. A text role or a password role
/// forces `EDITABLE | SETTABLE`. `OFFSCREEN` is set only when the source knew
/// the bit. `REQUIRED` is set only when the source knew it.
pub fn map_ax_states(bits: &AxStateBits, role: Role, password: bool, modal: bool) -> States {
    let mut states = States::empty();
    match bits.enabled {
        Some(true) | None => states |= States::ENABLED,
        Some(false) => {}
    }
    if bits.focused.is_some() {
        states |= States::FOCUSABLE;
    }
    if bits.focused == Some(true) {
        states |= States::FOCUSED;
    }
    if bits.selected.is_some() {
        states |= States::SELECTABLE;
    }
    if bits.selected == Some(true) {
        states |= States::SELECTED;
    }
    if bits.expanded.is_some() {
        states |= States::EXPANDABLE;
    }
    if bits.expanded == Some(true) {
        states |= States::EXPANDED;
    }
    if bits.checked == Some(true) {
        states |= States::CHECKED;
    }
    if bits.mixed {
        states |= States::MIXED;
    }
    if bits.editable {
        states |= States::EDITABLE | States::SETTABLE;
    }
    if bits.settable {
        states |= States::SETTABLE;
    }
    if bits.offscreen {
        states |= States::OFFSCREEN;
    }
    if bits.modal || modal {
        states |= States::MODAL;
    }
    if bits.busy {
        states |= States::BUSY;
    }
    if bits.required {
        states |= States::REQUIRED;
    }
    if password {
        states |= States::PASSWORD;
    }
    if password || is_text_role(role) {
        states |= States::EDITABLE | States::SETTABLE;
    }
    states
}

fn is_text_role(role: Role) -> bool {
    matches!(
        role,
        Role::TextField | Role::TextArea | Role::SearchField | Role::ComboBox
    )
}

fn bound_value(
    value: Option<NodeValue>,
    text_limit: Option<usize>,
    full_chars: Option<usize>,
    selected: Option<gcu_core::model::TextRange>,
    visible: Option<gcu_core::model::TextRange>,
) -> (Option<NodeValue>, Option<TextExtras>) {
    let value = match value {
        Some(NodeValue::Text(text)) => {
            Some(NodeValue::Text(bound_text(&text, text_limit, full_chars)))
        }
        other => other,
    };
    let extras = if selected.is_none() && visible.is_none() {
        None
    } else {
        Some(TextExtras {
            selected,
            visible,
            text: None,
        })
    };
    (value, extras)
}

/// Cut `text` to the character limit and the render byte cap.
///
/// When characters are removed, or `full_chars` is larger than the kept text,
/// the result ends with `…(+N chars)` where N is the number of characters not
/// shown.
pub fn bound_text(text: &str, char_limit: Option<usize>, full_chars: Option<usize>) -> String {
    let mut end = text.len().min(RENDER_BYTE_CAP);
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    if let Some(limit) = char_limit {
        let mut char_end = 0;
        for (count, (index, ch)) in text.char_indices().enumerate() {
            if count >= limit || index >= end {
                break;
            }
            char_end = index + ch.len_utf8();
        }
        end = char_end;
    }
    let shown = &text[..end];
    let kept = shown.chars().count();
    let total = full_chars.unwrap_or_else(|| text.chars().count());
    if total > kept {
        format!("{shown}…(+{} chars)", total - kept)
    } else {
        shown.to_string()
    }
}

fn window_relative_frame(
    frame: Rect,
    origin_x: f64,
    origin_y: f64,
    scale: f64,
    root: bool,
) -> Rect {
    if root {
        Rect {
            x: 0.0,
            y: 0.0,
            w: frame.w * scale,
            h: frame.h * scale,
        }
    } else {
        Rect {
            x: (frame.x - origin_x) * scale,
            y: (frame.y - origin_y) * scale,
            w: frame.w * scale,
            h: frame.h * scale,
        }
    }
}

fn normalized_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

async fn with_cancel<T>(
    cancel: &Cancel,
    fut: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
        result = fut => result,
    }
}

fn soften<T>(result: Result<T>, reason: &mut Option<IncompleteReason>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(DriverError::Native { .. }) => {
            reason.get_or_insert(IncompleteReason::PartialError);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// Map an AX action name onto a normalized action. Unknown names stay native.
pub fn map_ax_action(name: &str) -> NodeAction {
    match name {
        "AXPress" => NodeAction::Press,
        "AXShowMenu" => NodeAction::ShowMenu,
        "AXIncrement" => NodeAction::Increment,
        "AXDecrement" => NodeAction::Decrement,
        "AXConfirm" => NodeAction::Confirm,
        "AXCancel" => NodeAction::Cancel,
        "AXRaise" => NodeAction::Raise,
        "AXPick" => NodeAction::Select,
        "AXScrollUpByPage" => NodeAction::ScrollUp,
        "AXScrollDownByPage" => NodeAction::ScrollDown,
        "AXScrollLeftByPage" => NodeAction::ScrollLeft,
        "AXScrollRightByPage" => NodeAction::ScrollRight,
        other => NodeAction::Native(SmolStr::new(other)),
    }
}
