//! Index and render an observation before it is installed.
//!
//! This walk is the heavy part of recording. Callers run it without the lease
//! lock and without the snapshot mutex, then install the result in one commit.

use super::store::window_history_charge;
use super::{
    Diff, ElementIndex, ObservationMode, RenderedState, SnapshotError, SnapshotId, WindowKey,
};
use crate::{
    model::{Geometry, NodeValue, States, WindowPoint},
    redact::{PixelHash, RedactedImage, MAX_STATE_SKEW},
    render::RenderOpts,
    tree::{ProcessedNode, ProcessedTree},
};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

/// Copied store counters. The new tree is indexed against this, off the mutex.
pub(crate) struct ObservationBaseline {
    pub history: Option<(u64, bool)>,
    pub previous_index: Option<ElementIndex>,
    pub previous_rendered: Option<RenderedState>,
    pub latest_id: Option<SnapshotId>,
    pub memory_limit: usize,
    pub history_bytes: usize,
}

/// Private observation. Invisible until [`super::store::SnapshotStore::install`].
pub(crate) struct Assembled {
    pub baseline_history: Option<(u64, bool)>,
    pub baseline_latest: Option<SnapshotId>,
    pub window: WindowKey,
    pub index: ElementIndex,
    pub next_index: u64,
    pub rendered: RenderedState,
    pub text: String,
    pub overflow_text: Option<String>,
    pub mode: ObservationMode,
    pub diff: Option<Diff>,
    pub screenshot: Option<RedactedImage>,
    pub target_hashes: BTreeMap<u32, PixelHash>,
    pub geometry: Geometry,
    pub state_version: Option<u64>,
    pub observed_at: Option<Duration>,
    pub memory_bytes: usize,
    pub new_window_bytes: usize,
    pub delivery_size: (u32, u32),
    pub scope_run: Option<String>,
    pub scope_generation: Option<u64>,
}

/// Sanitize, assign indices, render, and hash. Does not touch the store.
///
/// Memory only aside from the CPU work. No IO, timeout, or cancellation.
/// A refusal leaves the store's counters unchanged. `baseline` is a copy taken
/// under the snapshot mutex and then released.
pub(crate) fn assemble(
    mut tree: ProcessedTree,
    opts: &RenderOpts,
    screenshot: Option<RedactedImage>,
    baseline: &ObservationBaseline,
) -> Result<Assembled, SnapshotError> {
    if let Some(image) = &screenshot {
        if !image.is_window_capture()
            || tree.state_version != Some(image.state_version())
            || tree
                .observed_at
                .is_none_or(|t| t.abs_diff(image.captured_at()) > MAX_STATE_SKEW)
            || image.geometry()
                != (Geometry {
                    frame: opts.window.frame,
                    scale: opts.window.scale,
                })
        {
            return Err(SnapshotError::StaleScreenshot);
        }
    }
    sanitize(&mut tree);
    let window = WindowKey::from_options(opts);
    let (index, next_index) = ElementIndex::assign(
        &mut tree,
        baseline.previous_index.as_ref(),
        baseline.history.map_or(0, |(next, _)| next),
    )?;
    let full_text = crate::render::text::file_text(&tree, opts);
    let overflow_text =
        (full_text.len() > crate::render::text::FACTS_PREFIX_BYTES).then_some(full_text);
    let rendered = RenderedState::from_tree(&tree, opts);
    let diff = if baseline.history.is_some_and(|(_, force_full)| !force_full) {
        baseline
            .previous_rendered
            .as_ref()
            .map(|previous| Diff::between(previous, &rendered))
    } else {
        None
    };
    let mode = match &diff {
        None => ObservationMode::Full,
        Some(diff) if diff.is_large_change() => ObservationMode::FullLargeChange,
        Some(_) => ObservationMode::Diff,
    };
    let text = diff.as_ref().map_or_else(|| rendered.text(), Diff::text);
    let mut target_hashes = BTreeMap::new();
    if let Some(image) = &screenshot {
        for (_, node) in tree.nodes() {
            if node.interactive() {
                if let Some(frame) = node.node.frame {
                    let center = WindowPoint {
                        x: frame.x + frame.w / 2.,
                        y: frame.y + frame.h / 2.,
                    };
                    if let Ok(hash) = image.target_hash(center) {
                        target_hashes.insert(node.element_index, hash);
                    }
                }
            }
        }
    }
    let memory_bytes = std::mem::size_of::<super::Snapshot>()
        + window.app_id.capacity()
        + window.window_id.capacity()
        + index.memory_bytes()
        + rendered.memory_bytes()
        + screenshot.as_ref().map_or(0, RedactedImage::memory_bytes)
        + target_hashes.len() * (std::mem::size_of::<(u32, PixelHash)>() + 48);
    let new_window_bytes = if baseline.history.is_none() {
        window_history_charge(&window)
    } else {
        0
    };
    let history_bytes = baseline
        .history_bytes
        .checked_add(new_window_bytes)
        .ok_or(SnapshotError::MemoryLimit)?;
    if history_bytes
        .checked_add(memory_bytes)
        .is_none_or(|size| size > baseline.memory_limit)
    {
        return Err(SnapshotError::MemoryLimit);
    }
    let delivery_size = screenshot
        .as_ref()
        .map_or((0, 0), |image| (image.width(), image.height()));
    Ok(Assembled {
        baseline_history: baseline.history,
        baseline_latest: baseline.latest_id,
        window,
        index,
        next_index,
        rendered,
        text,
        overflow_text,
        mode,
        diff,
        screenshot,
        target_hashes,
        geometry: Geometry {
            frame: opts.window.frame,
            scale: opts.window.scale,
        },
        state_version: tree.state_version,
        observed_at: tree.observed_at,
        memory_bytes,
        new_window_bytes,
        delivery_size,
        scope_run: None,
        scope_generation: None,
    })
}

pub(crate) fn sanitize(tree: &mut ProcessedTree) {
    let sensitive: HashSet<_> = tree
        .sensitive_regions
        .iter()
        .map(|r| r.fingerprint)
        .collect();
    fn walk(
        node: &mut ProcessedNode,
        inherited: bool,
        sensitive: &HashSet<crate::model::Fingerprint>,
    ) {
        let secret = inherited
            || sensitive.contains(&node.node.fingerprint)
            || node.node.states.contains(States::PASSWORD);
        if secret {
            node.node.value = Some(NodeValue::Text("•••".into()));
            node.node.text_extras = None;
            node.node.name = None;
            node.node.description = None;
            node.node.help = None;
            node.node.url = None;
            node.node.identifier = None;
            node.node.placeholder = None;
        }
        for child in &mut node.children {
            walk(child, secret, sensitive);
        }
    }
    if let Some(root) = &mut tree.root {
        walk(root, false, &sensitive);
    }
}
