//! Last-moment delivery proof. A refusal here means the event was not posted.

use gcu_core::scope::WindowBinding;

use super::facts::{contains, OsWindow};
use super::table::front_app;

/// What the post is about to send. Coordinates are in screen points when `screen` is set.
#[derive(Clone, Debug)]
pub struct PostView {
    /// HID tap, so the front app window must be the admitted window.
    pub foreground: bool,
    /// Pointer or scroll. Keys leave this false.
    pub coordinate: bool,
    /// Target pid from the event. Negative values cannot match an owner.
    pub pid: i32,
    /// `CGWindowID` the event would carry.
    pub window_id: u32,
    /// Screen point already in points.
    pub screen: Option<(f64, f64)>,
    /// Window-local point in points, added to the fresh window origin.
    pub window_local: Option<(f64, f64)>,
}

/// The post has not started. Callers must not send the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryFault {
    /// Nothing in this event was handed to the OS.
    NotDelivered,
}

/// Re-check the admitted window against the rows just observed.
///
/// Foreground posts require the front app window's id and owner. Coordinate
/// posts require the topmost visible row under the point to be that window.
/// A miss does not post.
pub fn prove_post(
    admitted: &WindowBinding,
    rows: &[OsWindow],
    fresh: Option<&WindowBinding>,
    post: &PostView,
) -> Result<(), DeliveryFault> {
    let Some(fresh) = fresh else {
        return Err(DeliveryFault::NotDelivered);
    };
    if fresh != admitted || admitted.native_id() != post.window_id.to_string() {
        return Err(DeliveryFault::NotDelivered);
    }
    let pid = u32::try_from(post.pid).map_err(|_| DeliveryFault::NotDelivered)?;
    if pid != admitted.owner().pid() {
        return Err(DeliveryFault::NotDelivered);
    }
    if post.foreground {
        let Some(front) = front_app(rows) else {
            return Err(DeliveryFault::NotDelivered);
        };
        if front.cg_id != post.window_id || front.owner_pid != pid {
            return Err(DeliveryFault::NotDelivered);
        }
    }
    if post.coordinate {
        let Some(row) = rows.iter().find(|row| row.cg_id == post.window_id) else {
            return Err(DeliveryFault::NotDelivered);
        };
        let Some((x, y)) = point_of(post, row) else {
            return Err(DeliveryFault::NotDelivered);
        };
        if !contains(row.frame, x, y) {
            return Err(DeliveryFault::NotDelivered);
        }
        let Some(hit) = hit_top(rows, x, y) else {
            return Err(DeliveryFault::NotDelivered);
        };
        if hit.cg_id != post.window_id || hit.owner_pid != pid {
            return Err(DeliveryFault::NotDelivered);
        }
    }
    Ok(())
}

fn point_of(post: &PostView, row: &OsWindow) -> Option<(f64, f64)> {
    if let Some(screen) = post.screen {
        return Some(screen);
    }
    let (x, y) = post.window_local?;
    Some((row.frame.x + x, row.frame.y + y))
}

/// Front-to-back. The first visible row that contains the point is the hit.
fn hit_top(rows: &[OsWindow], x: f64, y: f64) -> Option<&OsWindow> {
    rows.iter().find(|window| {
        window.on_screen
            && window.alpha > 0.01
            && window.cg_id > 0
            && window.owner_pid > 0
            && contains(window.frame, x, y)
    })
}
