//! Live X11 server connection backed by `x11rb`.

use crate::x11::server::{
    zpixmap_to_rgba, RawImage, XError, XRootGeometry, XServer, XWindowGeometry,
};
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::composite::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageData, ClientMessageEvent, ConnectionExt as _, EventMask, ImageFormat,
    MapState,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// Real X11 server connection backed by `x11rb`.
pub struct RealX {
    conn: RustConnection,
    screen_num: usize,
    net_client_list: u32,
    net_wm_pid: u32,
    net_wm_name: u32,
    net_active_window: u32,
    utf8_string: u32,
}

impl RealX {
    /// Connect to the active X11 server using the standard `DISPLAY` environment.
    pub fn connect() -> Result<Self, XError> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| XError::Connection(e.to_string()))?;
        Self::from_connection(conn, screen_num)
    }

    /// Wrap an existing `x11rb` connection.
    pub fn from_connection(conn: RustConnection, screen_num: usize) -> Result<Self, XError> {
        let ncl = conn
            .intern_atom(false, b"_NET_CLIENT_LIST")
            .map_err(|e| XError::Protocol(e.to_string()))?;
        let nwp = conn
            .intern_atom(false, b"_NET_WM_PID")
            .map_err(|e| XError::Protocol(e.to_string()))?;
        let nwn = conn
            .intern_atom(false, b"_NET_WM_NAME")
            .map_err(|e| XError::Protocol(e.to_string()))?;
        let naw = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .map_err(|e| XError::Protocol(e.to_string()))?;
        let u8s = conn
            .intern_atom(false, b"UTF8_STRING")
            .map_err(|e| XError::Protocol(e.to_string()))?;

        let net_client_list = ncl
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?
            .atom;
        let net_wm_pid = nwp
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?
            .atom;
        let net_wm_name = nwn
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?
            .atom;
        let net_active_window = naw
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?
            .atom;
        let utf8_string = u8s
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?
            .atom;

        Ok(Self {
            conn,
            screen_num,
            net_client_list,
            net_wm_pid,
            net_wm_name,
            net_active_window,
            utf8_string,
        })
    }

    fn screen(&self) -> &x11rb::protocol::xproto::Screen {
        &self.conn.setup().roots[self.screen_num]
    }
}

impl XServer for RealX {
    fn client_list(&self) -> Result<Vec<u32>, XError> {
        let root = self.screen().root;
        let prop = self
            .conn
            .get_property(false, root, self.net_client_list, AtomEnum::WINDOW, 0, 4096)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        if prop.format == 32 {
            let windows = prop.value32().map(|it| it.collect()).unwrap_or_default();
            Ok(windows)
        } else {
            Ok(Vec::new())
        }
    }

    fn window_pid(&self, window: u32) -> Result<Option<u32>, XError> {
        let prop = self
            .conn
            .get_property(false, window, self.net_wm_pid, AtomEnum::CARDINAL, 0, 1)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        if prop.format == 32 {
            Ok(prop.value32().and_then(|mut it| it.next()))
        } else {
            Ok(None)
        }
    }

    fn window_name(&self, window: u32) -> Result<Option<String>, XError> {
        let prop = self
            .conn
            .get_property(false, window, self.net_wm_name, self.utf8_string, 0, 1024)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        if !prop.value.is_empty() {
            if let Ok(name) = String::from_utf8(prop.value) {
                if !name.is_empty() {
                    return Ok(Some(name));
                }
            }
        }

        let fallback = self
            .conn
            .get_property(false, window, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 1024)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        if !fallback.value.is_empty() {
            Ok(Some(String::from_utf8_lossy(&fallback.value).into_owned()))
        } else {
            Ok(None)
        }
    }

    fn active_window(&self) -> Result<Option<u32>, XError> {
        let root = self.screen().root;
        let prop = self
            .conn
            .get_property(false, root, self.net_active_window, AtomEnum::WINDOW, 0, 1)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        if prop.format == 32 {
            Ok(prop
                .value32()
                .and_then(|mut it| it.next())
                .filter(|&w| w != 0))
        } else {
            Ok(None)
        }
    }

    fn window_geometry(&self, window: u32) -> Result<XWindowGeometry, XError> {
        let root = self.screen().root;
        let geom = self
            .conn
            .get_geometry(window)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|_| XError::WindowNotFound(window))?;

