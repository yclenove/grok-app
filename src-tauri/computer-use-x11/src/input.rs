use crate::client::{err, Client, Geometry};
use crate::DispatchRequest;
use grok_computer_use_core::protocol::{normalize_key, ActionKind, ActionTarget};
use x11rb::connection::Connection;
use x11rb::protocol::{xproto::ConnectionExt, xtest::ConnectionExt as _};

#[derive(Clone, Copy, Debug)]
struct Input {
    kind: u8,
    detail: u8,
    x: i16,
    y: i16,
}

impl Input {
    fn motion(point: (i16, i16)) -> Self {
        Self {
            kind: 6,
            detail: 0,
            x: point.0,
            y: point.1,
        }
    }
    fn button(kind: u8, detail: u8) -> Self {
        Self {
            kind,
            detail,
            x: 0,
            y: 0,
        }
    }
}

impl Client {
    pub fn require_released_input(&self) -> Result<(), String> {
        if self.input_uncertain {
            return Err("native completion is uncertain".into());
        }
        let pointer = self
            .conn
            .query_pointer(self.root)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let locks = self.lock_modifier_mask()?;
        if u16::from(pointer.mask) & 0x1fff & !locks != 0 {
            return Err("user holds native buttons or modifiers; input paused".into());
        }
        if self
            .conn
            .query_keymap()
            .map_err(err)?
            .reply()
            .map_err(err)?
            .keys
            .iter()
            .any(|b| *b != 0)
        {
            return Err("user holds a native key; input paused".into());
        }
        Ok(())
    }

    pub fn act(
        &mut self,
        req: &DispatchRequest,
        mut authority: impl FnMut() -> Result<(), String>,
    ) -> Result<bool, String> {
        if self.input_uncertain {
            return Err("earlier native input has not been proven quiescent".into());
        }
        self.fenced(|c| {
            authority()?;
            req.cancellation.check()?;
            let geometry = c.geometry(&req.target_id)?;
            if req.geometry_revision != geometry.revision() {
                return Err("stale geometryRevision".into());
            }
            if !c.foreground(&req.target_id)? {
                return Err("focus drifted; directed input paused".into());
            }
            c.require_released_input()?;
            let events = c.plan(req, geometry)?;
            let applied = !events.is_empty();
            let mut held = Vec::<(u8, u8)>::new();
            let mut last_pointer = None;
            let result = (|| {
                for event in events {
                    // Releases below are cleanup, not a new command, and must
                    // still happen if Stop arrives between press and release.
                    let release = event.kind == 3 || event.kind == 5;
                    if !release {
                        authority()?;
                        req.cancellation.check()?;
                    }
                    if let Some((x, y)) = last_pointer {
                        let pointer = c
                            .conn
                            .query_pointer(geometry.root)
                            .map_err(err)?
                            .reply()
                            .map_err(err)?;
                        if (pointer.root_x, pointer.root_y) != (x, y) && !release {
                            return Err("pointer moved independently; input paused".into());
                        }
                    }
                    if event.kind == 2 || event.kind == 4 {
                        held.push((event.kind + 1, event.detail));
                    }
                    c.input_uncertain = true;
                    c.send(geometry, event)?;
                    if release {
                        held.retain(|entry| *entry != (event.kind, event.detail));
                    }
                    if event.kind == 6 {
                        last_pointer = Some((event.x, event.y));
                    }
                }
                Ok(())
            })();
            let mut cleanup_error = None;
            for (kind, detail) in held.into_iter().rev() {
                if let Err(error) = c.send(geometry, Input::button(kind, detail)) {
                    cleanup_error = Some(error);
                }
            }
            let barrier = c
                .conn
                .get_input_focus()
                .map_err(err)
                .and_then(|r| r.reply().map_err(err));
            if cleanup_error.is_none() && barrier.is_ok() {
                c.input_uncertain = false;
            }
            if let Some(error) = cleanup_error {
                return Err(format!("native release failed: {error}; action={result:?}"));
            }
            barrier?;
            result.map(|()| applied)
        })
    }

    fn send(&self, g: Geometry, e: Input) -> Result<(), String> {
        self.conn
            .xtest_fake_input(e.kind, e.detail, 0, g.root, e.x, e.y, 0)
            .map_err(err)?
            .check()
            .map_err(err)
    }

