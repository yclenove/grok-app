//! AT-SPI tree building pipeline.

use super::{roles::map_atspi_role, sensitive::is_terminal_like, source::AtspiSource};
use gcu_core::{
    driver::{Cancel, DriverError, TreeOpts},
    fingerprint,
    model::{Fingerprint, RawNode, RawTree, Rect, States},
};
use smol_str::SmolStr;
use std::{collections::HashMap, time::Duration};

pub use super::bus::{AtspiNodeRef, DBUS_CALL_TIMEOUT};
pub use super::source::{
    AtspiSource as AtspiTreeSource, FakeNode as AtspiFakeNode, FakeSource as AtspiFakeSource,
};

/// Hard timeout for building the AT-SPI accessibility tree.
pub const TREE_BUILD_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum node ceiling for raw tree traversal.
pub const MAX_NODES_CEILING: usize = 5000;

/// Convert AT-SPI states and role attributes into unified `States`.
pub fn map_atspi_states(
    states: atspi::StateSet,
    role: atspi::Role,
    attributes: &HashMap<String, String>,
) -> States {
    let mut s = States::empty();

    if states.contains(atspi::State::Enabled) || states.contains(atspi::State::Sensitive) {
        s |= States::ENABLED;
    }
    if states.contains(atspi::State::Focused) {
        s |= States::FOCUSED;
    }
    if states.contains(atspi::State::Focusable) {
        s |= States::FOCUSABLE;
    }
    if states.contains(atspi::State::Selected) {
        s |= States::SELECTED;
    }
    if states.contains(atspi::State::Selectable) {
        s |= States::SELECTABLE;
    }
    if states.contains(atspi::State::Expanded) {
        s |= States::EXPANDED;
    }
    if states.contains(atspi::State::Expandable) {
        s |= States::EXPANDABLE;
    }
    if states.contains(atspi::State::Checked) {
        s |= States::CHECKED;
    }
    if states.contains(atspi::State::Indeterminate) {
        s |= States::MIXED;
    }
    if states.contains(atspi::State::Editable) {
        s |= States::EDITABLE | States::SETTABLE;
    }
    if role == atspi::Role::PasswordText
        || attributes.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("secret")
                || k.eq_ignore_ascii_case("password")
                || v.eq_ignore_ascii_case("password")
        })
    {
        s |= States::PASSWORD;
    }
    if !states.contains(atspi::State::Showing) && !states.contains(atspi::State::Visible) {
        s |= States::OFFSCREEN;
    }
    if states.contains(atspi::State::Modal) {
        s |= States::MODAL;
    }
    if states.contains(atspi::State::Busy) {
        s |= States::BUSY;
    }
    if states.contains(atspi::State::Required) {
        s |= States::REQUIRED;
    }

    s
}

/// Build a unified `RawTree` from an `AtspiSource`.
///
/// Features:
/// - Batched property traversal
/// - Screen coordinates converted to window-relative physical pixels
/// - Maximum 5000 nodes hard limit
/// - 2s hard traversal deadline, with per D-Bus call timeouts
/// - Complete states bitflags mapping
/// - Deterministic fingerprint generation
pub async fn build_raw_tree<S: AtspiSource>(
    source: &S,
    root: &S::Node,
    opts: &TreeOpts,
    cancel: &Cancel,
) -> Result<RawTree, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    let timeout_budget = opts.timeout.min(TREE_BUILD_TIMEOUT);
    tokio::time::timeout(timeout_budget, async {
        let max_nodes = opts.max_tree_nodes.min(MAX_NODES_CEILING);
        let mut node_count = 0;

        let root_extents = source.extents(root).await.ok().flatten();
        let (origin_x, origin_y) = match root_extents {
            Some(r) => (r.x, r.y),
            None => (0.0, 0.0),
        };

        let mut root_node = build_node_recursive(
            source,
            root,
            opts,
            cancel,
            origin_x,
            origin_y,
            max_nodes,
            &mut node_count,
            0,
        )
        .await?;

        fingerprint::refresh_tree(&mut root_node, Fingerprint(0), 0);

        Ok(RawTree {
            root: Some(root_node),
        })
    })
    .await
    .map_err(|_| DriverError::Timeout {
        maybe_executed: false,
    })?
}

#[allow(clippy::too_many_arguments)]
async fn build_node_recursive<S: AtspiSource>(
    source: &S,
    node: &S::Node,
    opts: &TreeOpts,
    cancel: &Cancel,
    origin_x: f64,
    origin_y: f64,
    max_nodes: usize,
    node_count: &mut usize,
    depth: usize,
) -> Result<RawNode, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }

    *node_count += 1;
    let atspi_role = source.role(node).await.unwrap_or(atspi::Role::Unknown);
    let (core_role, mut native_role) = map_atspi_role(atspi_role);
    let name = source.name(node).await.ok().flatten();
    let description = source.description(node).await.ok().flatten();
    let help = source.help(node).await.ok().flatten();
    let identifier = source.identifier(node).await.ok().flatten();
    let attributes = source.attributes(node).await.unwrap_or_default();
    let state_set = source
        .states(node)
        .await
        .unwrap_or_else(|_| atspi::StateSet::default());
    let states = map_atspi_states(state_set, atspi_role, &attributes);
    let value = source.value(node).await.ok().flatten();
    let extents = source.extents(node).await.ok().flatten();
    let actions = source
        .actions(node)
        .await
        .map(smallvec::SmallVec::from_vec)
        .unwrap_or_default();
    let text_extras = source
        .text_extras(node, opts.text_limit)
        .await
        .ok()
        .flatten();
    let url = source.url(node).await.ok().flatten();
    let handle = source.handle(node);

    // Convert frame into window-relative physical pixels
    let frame = extents.map(|rect| {
        if depth == 0 {
            Rect {
                x: 0.0,
                y: 0.0,
                w: rect.w,
                h: rect.h,
            }
        } else {
            Rect {
                x: rect.x - origin_x,
                y: rect.y - origin_y,
                w: rect.w,
                h: rect.h,
            }
        }
    });

    // Check terminal-like execution surface heuristic
    if is_terminal_like(atspi_role, &native_role, name.as_deref())
        && !native_role.to_ascii_lowercase().contains("terminal")
    {
        native_role = SmolStr::new(format!("{native_role}:terminal"));
    }

    let mut children = Vec::new();
    if *node_count < max_nodes && depth < opts.max_tree_depth {
        let child_nodes = source.children(node).await.unwrap_or_default();
        for child in child_nodes {
            if *node_count >= max_nodes {
                break;
            }
            let child_raw = Box::pin(build_node_recursive(
                source,
                &child,
                opts,
                cancel,
                origin_x,
                origin_y,
                max_nodes,
                node_count,
                depth + 1,
            ))
            .await?;
            children.push(child_raw);
        }
    }

    Ok(RawNode {
        role: core_role,
        native_role,
        subrole: None,
        name,
        value,
        description,
        placeholder: attributes.get("placeholder-text").cloned(),
        help,
        identifier,
        states,
        actions,
        frame,
        text_extras,
        url,
        children,
        handle,
        fingerprint: Fingerprint(0),
    })
}
