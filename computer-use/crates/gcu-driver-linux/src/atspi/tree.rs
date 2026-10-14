//! AT-SPI tree building pipeline.

use super::{roles::map_atspi_role, sensitive::is_terminal_like, source::AtspiSource};
use gcu_core::{
    driver::{Cancel, DriverError, TreeOpts},
    fingerprint,
    model::{Fingerprint, Incomplete, IncompleteReason, RawNode, RawTree, Rect, States},
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
///
/// Cancellation and fatal errors return errors. Deadline expiry after the root
/// was read retains partial nodes and marks them incomplete; an unread root times
/// out. Native property/children failures are marked, never silently certified.
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
        deadline,
        origin_x: 0.,
        origin_y: 0.,
        max_nodes: opts.max_tree_nodes.min(MAX_NODES_CEILING),
        node_count: 0,
        reason: None,
    };
    let root_extents = bounded(cancel, deadline, source.extents(root)).await;
    if let Some(r) = soften(root_extents, &mut walk.reason)?.flatten() {
        walk.origin_x = r.x;
        walk.origin_y = r.y;
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
    deadline: tokio::time::Instant,
    origin_x: f64,
    origin_y: f64,
    max_nodes: usize,
    node_count: usize,
    reason: Option<IncompleteReason>,
}

impl<S: AtspiSource> Walk<'_, S> {
    async fn node(&mut self, node: &S::Node, depth: usize) -> Result<RawNode, DriverError> {
        let (mut raw, children) =
            bounded(self.cancel, self.deadline, self.read_node(node, depth)).await?;
        self.node_count += 1;
        for child in children {
            if self.node_count >= self.max_nodes {
                self.reason.get_or_insert(IncompleteReason::NodeCap);
                break;
            }
            if depth >= self.opts.max_tree_depth {
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

    async fn read_node(
        &mut self,
        node: &S::Node,
        depth: usize,
    ) -> Result<(RawNode, Vec<S::Node>), DriverError> {
        macro_rules! read {
            ($call:expr) => {
                soften($call.await, &mut self.reason)?
            };
        }
        let atspi_role = read!(self.source.role(node)).unwrap_or(atspi::Role::Unknown);
        let (core_role, mut native_role) = map_atspi_role(atspi_role);
        let name = read!(self.source.name(node)).flatten();
        let description = read!(self.source.description(node)).flatten();
        let help = read!(self.source.help(node)).flatten();
        let identifier = read!(self.source.identifier(node)).flatten();
        let attributes = read!(self.source.attributes(node)).unwrap_or_default();
        let state_set = read!(self.source.states(node)).unwrap_or_default();
        let mut states = map_atspi_states(state_set, atspi_role, &attributes);
        if name
            .as_deref()
            .is_some_and(|s| s.to_ascii_lowercase().contains("password"))
            || identifier
                .as_deref()
                .is_some_and(|s| s.to_ascii_lowercase().contains("password"))
        {
            states |= States::PASSWORD;
        }
        let value = read!(self.source.value(node)).flatten();
        let extents = read!(self.source.extents(node)).flatten();
        let actions = read!(self.source.actions(node))
            .map(smallvec::SmallVec::from_vec)
            .unwrap_or_default();
        let text_extras = read!(self.source.text_extras(node, self.opts.text_limit)).flatten();
        let url = read!(self.source.url(node)).flatten();
        let handle = self.source.handle(node);

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
                    x: rect.x - self.origin_x,
                    y: rect.y - self.origin_y,
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

        let children = read!(self.source.children(node)).unwrap_or_default();

        Ok((
            RawNode {
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
                children: Vec::new(),
                handle,
                fingerprint: Fingerprint(0),
            },
            children,
        ))
    }
}

async fn bounded<T>(
    cancel: &Cancel,
    deadline: tokio::time::Instant,
    read: impl std::future::Future<Output = Result<T, DriverError>>,
) -> Result<T, DriverError> {
    if cancel.is_cancelled() {
        return Err(DriverError::Cancelled {
            maybe_executed: false,
        });
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(DriverError::Timeout {
            maybe_executed: false,
        });
    }
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: false }),
        result = tokio::time::timeout_at(deadline, read) => result.map_err(|_| DriverError::Timeout { maybe_executed: false })?,
    }
}

fn soften<T>(
    result: Result<T, DriverError>,
    reason: &mut Option<IncompleteReason>,
) -> Result<Option<T>, DriverError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(DriverError::Native { .. }) => {
            reason.get_or_insert(IncompleteReason::PartialError);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