    fn plan(&self, req: &DispatchRequest, geometry: Geometry) -> Result<Vec<Input>, String> {
        let ActionTarget::Coord { x, y } = req.target else {
            return Err("X11 semantic input requires AT-SPI; no coordinate fallback".into());
        };
        let point = geometry.point(x, y)?;
        self.visible_point(geometry, point)?;
        let mut result = vec![];
        match req.action {
            ActionKind::Click => {
                let button = match req
                    .parameters
                    .get("button")
                    .and_then(|v| v.as_str())
                    .unwrap_or("left")
                {
                    "left" => 1,
                    "middle" => 2,
                    "right" => 3,
                    _ => return Err("unsupported button".into()),
                };
                let count = req
                    .parameters
                    .get("count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1);
                if !(1..=2).contains(&count) {
                    return Err("unsupported click count".into());
                }
                result.push(Input::motion(point));
                for _ in 0..count {
                    result.extend([Input::button(4, button), Input::button(5, button)]);
                }
            }
            ActionKind::Scroll => {
                let delta = grok_computer_use_core::protocol::ScrollDelta::parse(&req.parameters)?;
                if delta.value() != 0 {
                    result.push(Input::motion(point));
                }
                // X11 wheel button 4 is UP; protocol positive is DOWN.
                let button = if delta.value() < 0 { 4 } else { 5 };
                for _ in 0..delta.value().unsigned_abs().div_ceil(120) {
                    result.extend([Input::button(4, button), Input::button(5, button)]);
                }
            }
            ActionKind::Drag => {
                let (end_x, end_y) =
                    grok_computer_use_core::protocol::drag_destination(&req.parameters)?;
                let end = geometry.point(end_x, end_y)?;
                result.extend([Input::motion(point), Input::button(4, 1)]);
                for step in 1..=16 {
                    let p = (
                        i32::from(point.0) + (i32::from(end.0) - i32::from(point.0)) * step / 16,
                        i32::from(point.1) + (i32::from(end.1) - i32::from(point.1)) * step / 16,
                    );
                    let p = (
                        i16::try_from(p.0).map_err(err)?,
                        i16::try_from(p.1).map_err(err)?,
                    );
                    self.visible_point(geometry, p)?;
                    result.push(Input::motion(p));
                }
                result.push(Input::button(5, 1));
            }
            ActionKind::Key => {
                let key = normalize_key(
                    req.parameters
                        .get("key")
                        .and_then(|v| v.as_str())
                        .ok_or("key required")?,
                )?;
                let symbol = match key {
                    "enter" => 0xff0d,
                    "tab" => 0xff09,
                    "escape" => 0xff1b,
                    "space" => 0x20,
                    "left" => 0xff51,
                    "up" => 0xff52,
                    "right" => 0xff53,
                    "down" => 0xff54,
                    "backspace" => 0xff08,
                    "delete" => 0xffff,
                    "home" => 0xff50,
                    "end" => 0xff57,
                    "pageup" => 0xff55,
                    "pagedown" => 0xff56,
                    _ => return Err("unsupported native key".into()),
                };
                let setup = self.conn.setup();
                let count = setup.max_keycode - setup.min_keycode + 1;
                let keys = self
                    .conn
                    .get_keyboard_mapping(setup.min_keycode, count)
                    .map_err(err)?
                    .reply()
                    .map_err(err)?;
                if keys.keysyms_per_keycode == 0 {
                    return Err("empty native keyboard mapping".into());
                }
                let index = keys
                    .keysyms
                    .chunks_exact(usize::from(keys.keysyms_per_keycode))
                    .position(|group| group[0] == symbol)
                    .ok_or("named key absent from unmodified native keymap")?;
                let code = setup
                    .min_keycode
                    .checked_add(u8::try_from(index).map_err(err)?)
                    .ok_or("keycode overflow")?;
                result.extend([Input::button(2, code), Input::button(3, code)]);
            }
            _ => {
                return Err(
                    "native semantic/text input is not yet implemented; no global fallback".into(),
                )
            }
        }
        Ok(result)
    }
}
