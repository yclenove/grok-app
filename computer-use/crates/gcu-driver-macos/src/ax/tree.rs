//! Pure AX tree walk. No macOS calls.
//!
//! The walk stops at [`TREE_BUILD_TIMEOUT`] and [`MAX_NODES_CEILING`]. A zero
//! node budget returns an empty tree. Cancellation is checked before every
//! source call. Reads report `maybe_executed = false`. A native failure on one
//! field becomes an empty field. `PermissionMissing`, `Cancelled`, `Timeout`,
//! and `Crashed` stop the walk.
//!
//! Frames are AX points times `scale`, then shifted so the window origin is
//! `(0, 0)`. The Y axis is not flipped. Text is cut to `text_limit` and to
//! [`RENDER_BYTE_CAP`] bytes. A cut value ends with `…(+N chars)`.

use std::time::Duration;

use gcu_core::{
    driver::{Cancel, DriverError, TreeOpts},
    fingerprint,
    model::{Fingerprint, NodeAction, NodeValue, RawNode, RawTree, Rect, Role, States, TextExtras},
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
        return Ok(RawTree { root: None });
    }
    let timeout = opts.timeout.min(TREE_BUILD_TIMEOUT);
    let walk = async {
        let mut walk = Walk {
            source,
            opts,
            cancel,
            origin_x: 0.0,
            origin_y: 0.0,
            scale: normalized_scale(scale),
            max_nodes: opts.max_tree_nodes.min(MAX_NODES_CEILING),
            node_count: 0,
        };
        let root_frame = soften(with_cancel(cancel, source.frame(root)).await)?;
        if let Some(Some(frame)) = root_frame {
            walk.origin_x = frame.x;
            walk.origin_y = frame.y;
        }
        let mut root_node = walk.node(root, 0).await?;
        fingerprint::refresh_tree(&mut root_node, Fingerprint(0), 0);
        Ok(RawTree {
            root: Some(root_node),
        })
    };
    match tokio::time::timeout(timeout, walk).await {
        Ok(result) => result,
        Err(_) => Err(DriverError::Timeout {
            maybe_executed: false,
        }),
    }
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
}

impl<S: AxSource> Walk<'_, S> {
    async fn node(&mut self, node: &S::Node, depth: usize) -> Result<RawNode> {
        if self.cancel.is_cancelled() {
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        self.node_count += 1;
        let role_name = soften(with_cancel(self.cancel, self.source.role(node)).await)?
            .unwrap_or_else(|| "AXUnknown".into());
        let subrole =
            soften(with_cancel(self.cancel, self.source.subrole(node)).await)?.unwrap_or(None);
        let mapped = map_ax_role(&role_name, subrole.as_deref());
        let bits =
            soften(with_cancel(self.cancel, self.source.states(node)).await)?.unwrap_or_default();
        let states = map_ax_states(&bits, mapped.role, mapped.password, mapped.modal);
        let name = soften(with_cancel(self.cancel, self.source.title(node)).await)?.unwrap_or(None);
        let description =
            soften(with_cancel(self.cancel, self.source.description(node)).await)?.unwrap_or(None);
        let placeholder =
            soften(with_cancel(self.cancel, self.source.placeholder(node)).await)?.unwrap_or(None);
        let help = soften(with_cancel(self.cancel, self.source.help(node)).await)?.unwrap_or(None);
        let identifier =
            soften(with_cancel(self.cancel, self.source.identifier(node)).await)?.unwrap_or(None);
        let frame =
            soften(with_cancel(self.cancel, self.source.frame(node)).await)?.unwrap_or(None);
        let actions =
            soften(with_cancel(self.cancel, self.source.actions(node)).await)?.unwrap_or_default();
        let selected =
            soften(with_cancel(self.cancel, self.source.selected_text_range(node)).await)?
                .unwrap_or(None);
        let visible = soften(with_cancel(self.cancel, self.source.visible_text_range(node)).await)?
            .unwrap_or(None);
        let full_chars =
            soften(with_cancel(self.cancel, self.source.number_of_characters(node)).await)?
                .unwrap_or(None);
        let url = soften(with_cancel(self.cancel, self.source.url(node)).await)?.unwrap_or(None);
        let raw_value =
            soften(with_cancel(self.cancel, self.source.value(node)).await)?.unwrap_or(None);
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
        let mut children = Vec::new();
        if self.node_count < self.max_nodes && depth + 1 < self.opts.max_tree_depth {
            if let Some(kids) = soften(with_cancel(self.cancel, self.source.children(node)).await)?
            {
                for child in kids {
                    if self.node_count >= self.max_nodes {
                        break;
                    }
                    children.push(Box::pin(self.node(&child, depth + 1)).await?);
                }
            }
        }
        Ok(RawNode {
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
            children,
            handle: self.source.handle(node),
            fingerprint: Fingerprint(0),
        })
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

fn soften<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(DriverError::Native { .. }) => Ok(None),
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
