//! Validate whole requests before any activation or input.
use super::{api::Event, messages};
use crate::api::NativeWindow;
use gcu_core::{
    driver::*,
    model::{NativeHandle, WindowPoint},
};
pub(super) struct Plan {
    pub point: Option<WindowPoint>,
    pub focus: Option<NativeHandle>,
    pub foreground: bool,
    pub packets: Vec<Vec<Event>>,
    pub chord: Option<KeyChord>,
    pub text: bool,
}
pub(super) fn prepare(window: &NativeWindow, req: &DriverAction) -> Result<Plan> {
    let mut plan = Plan {
        point: None,
        focus: None,
        foreground: false,
        packets: Vec::new(),
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
