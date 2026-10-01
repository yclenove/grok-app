use crate::TargetInfo;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt, EventMask, MapState, Window};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

pub(crate) fn err(e: impl std::fmt::Display) -> String {
    format!("X11: {e}")
}

#[derive(Clone)]
struct Target {
    id: String,
    stamp: u64,
    epoch: u64,
    ancestry: Vec<Window>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub window: Window,
    pub root: Window,
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
    pub root_width: u16,
    pub root_height: u16,
    pub visual: u32,
    pub depth: u8,
    epoch: u64,
    stamp: u64,
}

impl Geometry {
    pub fn revision(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        h.finish().max(1)
    }
    pub fn point(&self, x: f64, y: f64) -> Result<(i16, i16), String> {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(self.width)
            || y >= f64::from(self.height)
        {
            return Err("point is outside the observed target image".into());
        }
        let x = i32::from(self.x) + x.floor() as i32;
        let y = i32::from(self.y) + y.floor() as i32;
        Ok((
            i16::try_from(x).map_err(err)?,
            i16::try_from(y).map_err(err)?,
        ))
    }
}

pub(crate) struct Client {
    pub conn: Arc<RustConnection>,
    pub root: Window,
    targets: HashMap<Window, Target>,
    pub input_uncertain: bool,
}

impl Client {
    /// XRes reports the actual local resource owner. _NET_WM_PID is a writable
    /// client property and must not authorize access to another process's AX tree.
    pub fn owner_pid(&self, window: Window) -> Result<u32, String> {
        use x11rb::protocol::res::{ClientIdMask, ClientIdSpec, ConnectionExt as _};
        let ids = self
            .conn
            .res_query_client_ids(&[ClientIdSpec {
                client: window,
                mask: ClientIdMask::LOCAL_CLIENT_PID,
            }])
            .map_err(err)?
            .reply()
            .map_err(err)?
            .ids;
        let pids: Vec<_> = ids
            .iter()
            .filter(|id| id.spec.mask.contains(ClientIdMask::LOCAL_CLIENT_PID))
            .filter_map(|id| match id.value.as_slice() {
                [pid] if *pid > 0 => Some(*pid),
                _ => None,
            })
            .collect();
        match pids.as_slice() {
            [pid] => Ok(*pid),
            _ => Err("XRes did not prove one local process owner".into()),
        }
    }

    pub fn semantic_binding(&mut self, id: &str) -> Result<(Geometry, u32, String), String> {
        self.fenced(|c| {
            let geometry = c.geometry(id)?;
            Ok((
                geometry,
                c.owner_pid(geometry.window)?,
                c.title(geometry.window)?,
            ))
        })
    }

    pub fn connect() -> Result<Self, String> {
        let (conn, screen) = x11rb::connect(None).map_err(err)?;
        conn.xtest_get_version(2, 2)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let root = conn.setup().roots[screen].root;
        Ok(Self {
            conn: Arc::new(conn),
            root,
            targets: HashMap::new(),
            input_uncertain: false,
        })
    }

