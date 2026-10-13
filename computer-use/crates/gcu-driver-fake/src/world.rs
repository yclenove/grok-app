//! Memory desktop and action effects. No OS input, policy, snapshot index, or retry.

use crate::{
    faults::Fault,
    scene::{self, Handle, Scene},
};
use gcu_core::{driver::*, model::*};
use gcu_protocol::keys::{Key, Modifier, NamedKey};
use std::collections::{BTreeMap, VecDeque};
use tokio::{sync::mpsc, time::Instant};
#[path = "world_nodes.rs"]
mod nodes;
use nodes::*;
pub(crate) use nodes::{find, focused};

/// External fixture oracle, separate from driver observations and rendering.
#[derive(Debug, Clone, Default)]
pub struct Oracle {
    /// Button delivery counts keyed by node identifier.
    pub presses: BTreeMap<String, u64>,
    /// Live text values keyed by node identifier.
    pub text: BTreeMap<String, String>,
    /// Numeric slider values keyed by node identifier.
    pub numbers: BTreeMap<String, f64>,
    /// Scroll offset in rows keyed by scroll-container identifier.
    pub scroll_rows: BTreeMap<String, i64>,
    /// Explicitly activated window, if any.
    pub foreground: Option<String>,
    /// Simulated held input after an interrupted chord.
    pub held_input: usize,
    /// Failed delayed effects, reported rather than silently discarded.
    pub deferred_errors: Vec<DriverError>,
}
#[derive(Clone)]
pub(crate) struct DesktopWindow {
    pub reference: WindowRef,
    pub info: WindowInfo,
    pub root: Option<RawNode>,
}
pub(crate) struct World {
    pub id: u64,
    pub apps: Vec<AppRef>,
    pub windows: Vec<DesktopWindow>,
    pub faults: VecDeque<Fault>,
    pub oracle: Oracle,
    pub pending: VecDeque<(Instant, DriverAction, Delivery)>,
    pub observers: Vec<(String, mpsc::UnboundedSender<UiEvent>)>,
}
impl World {
    pub fn new(scene: Scene, id: u64) -> Self {
        let apps: Vec<_> = scene.apps.iter().map(scene::app_ref).collect();
        let mut windows = Vec::new();
        for (app, reference) in scene.apps.iter().zip(&apps) {
            for win in &app.windows {
                windows.push(DesktopWindow {
                    reference: scene::window_ref(reference, win),
                    info: scene::window_info(win),
                    root: win
                        .root
                        .as_ref()
                        .map(|root| scene::raw_node(root, id, &win.id, 0)),
                });
            }
        }
        let mut world = Self {
            id,
            apps,
            windows,
            faults: VecDeque::new(),
            oracle: Oracle::default(),
            pending: VecDeque::new(),
            observers: Vec::new(),
        };
        for window in &mut world.windows {
            if let Some(root) = &mut window.root {
                layout_lists(root, 0);
            }
        }
        world
    }
    pub fn app(&self, app: &AppRef) -> Result<&AppRef> {
        self.apps
            .iter()
            .find(|candidate| *candidate == app)
            .ok_or(DriverError::AppNotFound)
    }
    pub fn window(&self, win: &WindowRef) -> Result<&DesktopWindow> {
        self.windows
            .iter()
            .find(|w| &w.reference == win)
            .ok_or(DriverError::WindowGone)
    }
    pub fn window_mut(&mut self, win: &WindowRef) -> Result<&mut DesktopWindow> {
        self.windows
            .iter_mut()
            .find(|w| &w.reference == win)
            .ok_or(DriverError::WindowGone)
    }
    pub fn handle(&self, el: &NativeHandle) -> Result<Handle> {
        let handle = el
            .downcast_ref::<Handle>()
            .ok_or(DriverError::StaleElement)?;
        if handle.world != self.id {
            return Err(DriverError::StaleElement);
        }
        let window = self
            .windows
            .iter()
            .find(|w| w.info.id == handle.window)
            .ok_or(DriverError::WindowGone)?;
        let root = window.root.as_ref().ok_or(DriverError::StaleElement)?;
        find(root, &handle.id).ok_or(DriverError::StaleElement)?;
        Ok(handle.clone())
    }
    fn node(&mut self, handle: &Handle) -> Result<&mut RawNode> {
        let window = self
            .windows
            .iter_mut()
            .find(|w| w.info.id == handle.window)
            .ok_or(DriverError::WindowGone)?;
        find_mut(
            window.root.as_mut().ok_or(DriverError::StaleElement)?,
            &handle.id,
        )
        .ok_or(DriverError::StaleElement)
    }
    pub fn emit(&mut self, window: &str, event: UiEvent) {
        self.observers.retain(|(id, sender)| {
            if sender.is_closed() {
                return false;
            }
            if id != window {
                return true;
            }
            let sent = sender.send(event.clone()).is_ok();
            sent && event != UiEvent::WindowGone
        });
    }
    pub fn structural_fault(&mut self, fault: &Fault) -> Result<()> {
        match fault {
            Fault::WindowGone { window } => {
                let index = self
                    .windows
                    .iter()
                    .position(|w| &w.info.id == window)
                    .ok_or(DriverError::WindowGone)?;
                self.windows.remove(index);
                self.emit(window, UiEvent::WindowGone);
            }
            Fault::GeometryChanged { window, geometry } => {
                if !scene::valid_rect(geometry.frame)
                    || geometry.frame.w < 1.0
                    || geometry.frame.h < 1.0
                    || geometry.frame.w > 4096.0
                    || geometry.frame.h > 4096.0
                    || geometry.frame.w.fract() != 0.0
                    || geometry.frame.h.fract() != 0.0
                    || !geometry.scale.is_finite()
                    || geometry.scale <= 0.0
                {
                    return Err(DriverError::InvalidRequest);
                }
                let win = self
                    .windows
                    .iter_mut()
                    .find(|w| &w.info.id == window)
                    .ok_or(DriverError::WindowGone)?;
                win.info.frame = geometry.frame;
                win.info.scale = geometry.scale;
                if let Some(root) = &mut win.root {
                    root.frame = Some(Rect {
                        x: 0.0,
                        y: 0.0,
                        w: geometry.frame.w,
                        h: geometry.frame.h,
                    });
                }
                self.emit(window, UiEvent::GeometryChanged);
            }
            Fault::ElementDeleted { window, identifier } => {
                let win = self
                    .windows
                    .iter_mut()
                    .find(|w| &w.info.id == window)
                    .ok_or(DriverError::WindowGone)?;
                let root = win.root.as_mut().ok_or(DriverError::StaleElement)?;
                if root.identifier.as_ref() == Some(identifier) {
                    win.root = None;
                } else if !remove(root, identifier) {
                    return Err(DriverError::StaleElement);
                }
                self.emit(window, UiEvent::Changed);
            }
            _ => {}
        }
        Ok(())
    }
    pub fn flush(&mut self) {
        let now = Instant::now();
        let mut remaining = VecDeque::new();
        while let Some((at, action, delivery)) = self.pending.pop_front() {
            if at <= now {
                if let Err(error) = self.apply(&action, delivery) {
                    self.oracle.deferred_errors.push(error);
                }
            } else {
                remaining.push_back((at, action, delivery));
            }
        }
        self.pending = remaining;
    }
    pub fn report(&self) -> Oracle {
        let mut oracle = self.oracle.clone();
        for window in &self.windows {
            if let Some(root) = &window.root {
                collect_values(root, &mut oracle);
            }
        }
        oracle
    }
    pub fn apply(&mut self, req: &DriverAction, delivery: Delivery) -> Result<()> {
        let window;
        match req {
            DriverAction::Press { el } => {
                let h = self.handle(el)?;
                window = h.window.clone();
                self.press(&h)?;
            }
            DriverAction::SecondaryAction { el, action } => {
                let h = self.handle(el)?;
                window = h.window.clone();
                let node = self.node(&h)?;
                actionable(node)?;
                if !node.actions.contains(action) {
                    return Err(DriverError::ElementNotActionable);
                }
                match action {
                    NodeAction::Press | NodeAction::Confirm => self.press(&h)?,
                    NodeAction::Toggle => {
                        node.states.toggle(States::CHECKED);
                        node.value = Some(NodeValue::Bool(node.states.contains(States::CHECKED)));
                    }
                    NodeAction::Select => {
                        node.states.insert(States::SELECTED);
                    }
                    NodeAction::Expand => node.states.insert(States::EXPANDED),
                    NodeAction::Collapse => node.states.remove(States::EXPANDED),
                    NodeAction::Increment | NodeAction::Decrement => {
                        let sign = if *action == NodeAction::Increment {
                            1.0
                        } else {
                            -1.0
                        };
                        match &mut node.value {
                            Some(NodeValue::Range { value, min, max }) => {
                                *value = (*value + sign).clamp(*min, *max)
                            }
                            _ => return Err(DriverError::NotSettable),
                        }
                    }
                    NodeAction::ScrollUp => self.scroll(&h, Dir::Up, 1.0)?,
                    NodeAction::ScrollDown => self.scroll(&h, Dir::Down, 1.0)?,
                    NodeAction::ScrollLeft => self.scroll(&h, Dir::Left, 1.0)?,
                    NodeAction::ScrollRight => self.scroll(&h, Dir::Right, 1.0)?,
                    _ => return Err(DriverError::CapabilityUnavailable),
                }
            }
            DriverAction::SetValue { el, value } => {
                let h = self.handle(el)?;
                window = h.window.clone();
                let node = self.node(&h)?;
                settable(node)?;
                node.value = Some(match &node.value {
                    Some(NodeValue::Range { min, max, .. }) => {
                        let n: f64 = value.parse().map_err(|_| DriverError::InvalidRequest)?;
                        if !n.is_finite() || n < *min || n > *max {
                            return Err(DriverError::InvalidRequest);
                        }
                        NodeValue::Range {
                            value: n,
                            min: *min,
                            max: *max,
                        }
                    }
                    Some(NodeValue::Number(_)) => {
                        let n: f64 = value.parse().map_err(|_| DriverError::InvalidRequest)?;
                        if !n.is_finite() {
                            return Err(DriverError::InvalidRequest);
                        }
                        NodeValue::Number(n)
                    }
                    _ => NodeValue::Text(value.clone()),
                });
                node.text_extras = None;
            }
            DriverAction::SelectText { el, range } => {
                let h = self.handle(el)?;
                window = h.window.clone();
                let node = self.node(&h)?;
                settable(node)?;
                let len = text(node)?.chars().count();
                if range
                    .start
                    .checked_add(range.length)
                    .is_none_or(|end| end > len)
                {
                    return Err(DriverError::InvalidRequest);
                }
                node.text_extras
                    .get_or_insert_with(TextExtras::default)
                    .selected = Some(*range);
            }
            DriverAction::ScrollElement { el, dir, pages } => {
                let h = self.handle(el)?;
                window = h.window.clone();
                self.scroll(&h, *dir, *pages)?;
            }
            DriverAction::PointerClick {
                win,
                at,
                button,
                count,
                ..
            } => {
                if *button != Button::Left || *count == 0 || *count > 3 {
                    return Err(DriverError::InvalidRequest);
                }
                let h = self.hit(win, *at)?;
                window = win.id.clone();
                let node = self.node(&h)?;
                if node.states.contains(States::FOCUSABLE) {
                    settable(node)?;
                    self.focus(win, &h)?;
                } else {
                    for _ in 0..*count {
                        self.press(&h)?;
                    }
                }
            }
            DriverAction::PointerScroll {
                win, at, dx, dy, ..
            } => {
                if *dx == 0 && *dy == 0 {
                    return Err(DriverError::InvalidRequest);
                }
                let h = self.scroll_hit(win, *at)?;
                window = win.id.clone();
                let delta = if *dy != 0 { *dy } else { *dx };
                let dir = if *dy > 0 {
                    Dir::Down
                } else if *dy < 0 {
                    Dir::Up
                } else if *dx > 0 {
                    Dir::Right
                } else {
                    Dir::Left
                };
                self.scroll(&h, dir, delta.unsigned_abs() as f32 / 360.0)?;
            }
            DriverAction::PointerDrag { win, path, .. } => {
                if path.len() < 2 {
                    return Err(DriverError::InvalidRequest);
                }
                for point in path {
                    self.bounds(win, *point)?;
                }
                let h = self.hit(win, path[0])?;
                window = win.id.clone();
                let node = self.node(&h)?;
                settable(node)?;
                let frame = node.frame.ok_or(DriverError::ElementNotActionable)?;
                if frame.w <= 0.0 {
                    return Err(DriverError::InvalidRequest);
                }
                let last = path[path.len() - 1];
                match &mut node.value {
                    Some(NodeValue::Range { value, min, max }) => {
                        *value =
                            *min + (last.x - frame.x).clamp(0.0, frame.w) / frame.w * (*max - *min)
                    }
                    _ => return Err(DriverError::NotSettable),
                }
            }
            DriverAction::TypeText {
                win,
                focus,
                text: input,
                ..
            } => {
                self.window(win)?;
                let h = match focus {
                    Some(el) => self.handle(el)?,
                    None => self.focused(win)?,
                };
                if h.window != win.id {
                    return Err(DriverError::StaleElement);
                }
                settable(self.node(&h)?)?;
                text(self.node(&h)?)?;
                window = win.id.clone();
                self.focus(win, &h)?;
                let node = self.node(&h)?;
                let previous = text(node)?.to_owned();
                let selection = node.text_extras.as_ref().and_then(|extra| extra.selected);
                node.value = Some(NodeValue::Text(replace_selection(
                    &previous, selection, input,
                )));
                node.text_extras = None;
            }
            DriverAction::PressKeys { win, chord, .. } => {
                let h = self.focused(win)?;
                let node = self.node(&h)?;
                settable(node)?;
                let previous = text(node)?.to_owned();
                window = win.id.clone();
                match &chord.key {
                    Key::Character('a' | 'A')
                        if chord.modifiers.contains(&Modifier::Ctrl)
                            || chord.modifiers.contains(&Modifier::Cmd) =>
                    {
                        node.text_extras
                            .get_or_insert_with(TextExtras::default)
                            .selected = Some(TextRange {
                            start: 0,
                            length: previous.chars().count(),
                        });
                    }
                    Key::Named(NamedKey::BackSpace | NamedKey::Delete)
                        if chord.modifiers.is_empty() =>
                    {
                        let selection = node.text_extras.as_ref().and_then(|extra| extra.selected);
                        let next = if selection.is_some_and(|r| r.length > 0) {
                            replace_selection(&previous, selection, "")
                        } else if chord.key == Key::Named(NamedKey::BackSpace) {
                            let mut chars = previous.chars().collect::<Vec<_>>();
                            chars.pop();
                            chars.into_iter().collect()
                        } else {
                            previous
                        };
                        node.value = Some(NodeValue::Text(next));
                        node.text_extras = None;
                    }
                    _ => return Err(DriverError::CapabilityUnavailable),
                }
            }
            DriverAction::Activate { win } => {
                self.window(win)?;
                window = win.id.clone();
            }
        }
        if delivery == Delivery::Foreground {
            self.oracle.foreground = Some(window.clone());
        }
        self.emit(&window, UiEvent::Changed);
        Ok(())
    }
    fn press(&mut self, h: &Handle) -> Result<()> {
        let node = self.node(h)?;
        actionable(node)?;
        if !node.actions.contains(&NodeAction::Press)
            && !node.actions.contains(&NodeAction::Confirm)
        {
            return Err(DriverError::ElementNotActionable);
        }
        let count = self.oracle.presses.entry(h.id.clone()).or_default();
        *count += 1;
        let count = *count;
        self.node(h)?.value = Some(NodeValue::Number(count as f64));
        Ok(())
    }
    fn scroll(&mut self, h: &Handle, dir: Dir, pages: f32) -> Result<()> {
        if !pages.is_finite() || pages <= 0.0 {
            return Err(DriverError::InvalidRequest);
        }
        let node = self.node(h)?;
        actionable(node)?;
        if !matches!(
            node.role,
            Role::List | Role::ScrollArea | Role::Table | Role::Outline | Role::Tree
        ) {
            return Err(DriverError::ElementNotActionable);
        }
        if matches!(dir, Dir::Left | Dir::Right) {
            return Err(DriverError::CapabilityUnavailable);
        }
        let visible = node
            .frame
            .map_or(15, |r| (r.h / 24.0).floor() as i64)
            .max(1);
        let max = (node.children.len() as i64 - visible).max(0);
        let offset = self.oracle.scroll_rows.entry(h.id.clone()).or_default();
        let rows = (pages * visible as f32).round() as i64;
        *offset = (if dir == Dir::Up {
            offset.saturating_sub(rows)
        } else {
            offset.saturating_add(rows)
        })
        .clamp(0, max);
        let offset = *offset;
        layout_lists(self.node(h)?, offset);
        Ok(())
    }
    pub fn bounds(&self, win: &WindowRef, point: WindowPoint) -> Result<()> {
        let frame = self.window(win)?.info.frame;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < 0.0
            || point.y < 0.0
            || point.x >= frame.w
            || point.y >= frame.h
        {
            return Err(DriverError::OutOfBounds);
        }
        Ok(())
    }
    fn hit(&self, win: &WindowRef, point: WindowPoint) -> Result<Handle> {
        self.bounds(win, point)?;
        let root = self
            .window(win)?
            .root
            .as_ref()
            .ok_or(DriverError::ElementNotActionable)?;
        let root = modal(root).unwrap_or(root);
        let node = hit(root, point).ok_or(DriverError::ElementNotActionable)?;
        self.handle(&node.handle)
    }
    fn scroll_hit(&self, win: &WindowRef, point: WindowPoint) -> Result<Handle> {
        self.bounds(win, point)?;
        let root = self
            .window(win)?
            .root
            .as_ref()
            .ok_or(DriverError::ElementNotActionable)?;
        let root = modal(root).unwrap_or(root);
        let node = scroll_hit(root, point).ok_or(DriverError::ElementNotActionable)?;
        self.handle(&node.handle)
    }
    pub fn focused(&self, win: &WindowRef) -> Result<Handle> {
        let root = self
            .window(win)?
            .root
            .as_ref()
            .ok_or(DriverError::ElementNotActionable)?;
        let node = focused(root).ok_or(DriverError::ElementNotActionable)?;
        self.handle(&node.handle)
    }
    fn focus(&mut self, win: &WindowRef, h: &Handle) -> Result<()> {
        let root = self
            .window_mut(win)?
            .root
            .as_mut()
            .ok_or(DriverError::StaleElement)?;
        set_focus(root, &h.id);
        Ok(())
    }
}
