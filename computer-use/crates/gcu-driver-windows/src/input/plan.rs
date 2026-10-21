//! Validate whole requests before any activation or input.
use super::{api::Event, messages};
use crate::api::NativeWindow;
use gcu_core::{
    driver::*,
    model::{NativeHandle, WindowPoint},
};
use gcu_protocol::Rect;
/// Pause between the approach move and the target move of foreground pointer input.
pub(super) const APPROACH_DWELL: std::time::Duration = std::time::Duration::from_millis(30);
pub(super) struct Plan {
    pub point: Option<WindowPoint>,
    pub focus: Option<NativeHandle>,
    pub foreground: bool,
    pub packets: Vec<Vec<Event>>,
    /// Sleep `APPROACH_DWELL` after sending this packet.
    pub dwell_after: Option<usize>,
    pub chord: Option<KeyChord>,
    pub text: bool,
}
/// A point one physical pixel from `p`, kept inside `frame` (screen coordinates).
///
/// Real-hardware finding (click-matrix4, 2026-10-21): when the pointer arrives from outside the
/// window, egui/winit ignores a click unless an earlier in-window move preceded the move onto the
/// target. A single absolute move, a repeat at the same point, a relative nudge or `SetCursorPos`
/// all left clicks dead; a move to a neighbouring pixel, a short dwell, then the target worked.
pub(super) fn approach_point(frame: &Rect, p: WindowPoint) -> WindowPoint {
    let step = |v: f64, lo: f64, len: f64| if v + 1. < lo + len { v + 1. } else { v - 1. };
    WindowPoint {
        x: step(p.x, frame.x, frame.w),
        y: step(p.y, frame.y, frame.h),
    }
}
pub(super) fn prepare(window: &NativeWindow, req: &DriverAction) -> Result<Plan> {
    let mut plan = Plan {
        point: None,
        focus: None,
        foreground: false,
        packets: Vec::new(),
        dwell_after: None,
        chord: None,
        text: false,
    };
    let screen = |p: WindowPoint| -> Result<WindowPoint> {
        if !p.x.is_finite()
            || !p.y.is_finite()
            || p.x < 0.
            || p.y < 0.
            || p.x >= window.frame.w
            || p.y >= window.frame.h
        {
            return Err(DriverError::OutOfBounds);
        }
        Ok(WindowPoint {
            x: window.frame.x + p.x,
            y: window.frame.y + p.y,
        })
    };
    match req {
        DriverAction::PointerClick {
            at,
            button,
            count,
            route,
            ..
        } => {
            if !(1..=3).contains(count) {
                return Err(DriverError::InvalidRequest);
            }
            let p = screen(*at)?;
            plan.point = Some(p);
            plan.foreground = *route == PointerRoute::AllowForeground;
            if plan.foreground {
                plan.packets
                    .push(vec![Event::Move(approach_point(&window.frame, p), false)]);
                plan.dwell_after = Some(0);
            }
            plan.packets.push(vec![Event::Move(p, false)]);
            for n in 0..*count {
                plan.packets.push(vec![
                    Event::Button(*button, true, p, n == 1),
                    Event::Button(*button, false, p, false),
                ]);
            }
        }
        DriverAction::PointerScroll {
            at, dx, dy, route, ..
        } => {
            let p = screen(*at)?;
            plan.point = Some(p);
            plan.foreground = *route == PointerRoute::AllowForeground;
            // Keep wheel messages within signed 16-bit Win32 deltas and avoid negating MIN.
            if dx.unsigned_abs() > 32767 || dy.unsigned_abs() > 32767 {
                return Err(DriverError::InvalidRequest);
            }
            plan.packets
                .push(vec![Event::Move(p, false), Event::Wheel(*dx, *dy, p)]);
        }
        DriverAction::PointerDrag { path, route, .. } => {
            let path: Vec<_> = path.iter().map(|p| screen(*p)).collect::<Result<_>>()?;
            let events = messages::drag_events(&path)?;
            plan.point = Some(path[0]);
            plan.foreground = *route == PointerRoute::AllowForeground;
            plan.packets = events.into_iter().map(|e| vec![e]).collect();
            if plan.foreground {
                plan.packets.insert(
                    0,
                    vec![Event::Move(approach_point(&window.frame, path[0]), false)],
                );
                plan.dwell_after = Some(0);
            }
        }
        DriverAction::TypeText {
            win,
            focus,
            text,
            route,
        } => {
            if let Some(handle) = focus {
                if !handle
                    .downcast_ref::<crate::uia::UiaElement>()
                    .is_some_and(|el| &el.window == win)
                {
                    return Err(DriverError::StaleElement);
                }
            }
            plan.packets = super::text::chunks(text)?;
            plan.focus = focus.clone();
            plan.foreground = *route == KeyRoute::AllowForeground;
            plan.text = true;
        }
        DriverAction::PressKeys { chord, route, .. } => {
            plan.chord = Some(chord.clone());
            plan.foreground = *route == KeyRoute::AllowForeground;
        }
        DriverAction::Activate { .. } => plan.foreground = true,
        _ => return Err(DriverError::CapabilityUnavailable),
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Rect {
        Rect {
            x: 100.,
            y: 50.,
            w: 300.,
            h: 200.,
        }
    }
    #[test]
    fn approach_is_one_pixel_away_and_stays_inside_the_frame() {
        let f = frame();
        let a = approach_point(&f, WindowPoint { x: 150., y: 80. });
        assert_eq!((a.x, a.y), (151., 81.));
        let edge = approach_point(&f, WindowPoint { x: 399.5, y: 249.5 });
        assert_eq!((edge.x, edge.y), (398.5, 248.5));
        let corner = approach_point(&f, WindowPoint { x: 100., y: 50. });
        assert_eq!((corner.x, corner.y), (101., 51.));
    }
}
