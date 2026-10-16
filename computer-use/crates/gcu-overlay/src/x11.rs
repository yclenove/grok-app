//! Override-redirect ARGB windows. XI2 slave provenance, no core button/keys.
#[path = "x11/ffi.rs"]
mod ffi;
use crate::{
    desktop::{parts, Part, UserEvent},
    Error, Rect, Scene,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{
        randr::ConnectionExt as _,
        render::ConnectionExt as _,
        shape::{ConnectionExt as _, SK, SO},
        xinput::{self, ConnectionExt as _},
        xproto::{ConnectionExt as _, *},
        ErrorKind, Event,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

struct Window {
    id: u32,
    gc: u32,
    part: Part,
}
pub(crate) struct Surface {
    conn: Arc<RustConnection>,
    root: u32,
    visual: u32,
    colormap: u32,
    marker: u32,
    capture: u32,
    compositor: u32,
    host: u32,
    ack: u32,
    windows: Vec<Window>,
    physical: HashSet<u16>,
    down: HashMap<u16, (u32, u64)>,
    painter: ffi::Painter,
}
impl Surface {
    pub fn open() -> Result<Self, Error> {
        let (conn, screen_no) = x11rb::connect(None).map_err(Error::native)?;
        conn.xinput_xi_query_version(2, 0)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?;
        let screen = &conn.setup().roots[screen_no];
        let root = screen.root;
        // No compositor means no ARGB overlay. Never create opaque cursor boxes.
        let cm = conn
            .intern_atom(false, format!("_NET_WM_CM_S{screen_no}").as_bytes())
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .atom;
        if conn
            .get_selection_owner(cm)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .owner
            == 0
        {
            return Err(Error::native("X11 ARGB compositor unavailable"));
        }
        let visual = screen
            .allowed_depths
            .iter()
            .find(|d| d.depth == 32)
            .and_then(|d| {
                d.visuals.iter().find(|v| {
                    v.class == VisualClass::TRUE_COLOR
                        && v.red_mask == 0xff0000
                        && v.green_mask == 0xff00
                        && v.blue_mask == 0xff
                })
            })
            .ok_or_else(|| Error::native("X11 ARGB visual unavailable"))?
            .visual_id;
        let formats = conn
            .render_query_pict_formats()
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?;
        let alpha = formats
            .screens
            .iter()
            .flat_map(|s| &s.depths)
            .flat_map(|d| &d.visuals)
            .find(|v| v.visual == visual)
            .and_then(|v| formats.formats.iter().find(|f| f.id == v.format));
        if !alpha.is_some_and(|f| f.direct.alpha_mask == 255 && f.direct.alpha_shift == 24)
            || conn.setup().image_byte_order != ImageOrder::LSB_FIRST
            || !cfg!(target_endian = "little")
        {
            return Err(Error::native(
                "X11 premultiplied ARGB pixel format unavailable",
            ));
        }
        let colormap = conn.generate_id().map_err(Error::native)?;
        conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual)
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        let marker = conn
            .intern_atom(false, b"_GCU_OVERLAY")
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .atom;
        let capture = conn
            .intern_atom(false, b"_GCU_OVERLAY_CAPTURE")
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .atom;
        conn.xinput_xi_select_events(
            root,
            &[xinput::EventMask {
                deviceid: xinput::Device::ALL.into(),
                mask: vec![xinput::XIEventMask::HIERARCHY],
            }],
        )
        .map_err(Error::native)?
        .check()
        .map_err(Error::native)?;
        let ack = conn
            .intern_atom(false, b"_GCU_OVERLAY_ACK")
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .atom;
        let mut surface = Self {
            conn: Arc::new(conn),
            root,
            visual,
            colormap,
            marker,
            capture,
            compositor: cm,
            host: 0,
            ack,
            windows: Vec::new(),
            physical: HashSet::new(),
            down: HashMap::new(),
            painter: ffi::Painter::open()?,
        };
        surface.refresh_devices()?;
        surface.register_host()?;
        Ok(surface)
    }
    fn refresh_devices(&mut self) -> Result<(), Error> {
        let devices = self
            .conn
            .xinput_xi_query_device(xinput::Device::ALL)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?;
        self.physical = devices
            .infos
            .iter()
            .filter(|d| {
                d.enabled
                    && d.type_ == xinput::DeviceType::SLAVE_POINTER
                    && !String::from_utf8_lossy(&d.name)
                        .to_ascii_lowercase()
                        .contains("xtest")
            })
            .map(|d| d.deviceid)
            .collect();
        self.down.clear();
        if self.physical.is_empty() {
            return Err(Error::native(
                "XInput2 physical pointer provenance unavailable",
            ));
        }
        Ok(())
    }
    pub fn monitor(&self, target: Option<Rect>) -> Result<Rect, Error> {
        let monitors = self
            .conn
            .randr_get_monitors(self.root, true)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?;
        let point = target.map(|r| (r.x + r.w / 2., r.y + r.h / 2.));
        let chosen = point
            .and_then(|(x, y)| {
                monitors.monitors.iter().find(|m| {
                    x >= f64::from(m.x)
                        && y >= f64::from(m.y)
                        && x < f64::from(m.x) + f64::from(m.width)
                        && y < f64::from(m.y) + f64::from(m.height)
                })
            })
            .or_else(|| monitors.monitors.iter().find(|m| m.primary))
            .or_else(|| monitors.monitors.first())
            .ok_or_else(|| Error::native("RandR monitor unavailable"))?;
        Ok(Rect {
            x: chosen.x.into(),
            y: chosen.y.into(),
            w: chosen.width.into(),
            h: chosen.height.into(),
        })
    }
    pub fn events(&mut self) -> Result<Vec<UserEvent>, Error> {
        if self
            .conn
            .get_selection_owner(self.compositor)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .owner
            == 0
        {
            return Err(Error::native("X11 ARGB compositor lost"));
        }
        let mut out = Vec::new();
        for _ in 0..128 {
            let Some(event) = self.conn.poll_for_event().map_err(Error::native)? else {
                break;
            };
            match event {
                Event::XinputHierarchy(_) => self.refresh_devices()?,
                Event::XinputButtonPress(e)
                    if physical_button(e.response_type, e.detail, e.sourceid, &self.physical) =>
                {
                    if let Some(w) = self
                        .windows
                        .iter()
                        .find(|w| w.id == e.event && w.part.action.is_some())
                    {
                        self.down.insert(e.sourceid, (e.event, w.part.revision));
                    }
                }
                Event::XinputButtonRelease(e)
                    if physical_button(e.response_type, e.detail, e.sourceid, &self.physical) =>
                {
                    if let Some((id, revision)) = self.down.remove(&e.sourceid) {
                        if let Some(w) = self.windows.iter().find(|w| {
                            w.id == id
                                && id == e.event
                                && (w.part.action == Some(crate::ButtonAction::Stop)
                                    || w.part.revision == revision)
                        }) {
                            let x = f64::from(e.event_x) / 65536.;
                            let y = f64::from(e.event_y) / 65536.;
                            if x >= 0. && y >= 0. && x < w.part.frame.w && y < w.part.frame.h {
                                if let Some(action) = w.part.action {
                                    out.push(UserEvent {
                                        action,
                                        revision: w.part.revision,
                                    });
                                }
                            }
                        }
                    }
                }
                Event::Error(e) => return Err(Error::native(format!("{e:?}"))),
                _ => (),
            }
        }
        Ok(out)
    }
    pub fn render(&mut self, scene: &Scene) -> Result<(), Error> {
        let next = parts(scene);
        // Font work never holds the X server. An unresponsive UI cannot
        // acknowledge hiding, so the capture connection refuses its pixels.
        let pixels = next
            .iter()
            .map(|p| {
                self.painter.paint(
                    p,
                    p.frame.w.ceil().max(1.) as u16,
                    p.frame.h.ceil().max(1.) as u16,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let leases = self.capture_leases()?;
        self.render_transaction(next, pixels, leases)
    }
    fn render_transaction(
        &mut self,
        next: Vec<Part>,
        pixels: Vec<Vec<u8>>,
        leases: Vec<u32>,
    ) -> Result<(), Error> {
        let capturing = !leases.is_empty();
        // Reuse only equivalent input surfaces. Unmap before changing control identity.
        if self.windows.len() != next.len()
            || self
                .windows
                .iter()
                .zip(&next)
                .any(|(w, p)| w.part.action != p.action)
        {
            self.destroy();
            for part in next {
                self.create(part)?;
            }
        } else {
            for (w, part) in self.windows.iter_mut().zip(next) {
                w.part = part;
            }
        }
        for (w, pixels) in self.windows.iter().zip(pixels) {
            if capturing {
                self.conn.unmap_window(w.id).map_err(Error::native)?;
                continue;
            }
            let f = w.part.frame;
            self.conn
                .configure_window(
                    w.id,
                    &ConfigureWindowAux::new()
                        .x(f.x.round() as i32)
                        .y(f.y.round() as i32)
                        .width(f.w.ceil().max(1.) as u32)
                        .height(f.h.ceil().max(1.) as u32)
                        .stack_mode(StackMode::ABOVE),
                )
                .map_err(Error::native)?;
            let (width, height) = (f.w.ceil().max(1.) as u16, f.h.ceil().max(1.) as u16);
            self.conn
                .put_image(
                    ImageFormat::Z_PIXMAP,
                    w.id,
                    w.gc,
                    width,
                    height,
                    0,
                    0,
                    0,
                    32,
                    &pixels,
                )
                .map_err(Error::native)?;
            self.conn.map_window(w.id).map_err(Error::native)?;
        }
        // The checked property write is ordered after every unmap on this
        // connection. Never acknowledge a lease before its final hide request.
        self.conn
            .change_property32(
                PropMode::REPLACE,
                self.host,
                self.ack,
                AtomEnum::CARDINAL,
                &leases,
            )
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        self.conn.flush().map_err(Error::native)
    }
    fn register_host(&mut self) -> Result<(), Error> {
        let id = self.conn.generate_id().map_err(Error::native)?;
        self.conn
            .create_window(
                0,
                id,
                self.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::default(),
            )
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        self.host = id;
        let marker = self
            .conn
            .intern_atom(false, b"_GCU_OVERLAY_HOST")
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .atom;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                id,
                marker,
                AtomEnum::CARDINAL,
                &[std::process::id()],
            )
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        Ok(())
    }
    fn capture_leases(&self) -> Result<Vec<u32>, Error> {
        let mut proofs = Vec::new();
        for id in self
            .conn
            .query_tree(self.root)
            .map_err(Error::native)?
            .reply()
            .map_err(Error::native)?
            .children
        {
            // A popup may disappear after QueryTree. Only BadWindow is benign;
            // connection/protocol failures must still hide the overlay and Stop.
            let reply = match self
                .conn
                .get_property(false, id, self.capture, AtomEnum::CARDINAL, 0, 4)
                .map_err(Error::native)?
                .reply()
            {
                Ok(reply) => reply,
                Err(ReplyError::X11Error(e)) if e.error_kind == ErrorKind::Window => continue,
                Err(e) => return Err(Error::native(e)),
            };
            let nonce: Vec<_> = reply.value32().map(|v| v.collect()).unwrap_or_default();
            if nonce.len() == 4 {
                proofs.push(id);
                proofs.extend(nonce);
            }
            if proofs.len() > 64 * 5 {
                return Err(Error::native(
                    "too many simultaneous overlay capture leases",
                ));
            }
        }
        Ok(proofs)
    }
    fn create(&mut self, part: Part) -> Result<(), Error> {
        let id = self.conn.generate_id().map_err(Error::native)?;
        let gc = self.conn.generate_id().map_err(Error::native)?;
        self.conn
            .create_window(
                32,
                id,
                self.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                self.visual,
                &CreateWindowAux::new()
                    .override_redirect(1)
                    .background_pixel(0)
                    .border_pixel(0)
                    .colormap(self.colormap),
            )
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        // All windows start unmapped; the marker lets the independent X11 capture
        // connection request cooperative hiding, including on error paths.
        self.windows.push(Window { id, gc, part });
        let w = self
            .windows
            .last()
            .ok_or_else(|| Error::native("window allocation failed"))?;
        self.conn
            .change_property32(
                PropMode::REPLACE,
                id,
                self.marker,
                AtomEnum::CARDINAL,
                &[std::process::id()],
            )
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        // WM_HINTS input=False: no focus, no keyboard selection, no UIA/AT-SPI nodes.
        self.conn
            .change_property32(
                PropMode::REPLACE,
                id,
                AtomEnum::WM_HINTS,
                AtomEnum::WM_HINTS,
                &[1, 0, 0, 0, 0, 0, 0, 0, 0],
            )
            .map_err(Error::native)?;
        if w.part.action.is_none() {
            self.conn
                .shape_rectangles(SO::SET, SK::INPUT, ClipOrdering::UNSORTED, id, 0, 0, &[])
                .map_err(Error::native)?
                .check()
                .map_err(Error::native)?;
        } else {
            self.conn
                .xinput_xi_select_events(
                    id,
                    &[xinput::EventMask {
                        deviceid: xinput::Device::ALL_MASTER.into(),
                        mask: vec![
                            xinput::XIEventMask::BUTTON_PRESS | xinput::XIEventMask::BUTTON_RELEASE,
                        ],
                    }],
                )
                .map_err(Error::native)?
                .check()
                .map_err(Error::native)?;
        }
        self.conn
            .create_gc(gc, id, &CreateGCAux::default())
            .map_err(Error::native)?
            .check()
            .map_err(Error::native)?;
        Ok(())
    }
    pub fn hide(&mut self) {
        self.destroy();
    }
    fn destroy(&mut self) {
        self.down.clear();
        for w in self.windows.drain(..) {
            let _ = self.conn.free_gc(w.gc);
            let _ = self.conn.destroy_window(w.id);
        }
        let _ = self.conn.flush();
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        self.destroy();
        if self.host != 0 {
            let _ = self.conn.destroy_window(self.host);
        }
        let _ = self.conn.free_colormap(self.colormap);
        let _ = self.conn.flush();
    }
}
fn physical_button(response: u8, detail: u32, source: u16, physical: &HashSet<u16>) -> bool {
    response & 0x80 == 0 && detail == 1 && physical.contains(&source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synthetic_send_event_xtest_and_unknown_device_do_not_activate_controls() {
        let physical = HashSet::from([4]);
        assert!(!physical_button(0x80, 1, 4, &physical));
        assert!(!physical_button(0, 1, 3, &physical));
        assert!(!physical_button(0, 1, 99, &physical));
        assert!(physical_button(0, 1, 4, &physical));
    }
}