        let coords = self
            .conn
            .translate_coordinates(window, root, 0, 0)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        Ok(XWindowGeometry {
            x: coords.dst_x as i32,
            y: coords.dst_y as i32,
            width: geom.width as u32,
            height: geom.height as u32,
            border_width: geom.border_width as u32,
            depth: geom.depth,
        })
    }

    fn root_geometry(&self) -> Result<XRootGeometry, XError> {
        let screen = self.screen();
        let width = screen.width_in_pixels as u32;
        let height = screen.height_in_pixels as u32;
        let scale = if screen.width_in_millimeters > 0 {
            let dpi = (width as f64 * 25.4) / (screen.width_in_millimeters as f64);
            if dpi >= 144.0 {
                (dpi / 96.0 * 2.0).round() / 2.0
            } else {
                1.0
            }
        } else {
            1.0
        };

        Ok(XRootGeometry {
            root_id: screen.root,
            width,
            height,
            scale,
        })
    }

    fn query_tree(&self, root: u32) -> Result<Vec<u32>, XError> {
        let tree = self
            .conn
            .query_tree(root)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;
        Ok(tree.children)
    }

    fn is_composite_available(&self) -> Result<bool, XError> {
        match self.conn.composite_query_version(0, 4) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => Ok(reply.major_version > 0 || reply.minor_version >= 2),
                Err(_) => Ok(false),
            },
            Err(_) => Ok(false),
        }
    }

    fn is_window_mapped(&self, window: u32) -> Result<bool, XError> {
        match self.conn.get_window_attributes(window) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => Ok(reply.map_state == MapState::VIEWABLE),
                Err(_) => Ok(false),
            },
            Err(_) => Ok(false),
        }
    }

    fn composite_name_window_pixmap(&self, window: u32) -> Result<u32, XError> {
        let _ = self
            .conn
            .composite_redirect_window(window, composite::Redirect::AUTOMATIC);
        let pixmap = self
            .conn
            .generate_id()
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .composite_name_window_pixmap(window, pixmap)
            .map_err(|e| XError::ExtensionUnavailable(e.to_string()))?
            .check()
            .map_err(|e| XError::ExtensionUnavailable(e.to_string()))?;
        Ok(pixmap)
    }

    fn free_pixmap(&self, pixmap: u32) -> Result<(), XError> {
        self.conn
            .free_pixmap(pixmap)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .check()
            .map_err(|e| XError::Protocol(e.to_string()))?;
        Ok(())
    }

    fn get_image(
        &self,
        drawable: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<RawImage, XError> {
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, drawable, x, y, width, height, !0)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;

        let red_mask = 0x00ff_0000;
        let green_mask = 0x0000_ff00;
        let blue_mask = 0x0000_00ff;

        let rgba = zpixmap_to_rgba(
            &reply.data,
            width as u32,
            height as u32,
            reply.depth,
            red_mask,
            green_mask,
            blue_mask,
        )?;
        Ok(RawImage {
            width: width as u32,
            height: height as u32,
            data: rgba,
        })
    }

    fn is_xtest_available(&self) -> Result<bool, XError> {
        let ext = self
            .conn
            .extension_information(x11rb::protocol::xtest::X11_EXTENSION_NAME)
            .map_err(|e| XError::Protocol(e.to_string()))?;
        if ext.is_none() {
            return Ok(false);
        }
        match self.conn.xtest_get_version(2, 2) {
            Ok(cookie) => match cookie.reply() {
                Ok(reply) => Ok(reply.major_version >= 2),
                Err(_) => Ok(false),
            },
            Err(_) => Ok(false),
        }
    }

    fn xtest_fake_motion(&self, root_x: i16, root_y: i16) -> Result<(), XError> {
        // MotionNotify = 6
        self.conn
            .xtest_fake_input(6, 0, 0, self.screen().root, root_x, root_y, 0)
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }

    fn xtest_fake_button(&self, button: u8, is_press: bool) -> Result<(), XError> {
        // ButtonPress = 4, ButtonRelease = 5
        let event_type = if is_press { 4 } else { 5 };
        self.conn
            .xtest_fake_input(event_type, button, 0, x11rb::NONE, 0, 0, 0)
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }

    fn xtest_fake_key(&self, keycode: u8, is_press: bool) -> Result<(), XError> {
        // KeyPress = 2, KeyRelease = 3
        let event_type = if is_press { 2 } else { 3 };
        self.conn
            .xtest_fake_input(event_type, keycode, 0, x11rb::NONE, 0, 0, 0)
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }

    fn activate_window(&self, window: u32) -> Result<(), XError> {
        let event = ClientMessageEvent {
            response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window,
            type_: self.net_active_window,
            data: ClientMessageData::from([1, x11rb::CURRENT_TIME, 0, 0, 0]),
        };
        self.conn
            .send_event(
                false,
                self.screen().root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }

    fn keyboard_bounds(&self) -> Result<(u8, u8), XError> {
        let setup = self.conn.setup();
        Ok((setup.min_keycode, setup.max_keycode))
    }

    fn get_keyboard_mapping(&self, first_keycode: u8, count: u8) -> Result<(Vec<u32>, u8), XError> {
        let reply = self
            .conn
            .get_keyboard_mapping(first_keycode, count)
            .map_err(|e| XError::Protocol(e.to_string()))?
            .reply()
            .map_err(|e| XError::Protocol(e.to_string()))?;
        Ok((reply.keysyms, reply.keysyms_per_keycode))
    }

    fn change_keyboard_mapping(
        &self,
        first_keycode: u8,
        keysyms_per_keycode: u8,
        keysyms: &[u32],
    ) -> Result<(), XError> {
        let keycode_count = (keysyms.len() / (keysyms_per_keycode as usize).max(1)) as u8;
        self.conn
            .change_keyboard_mapping(keycode_count, first_keycode, keysyms_per_keycode, keysyms)
            .map_err(|e| XError::Protocol(e.to_string()))?;
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }

    fn flush(&self) -> Result<(), XError> {
        self.conn
            .flush()
            .map_err(|e| XError::Protocol(e.to_string()))
    }
}