    pub fn fenced<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut fence = ServerFence::new(self.conn.clone())?;
        let result = f(self);
        let cleanup = fence.close();
        if cleanup.is_err() {
            self.input_uncertain = true;
        }
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(format!("server fence cleanup failed: {error}")),
            (Err(primary), Err(cleanup)) => {
                Err(format!("{primary}; server fence cleanup failed: {cleanup}"))
            }
        }
    }

    fn events(&mut self) -> Result<(), String> {
        // Round-trip before reading the queue, so already-processed destruction
        // cannot be missed just because its event has not reached our socket yet.
        self.conn
            .get_input_focus()
            .map_err(err)?
            .reply()
            .map_err(err)?;
        for _ in 0..4096 {
            let Some(event) = self.conn.poll_for_event().map_err(err)? else {
                return Ok(());
            };
            match event {
                Event::DestroyNotify(e) => {
                    self.targets.retain(|_, t| !t.ancestry.contains(&e.window));
                }
                Event::UnmapNotify(e) => {
                    self.targets.retain(|_, t| !t.ancestry.contains(&e.window));
                }
                Event::ReparentNotify(e) => {
                    self.targets.retain(|_, t| !t.ancestry.contains(&e.window));
                }
                Event::ConfigureNotify(e) => {
                    for target in self
                        .targets
                        .values_mut()
                        .filter(|t| t.ancestry.contains(&e.window))
                    {
                        target.epoch = target
                            .epoch
                            .checked_add(1)
                            .ok_or("window epoch exhausted")?;
                    }
                }
                Event::Error(e) => return Err(format!("X11 protocol error: {e:?}")),
                _ => {}
            }
        }
        Err("native event queue exceeded the bounded drain; retry observation".into())
    }

    fn atom(&self, name: &[u8]) -> Result<u32, String> {
        Ok(self
            .conn
            .intern_atom(false, name)
            .map_err(err)?
            .reply()
            .map_err(err)?
            .atom)
    }

    fn ancestry(&self, mut window: Window) -> Result<Vec<Window>, String> {
        let mut chain = Vec::new();
        for _ in 0..64 {
            if window == x11rb::NONE || chain.contains(&window) {
                return Err("invalid native window ancestry".into());
            }
            chain.push(window);
            if window == self.root {
                return Ok(chain);
            }
            window = self
                .conn
                .query_tree(window)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .parent;
        }
        Err("native window ancestry exceeds the supported depth".into())
    }

    fn title(&self, window: Window) -> Result<String, String> {
        for property in [
            self.atom(b"_NET_WM_NAME")?,
            u32::from(xproto::AtomEnum::WM_NAME),
        ] {
            let reply = self
                .conn
                .get_property(false, window, property, xproto::AtomEnum::ANY, 0, 1024)
                .map_err(err)?
                .reply()
                .map_err(err)?;
            if reply.format == 8 && !reply.value.is_empty() {
                return Ok(String::from_utf8_lossy(&reply.value)
                    .trim_end_matches('\0')
                    .to_owned());
            }
        }
        Ok(String::new())
    }

    pub fn list_targets(&mut self) -> Result<Vec<TargetInfo>, String> {
        self.fenced(|c| {
            c.events()?;
            let property = c
                .conn
                .get_property(
                    false,
                    c.root,
                    c.atom(b"_NET_CLIENT_LIST_STACKING")?,
                    xproto::AtomEnum::WINDOW,
                    0,
                    512,
                )
                .map_err(err)?
                .reply()
                .map_err(err)?;
            let mut windows = property
                .value32()
                .map(|v| v.collect::<Vec<_>>())
                .unwrap_or_default();
            if windows.is_empty() {
                windows = c
                    .conn
                    .query_tree(c.root)
                    .map_err(err)?
                    .reply()
                    .map_err(err)?
                    .children;
            }
            windows.truncate(512);
            let mut result = Vec::new();
            let mut watched = HashSet::new();
            for window in windows {
                let Ok(attrs) = c
                    .conn
                    .get_window_attributes(window)
                    .map_err(err)
                    .and_then(|v| v.reply().map_err(err))
                else {
                    continue;
                };
                if attrs.map_state != MapState::VIEWABLE || attrs.override_redirect {
                    continue;
                }
                let Ok(title) = c.title(window) else {
                    continue;
                };
                if title.is_empty() {
                    continue;
                }
                if !c.targets.contains_key(&window) && c.targets.len() >= 4096 {
                    return Err("native target registry is at capacity".into());
                }
                let ancestry = c.ancestry(window)?;
                for ancestor in &ancestry {
                    if watched.insert(*ancestor) {
                        c.conn
                            .change_window_attributes(
                                *ancestor,
                                &xproto::ChangeWindowAttributesAux::new()
                                    .event_mask(EventMask::STRUCTURE_NOTIFY),
                            )
                            .map_err(err)?
                            .check()
                            .map_err(err)?;
                    }
                }
                // A reparented/replaced ancestor must never transfer authority,
                // even if the client XID and its final screen position match.
                if c.targets
                    .get(&window)
                    .is_some_and(|t| t.ancestry != ancestry)
                {
                    c.targets.remove(&window);
                }
                let record = c
                    .targets
                    .entry(window)
                    .or_insert_with(|| {
                        let nonce = uuid::Uuid::new_v4();
                        Target {
                            id: format!("x11:{window}:{nonce}"),
                            stamp: nonce.as_u128() as u64,
                            epoch: 1,
                            ancestry,
                        }
                    })
                    .clone();
                result.push(TargetInfo {
                    target_id: record.id,
                    lifecycle_stamp: record.stamp,
                    title: title.clone(),
                    app_name: "X11".into(),
                    kind: "window".into(),
                    pid: None,
                    backend: "linux".into(),
                    execution_mode: "exclusive".into(),
                    replay_policy: "never".into(),
                    display_id: format!("x11:{}", c.root),
                    coordinate_space: "image-pixels".into(),
                    scope_label: format!("X11 window “{title}”"),
                });
            }
            Ok(result)
        })
    }

    pub fn geometry(&mut self, id: &str) -> Result<Geometry, String> {
        self.events()?;
        let (&window, record) = self
            .targets
            .iter()
            .find(|(_, t)| t.id == id)
            .ok_or("target lifetime is no longer current")?;
        let attrs = self
            .conn
            .get_window_attributes(window)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if attrs.map_state != MapState::VIEWABLE {
            return Err("target is not viewable".into());
        }
        let g = self
            .conn
            .get_geometry(window)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let root = self
            .conn
            .get_geometry(g.root)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        let origin = self
            .conn
            .translate_coordinates(window, g.root, 0, 0)
            .map_err(err)?
            .reply()
            .map_err(err)?;
        if !origin.same_screen || g.width == 0 || g.height == 0 {
            return Err("target geometry unavailable".into());
        }
        Ok(Geometry {
            window,
            root: g.root,
            x: origin.dst_x,
            y: origin.dst_y,
            width: g.width,
            height: g.height,
            root_width: root.width,
            root_height: root.height,
            visual: attrs.visual,
            depth: g.depth,
            epoch: record.epoch,
            stamp: record.stamp,
        })
    }

    pub fn foreground(&mut self, id: &str) -> Result<bool, String> {
        let g = self.geometry(id)?;
        let mut focus = self
            .conn
            .get_input_focus()
            .map_err(err)?
            .reply()
            .map_err(err)?
            .focus;
        for _ in 0..64 {
            if focus == g.window {
                return Ok(true);
            }
            if focus == x11rb::NONE || focus == 1 || focus == g.root {
                return Ok(false);
            }
            focus = self
                .conn
                .query_tree(focus)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .parent;
        }
        Err("focus ancestry exceeds the supported depth".into())
    }

    pub fn visible_point(&self, g: Geometry, point: (i16, i16)) -> Result<(), String> {
        let mut parent = g.root;
        let mut found = false;
        for _ in 0..64 {
            found |= parent == g.window;
            let hit = self
                .conn
                .translate_coordinates(g.root, parent, point.0, point.1)
                .map_err(err)?
                .reply()
                .map_err(err)?;
            if !hit.same_screen {
                break;
            }
            if hit.child == x11rb::NONE {
                return if found {
                    Ok(())
                } else {
                    Err("target is occluded at the input point".into())
                };
            }
            parent = hit.child;
        }
        Err("input hit test could not prove the directed target".into())
    }
}

pub(crate) struct ServerFence {
    conn: Arc<RustConnection>,
    active: bool,
}

impl ServerFence {
    pub(crate) fn new(conn: Arc<RustConnection>) -> Result<Self, String> {
        conn.grab_server().map_err(err)?.check().map_err(err)?;
        Ok(Self { conn, active: true })
    }
    pub(crate) fn close(&mut self) -> Result<(), String> {
        self.conn
            .ungrab_server()
            .map_err(err)?
            .check()
            .map_err(err)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for ServerFence {
    fn drop(&mut self) {
        if self.active {
            // Also unblock the server on Rust unwinding; no native input is
            // synthesized by this cleanup. A failed barrier stays uncertain.
            let _ = self.conn.ungrab_server();
            let _ = self.conn.flush();
        }
    }
}
