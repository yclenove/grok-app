//! Independent Cocoa pointer postconditions, not a native event injector.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerKind {
    Down,
    Up,
    Drag,
    Wheel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerEvent {
    pub kind: PointerKind,
    pub button: u8,
    pub click_count: u32,
    pub x: f64,
    pub y: f64,
    pub delta_y: f64,
    pub flags: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PointerState {
    pub width: f64,
    pub height: f64,
    pub scroll_offset: f64,
    pub box_rect: [f64; 4],
    pub dragging: bool,
    pub overflow: bool,
    pub events: Vec<PointerEvent>,
}

impl PointerState {
    pub fn validate(&self) -> Result<(), String> {
        let [x, y, w, h] = self.box_rect;
        if ![self.width, self.height, self.scroll_offset, x, y, w, h]
            .iter()
            .all(|n| n.is_finite())
            || self.width <= 0.0
            || self.height <= 0.0
            || self.width > 4096.0
            || self.height > 4096.0
            || !(0.0..=1000.0).contains(&self.scroll_offset)
            || x < 0.0
            || y < 0.0
            || w <= 0.0
            || h <= 0.0
            || x + w > self.width
            || y + h > self.height
            || self.overflow
            || self.events.len() > 96
        {
            return Err("invalid or truncated owned pointer state".into());
        }
        for event in &self.events {
            if ![event.x, event.y, event.delta_y]
                .iter()
                .all(|n| n.is_finite())
                || !(0.0..=self.width).contains(&event.x)
                || !(0.0..=self.height).contains(&event.y)
                || event.delta_y.abs() > 10_000.0
                || event.button > 2
                || match event.kind {
                    PointerKind::Wheel => event.button != 0 || event.click_count != 0,
                    PointerKind::Down => {
                        !(1..=2).contains(&event.click_count) || event.delta_y != 0.0
                    }
                    PointerKind::Up => event.click_count > 2 || event.delta_y != 0.0,
                    PointerKind::Drag => event.click_count != 0 || event.delta_y != 0.0,
                }
            {
                return Err("invalid owned pointer event".into());
            }
        }
        Ok(())
    }

    pub fn fresh(&self) -> bool {
        self.validate().is_ok()
            && self.width == 660.0
            && self.height == 180.0
            && self.scroll_offset == 500.0
            && self.box_rect == [290.0, 70.0, 80.0, 40.0]
            && !self.dragging
            && self.events.is_empty()
    }
}

fn suffix<'a>(
    before: &PointerState,
    after: &'a PointerState,
) -> Result<&'a [PointerEvent], String> {
    before.validate()?;
    after.validate()?;
    if before.width != after.width
        || before.height != after.height
        || before.dragging
        || !after.events.starts_with(&before.events)
    {
        return Err("pointer identity, history or initial ownership changed".into());
    }
    let events = &after.events[before.events.len()..];
    if events.iter().any(|e| e.flags != 0) || after.dragging {
        return Err("pointer modifiers or unreleased drag remain".into());
    }
    Ok(events)
}

fn near(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1.0
}
fn position(event: &PointerEvent, state: &PointerState, point: [f64; 2]) -> bool {
    near(event.x, point[0] * state.width) && near(event.y, point[1] * state.height)
}
fn point_valid(point: [f64; 2]) -> bool {
    point
        .iter()
        .all(|v| v.is_finite() && (0.0..1.0).contains(v))
}

pub fn click(
    before: &PointerState,
    after: &PointerState,
    point: [f64; 2],
    button: u8,
    count: u32,
) -> Result<(), String> {
    let events = suffix(before, after)?;
    if !point_valid(point)
        || button > 2
        || !(1..=2).contains(&count)
        || events.len() != count as usize * 2
        || before.box_rect != after.box_rect
        || before.scroll_offset != after.scroll_offset
    {
        return Err("click changed unrelated pointer state or pair count".into());
    }
    for (index, event) in events.iter().enumerate() {
        let kind = if index % 2 == 0 {
            PointerKind::Down
        } else {
            PointerKind::Up
        };
        if event.kind != kind
            || event.button != button
            || event.click_count != (index / 2 + 1) as u32
            || !position(event, before, point)
        {
            return Err("owned view did not receive the exact click pairs".into());
        }
    }
    Ok(())
}

pub fn scroll(
    before: &PointerState,
    after: &PointerState,
    point: [f64; 2],
    delta: i32,
) -> Result<(), String> {
    let events = suffix(before, after)?;
    if !point_valid(point) {
        return Err("scroll proof has an invalid coordinate".into());
    }
    if delta == 0 {
        return if before == after {
            Ok(())
        } else {
            Err("zero scroll changed owned view".into())
        };
    }
    let [event] = events else {
        return Err("scroll must deliver one wheel event".into());
    };
    let expected = (before.scroll_offset + f64::from(delta)).clamp(0.0, 1000.0);
    if event.kind != PointerKind::Wheel
        || (event.delta_y + f64::from(delta)).abs() > 0.01
        || !position(event, before, point)
        || (after.scroll_offset - expected).abs() > 0.01
        || before.box_rect != after.box_rect
    {
        return Err("wheel direction, pixel amount or actual viewport effect mismatched".into());
    }
    Ok(())
}

pub fn drag(
    before: &PointerState,
    after: &PointerState,
    from: [f64; 2],
    to: [f64; 2],
) -> Result<(), String> {
    let events = suffix(before, after)?;
    if !point_valid(from)
        || !point_valid(to)
        || !(3..=18).contains(&events.len())
        || before.scroll_offset != after.scroll_offset
    {
        return Err("drag evidence is incomplete or changed the viewport".into());
    }
    let start = [from[0] * before.width, from[1] * before.height];
    let end = [to[0] * before.width, to[1] * before.height];
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let length = dx.hypot(dy);
    let [x, y, w, h] = before.box_rect;
    let expected = [x + dx, y + dy, w, h];
    if length < 1.0
        || !near(x + w / 2.0, start[0])
        || !near(y + h / 2.0, start[1])
        || after.box_rect[2..] != before.box_rect[2..]
        || !after.box_rect[..2]
            .iter()
            .zip(expected[..2].iter())
            .all(|(a, b)| near(*a, *b))
    {
        return Err(
            "drag did not move the owned box from its real center to the destination".into(),
        );
    }
    let mut progress = 0.0;
    for (i, event) in events.iter().enumerate() {
        let kind = if i == 0 {
            PointerKind::Down
        } else if i + 1 == events.len() {
            PointerKind::Up
        } else {
            PointerKind::Drag
        };
        let advance = ((event.x - start[0]) * dx + (event.y - start[1]) * dy) / length;
        let sideways = ((event.x - start[0]) * dy - (event.y - start[1]) * dx).abs() / length;
        if event.kind != kind
            || event.button != 0
            || match kind {
                PointerKind::Down => event.click_count != 1,
                PointerKind::Up => event.click_count > 1,
                PointerKind::Drag => event.click_count != 0,
                PointerKind::Wheel => true,
            }
            || advance < progress - 1.0
            || advance < -1.0
            || advance > length + 1.0
            || sideways > 1.0
        {
            return Err("drag native path, order or button identity mismatched".into());
        }
        progress = advance;
    }
    if !position(&events[0], before, from)
        || !position(&events[events.len() - 2], before, to)
        || !position(&events[events.len() - 1], before, to)
    {
        return Err("drag lacks an actual final motion and matching owned release".into());
    }
    // macOS may coalesce intermediate mouseDragged deliveries. Exact endpoints,
    // an ordered bounded path, paired release and the rendered box effect remain required.
    Ok(())
}
