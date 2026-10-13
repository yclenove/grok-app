//! Filter layer-0 windows and align them with Accessibility windows.
//!
//! Alignment is greedy and stable. Intersection-over-union of at least one half
//! wins. A shared nonempty title plus a similar size is a weaker match when the
//! rectangles disagree. Unmatched Core Graphics windows stay in the list with
//! no Accessibility index. `on_current_desktop` is copied through; this module
//! does not invent Space membership.

use crate::api::{AxWindow, CgWindow};
use gcu_protocol::{Rect, WindowInfo};

/// One window the driver can hand back, plus the handles needed to raise it.
#[derive(Clone, Debug, PartialEq)]
pub struct AlignedWindow {
    /// `cg:{window_id}:{pid}`.
    pub id: String,
    /// Accessibility title when it is nonempty, otherwise the Core Graphics title.
    pub title: String,
    /// Screen rectangle.
    pub frame: Rect,
    /// Display scale from the Core Graphics row.
    pub scale: f64,
    /// Current Space, or `None` when the source did not know.
    pub on_current_desktop: Option<bool>,
    /// Owning pid.
    pub pid: u32,
    /// Minimized according to Accessibility. False when there is no match.
    pub minimized: bool,
    /// `AXMain`, when an Accessibility window matched.
    pub main: bool,
    /// Index for [`crate::api::MacApi::ax_raise`].
    pub ax_index: Option<u32>,
    /// Core Graphics window id.
    pub cg_id: u32,
}

impl AlignedWindow {
    /// Portable window record without driver-private handles.
    pub fn info(&self) -> WindowInfo {
        WindowInfo {
            id: self.id.clone(),
            title: self.title.clone(),
            frame: self.frame,
            scale: self.scale,
            on_current_desktop: self.on_current_desktop,
        }
    }
}

/// Opaque id. The pid is included so a recycled window id in another process
/// does not alias the old grant.
pub fn window_id(cg_id: u32, pid: u32) -> String {
    format!("cg:{cg_id}:{pid}")
}

/// Layer 0, visible, non-empty, and not the desktop or WindowServer.
pub fn is_listed(window: &CgWindow) -> bool {
    window.layer == 0
        && window.alpha > 0.01
        && window.frame.w >= 2.0
        && window.frame.h >= 2.0
        && window.pid > 0
        && !is_desktop(window)
}

/// Align Core Graphics rows for `pid` with that process's Accessibility windows.
pub fn align_pid(pid: u32, cg: &[CgWindow], ax: &[AxWindow]) -> Vec<AlignedWindow> {
    let rows: Vec<&CgWindow> = cg
        .iter()
        .filter(|window| window.pid == pid && is_listed(window))
        .collect();
    let mut used = vec![false; ax.len()];
    let mut aligned = Vec::with_capacity(rows.len());
    for window in rows {
        let mut best: Option<(usize, u8, f64)> = None;
        for (index, candidate) in ax.iter().enumerate() {
            if used[index] {
                continue;
            }
            let Some((rank, score)) = match_score(window, candidate) else {
                continue;
            };
            let replace = match best {
                None => true,
                Some((_, best_rank, best_score)) => {
                    rank > best_rank || (rank == best_rank && score > best_score)
                }
            };
            if replace {
                best = Some((index, rank, score));
            }
        }
        let (title, minimized, main, ax_index) = if let Some((index, _, _)) = best {
            used[index] = true;
            let candidate = &ax[index];
            let title = if candidate.title.trim().is_empty() {
                window.title.clone()
            } else {
                candidate.title.clone()
            };
            (
                title,
                candidate.minimized,
                candidate.main,
                Some(candidate.index),
            )
        } else {
            (window.title.clone(), false, false, None)
        };
        aligned.push(AlignedWindow {
            id: window_id(window.window_id, window.pid),
            title,
            frame: window.frame,
            scale: window.scale,
            on_current_desktop: window.on_current_desktop,
            pid: window.pid,
            minimized,
            main,
            ax_index,
            cg_id: window.window_id,
        });
    }
    aligned
}

fn is_desktop(window: &CgWindow) -> bool {
    let owner = window.owner_name.trim();
    owner.eq_ignore_ascii_case("Window Server") || owner.eq_ignore_ascii_case("WindowServer")
}

/// Rank 2 is intersection-over-union. Rank 1 is title plus similar size.
fn match_score(window: &CgWindow, candidate: &AxWindow) -> Option<(u8, f64)> {
    let overlap = iou(window.frame, candidate.frame);
    if overlap >= 0.5 {
        return Some((2, overlap));
    }
    let titled = !window.title.trim().is_empty()
        && window
            .title
            .trim()
            .eq_ignore_ascii_case(candidate.title.trim());
    if titled && similar_size(window.frame, candidate.frame) {
        return Some((1, overlap));
    }
    None
}

fn similar_size(left: Rect, right: Rect) -> bool {
    close(left.w, right.w) && close(left.h, right.h)
}

fn close(left: f64, right: f64) -> bool {
    let delta = (left - right).abs();
    delta <= 4.0 || delta <= 0.1 * left.max(right)
}

fn iou(left: Rect, right: Rect) -> f64 {
    let x1 = left.x.max(right.x);
    let y1 = left.y.max(right.y);
    let x2 = (left.x + left.w).min(right.x + right.w);
    let y2 = (left.y + left.h).min(right.y + right.h);
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = area(left) + area(right) - intersection;
    if union <= f64::EPSILON {
        0.0
    } else {
        intersection / union
    }
}

fn area(rect: Rect) -> f64 {
    rect.w.max(0.0) * rect.h.max(0.0)
}
