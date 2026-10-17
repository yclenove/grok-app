use crate::messages::OverlayShow;
use gcu_protocol::Rect;
use serde::Serialize;

/// One monitor reported by the OS, in physical screen pixels.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Monitor {
    /// Physical bounds, including negative origins.
    pub frame: Rect,
    /// This monitor's backing scale, independent of the target window's DPI.
    pub scale: f64,
}
/// Overlay-local CSS coordinate.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Point {
    /// Logical x.
    pub x: f64,
    /// Logical y.
    pub y: f64,
}
/// Placement of separate drawing and capsule surfaces.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Placement {
    /// Chosen display; the drawing surface fills these physical bounds.
    pub monitor: Monitor,
    /// Physical capsule rectangle, fully on the chosen display.
    pub capsule: Rect,
    /// Monitor-local logical pointer. None when target is on a disconnected display.
    pub cursor: Option<Point>,
}
pub(super) fn valid_scale(scale: f64) -> bool {
    scale.is_finite() && (0.25..=16.).contains(&scale)
}
pub(super) fn valid_rect(r: Rect) -> bool {
    [r.x, r.y, r.w, r.h]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 10_000_000.)
        && r.w > 0.
        && r.h > 0.
        && r.w <= 32768.
        && r.h <= 32768.
}
fn contains(r: Rect, x: f64, y: f64) -> bool {
    x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h
}
fn overlap(a: Rect, b: Rect) -> f64 {
    ((a.x + a.w).min(b.x + b.w) - a.x.max(b.x)).max(0.)
        * ((a.y + a.h).min(b.y + b.h) - a.y.max(b.y)).max(0.)
}
/// Select pointer's monitor, then largest target overlap, then first valid display.
/// Convert relative to that monitor before dividing by its DPI; never scale origins.
/// Pure bounded computation; no waits, IO or cancellation.
pub fn placement(monitors: &[Monitor], target: Option<&OverlayShow>) -> Option<Placement> {
    let valid: Vec<_> = monitors
        .iter()
        .take(64)
        .filter(|m| valid_rect(m.frame) && valid_scale(m.scale))
        .copied()
        .collect();
    let monitor = target
        .and_then(|t| valid.iter().find(|m| contains(m.frame, t.x, t.y)))
        .or_else(|| {
            target.and_then(|t| {
                valid
                    .iter()
                    .filter(|m| overlap(m.frame, t.frame) > 0.)
                    .max_by(|a, b| overlap(a.frame, t.frame).total_cmp(&overlap(b.frame, t.frame)))
            })
        })
        .or(valid.first())
        .copied()?;
    let w = (640. * monitor.scale).min(monitor.frame.w);
    let h = (56. * monitor.scale).min(monitor.frame.h);
    let capsule = Rect {
        x: monitor.frame.x + (monitor.frame.w - w) / 2.,
        y: monitor.frame.y + (12. * monitor.scale).min(monitor.frame.h - h),
        w,
        h,
    };
    let cursor = target
        .filter(|t| contains(monitor.frame, t.x, t.y))
        .map(|t| Point {
            x: (t.x - monitor.frame.x) / monitor.scale,
            y: (t.y - monitor.frame.y) / monitor.scale,
        });
    Some(Placement {
        monitor,
        capsule,
        cursor,
    })
}

/// macOS uses global points multiplied by the target window's backing factor,
/// whereas Tauri scales each display's origin by that display's own factor.
/// Select in global points, then restore the selected display's physical space.
/// Pure bounded computation; no IO, waits or cancellation.
pub fn backing_placement(monitors: &[Monitor], target: Option<&OverlayShow>) -> Option<Placement> {
    let points: Vec<_> = monitors
        .iter()
        .take(64)
        .filter(|m| valid_rect(m.frame) && valid_scale(m.scale))
        .map(|m| Monitor {
            frame: scaled(m.frame, 1. / m.scale),
            scale: 1.,
        })
        .collect();
    let target = target.filter(|t| valid_scale(t.scale)).map(|t| {
        let mut point = t.clone();
        point.frame = scaled(t.frame, 1. / t.scale);
        point.x /= t.scale;
        point.y /= t.scale;
        point.scale = 1.;
        point
    });
    let logical = placement(&points, target.as_ref())?;
    let monitor = monitors.iter().take(64).find(|m| {
        valid_rect(m.frame)
            && valid_scale(m.scale)
            && scaled(m.frame, 1. / m.scale) == logical.monitor.frame
    })?;
    Some(Placement {
        monitor: *monitor,
        capsule: scaled(logical.capsule, monitor.scale),
        cursor: logical.cursor,
    })
}

fn scaled(r: Rect, scale: f64) -> Rect {
    Rect {
        x: r.x * scale,
        y: r.y * scale,
        w: r.w * scale,
        h: r.h * scale,
    }
}
