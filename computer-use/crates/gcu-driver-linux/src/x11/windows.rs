//! X11 window enumeration and alignment with AT-SPI window candidates.

use crate::x11::{XError, XServer, XWindowGeometry};
use gcu_protocol::{Rect, WindowInfo};
use std::collections::{HashMap, HashSet};

/// Candidate window from AT-SPI tree traversal.
#[derive(Debug, Clone, PartialEq)]
pub struct AtspiWindowCandidate {
    /// AT-SPI object path (e.g. `/org/a11y/atspi/accessible/...`).
    pub path: String,
    /// Window title reported by AT-SPI.
    pub title: String,
    /// Screen-relative frame reported by AT-SPI extents.
    pub frame: Rect,
    /// Associated process ID if resolvable.
    pub pid: Option<u32>,
}

/// Candidate window from X11 server enumeration.
#[derive(Debug, Clone, PartialEq)]
pub struct XWindowCandidate {
    /// X11 window ID.
    pub id: u32,
    /// Window title from `_NET_WM_NAME` or `WM_NAME`.
    pub title: Option<String>,
    /// Process ID from `_NET_WM_PID`.
    pub pid: Option<u32>,
    /// Window geometry in root-relative coordinates.
    pub geometry: XWindowGeometry,
    /// Whether the window is currently mapped.
    pub is_mapped: bool,
}

/// Result of aligning AT-SPI and X11 window candidates.
#[derive(Debug, Clone)]
pub struct AlignmentResult {
    /// Final list of window information entries.
    pub windows: Vec<WindowInfo>,
    /// Mapping from AT-SPI object path to matched X11 window ID.
    pub atspi_to_x: HashMap<String, u32>,
    /// Mapping from X11 window ID to matched AT-SPI object path.
    pub x_to_atspi: HashMap<u32, String>,
}

/// Enumerate all managed client windows on the X11 server.
pub fn list_x11_windows(x: &dyn XServer) -> Result<Vec<XWindowCandidate>, XError> {
    let client_ids = x.client_list()?;
    let mut candidates = Vec::with_capacity(client_ids.len());
    for id in client_ids {
        if let Ok(geom) = x.window_geometry(id) {
            let title = x.window_name(id).ok().flatten();
            let pid = x.window_pid(id).ok().flatten();
            let is_mapped = x.is_window_mapped(id).unwrap_or(false);
            candidates.push(XWindowCandidate {
                id,
                title,
                pid,
                geometry: geom,
                is_mapped,
            });
        }
    }
    Ok(candidates)
}

/// Compute a match score between an AT-SPI candidate and an X11 window candidate.
///
/// Returns positive scores for compatible pairs and negative penalties for mismatches.
pub fn score_match(atspi: &AtspiWindowCandidate, x: &XWindowCandidate) -> i32 {
    let mut score = 0;

    // Process ID comparison: primary authoritative signal.
    match (atspi.pid, x.pid) {
        (Some(p1), Some(p2)) if p1 == p2 => score += 100,
        (Some(p1), Some(p2)) if p1 != p2 => return -1000,
        _ => {}
    }

    // Window title comparison.
    if let Some(ref x_title) = x.title {
        let t_atspi = atspi.title.trim();
        let t_x = x_title.trim();
        if !t_atspi.is_empty() && !t_x.is_empty() {
            if t_atspi.eq_ignore_ascii_case(t_x) {
                score += 60;
            } else if t_atspi.contains(t_x) || t_x.contains(t_atspi) {
                score += 35;
            }
        }
    }

    // Geometry comparison: bounds difference in screen coordinates.
    let dw = (atspi.frame.w - x.geometry.width as f64).abs();
    let dh = (atspi.frame.h - x.geometry.height as f64).abs();
    let dx = (atspi.frame.x - x.geometry.x as f64).abs();
    let dy = (atspi.frame.y - x.geometry.y as f64).abs();

    if dw < 4.0 && dh < 4.0 {
        score += 40;
        if dx < 4.0 && dy < 4.0 {
            score += 20;
        }
    } else if dw < 30.0 && dh < 30.0 {
        score += 20;
    }

    score
}

/// Align AT-SPI window candidates with X11 window candidates.
///
/// Matches each AT-SPI candidate to the best corresponding X11 candidate by
/// process ID, title, and bounding geometry. When a match is found, upgrades
/// the window ID to the exact X11 ID. If no match satisfies the threshold,
/// retains the AT-SPI path as the ID and logs a diagnostic warning.
pub fn align_windows(
    atspi_windows: &[AtspiWindowCandidate],
    x_windows: &[XWindowCandidate],
    scale: f64,
) -> AlignmentResult {
    let mut atspi_to_x = HashMap::new();
    let mut x_to_atspi = HashMap::new();
    let mut used_x = HashSet::new();
    let mut windows = Vec::new();

    for atspi in atspi_windows {
        let mut best_match: Option<(&XWindowCandidate, i32)> = None;

        for x in x_windows {
            if used_x.contains(&x.id) {
                continue;
            }
            let score = score_match(atspi, x);
            if score >= 40 {
                if let Some((_, best_score)) = best_match {
                    if score > best_score {
                        best_match = Some((x, score));
                    }
                } else {
                    best_match = Some((x, score));
                }
            }
        }

        if let Some((x, _)) = best_match {
            used_x.insert(x.id);
            atspi_to_x.insert(atspi.path.clone(), x.id);
            x_to_atspi.insert(x.id, atspi.path.clone());

            let title = x.title.clone().unwrap_or_else(|| atspi.title.clone());
            windows.push(WindowInfo {
                id: x.id.to_string(),
                title,
                frame: Rect {
                    x: x.geometry.x as f64,
                    y: x.geometry.y as f64,
                    w: x.geometry.width as f64,
                    h: x.geometry.height as f64,
                },
                scale,
                on_current_desktop: Some(true),
            });
        } else {
            tracing::warn!(
                atspi_path = %atspi.path,
                title = %atspi.title,
                "Could not align AT-SPI window with X11 client window; keeping AT-SPI approximation"
            );
            windows.push(WindowInfo {
                id: atspi.path.clone(),
                title: atspi.title.clone(),
                frame: atspi.frame,
                scale,
                on_current_desktop: Some(true),
            });
        }
    }

    AlignmentResult {
        windows,
        atspi_to_x,
        x_to_atspi,
    }
}
