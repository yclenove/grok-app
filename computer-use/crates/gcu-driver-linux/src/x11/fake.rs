//! In-memory mock X11 server for platform-independent testing.

use crate::x11::server::{
    crop_into, RawImage, XError, XEventKind, XRootGeometry, XServer, XWindowGeometry,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

/// In-memory mock window for [`FakeX`].
#[derive(Debug, Clone)]
pub struct FakeWindow {
    /// X11 window ID.
    pub id: u32,
    /// Window title.
    pub name: Option<String>,
    /// Window PID.
    pub pid: Option<u32>,
    /// Geometry in root coordinates.
    pub geometry: XWindowGeometry,
    /// Whether window is mapped.
    pub is_mapped: bool,
    /// Window content image.
    pub image: Option<RawImage>,
}

/// Simulated event recorded by [`FakeX`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeXEvent {
    /// Motion event.
    Motion {
        /// Target root X coordinate.
        x: i16,
        /// Target root Y coordinate.
        y: i16,
    },
    /// Button event.
    Button {
        /// Button number.
        button: u8,
        /// Whether press or release.
        is_press: bool,
    },
    /// Key event.
    Key {
        /// X11 keycode.
        keycode: u8,
        /// Whether press or release.
        is_press: bool,
    },
}

/// In-memory mock X11 server for unit testing.
pub struct FakeX {
    /// Overlay capture barrier for deterministic cleanup and exclusion tests.
    pub overlay: super::fake_overlay::FakeOverlayCapture,
    windows: Mutex<HashMap<u32, FakeWindow>>,
    root: XRootGeometry,
    active_window: Mutex<Option<u32>>,
    stacking_order: Mutex<Vec<u32>>,
    composite_available: bool,
    pixmaps: Mutex<HashMap<u32, (u32, RawImage)>>,
    next_pixmap_id: Mutex<u32>,
    root_image: Mutex<Option<RawImage>>,
    xtest_available: Mutex<bool>,
    mouse_pos: Mutex<(i16, i16)>,
    pressed_buttons: Mutex<Vec<u8>>,
    pressed_keys: Mutex<Vec<u8>>,
    keymap: Mutex<HashMap<u8, Vec<u32>>>,
    activate_fails: Mutex<bool>,
    events: Mutex<Vec<FakeXEvent>>,
    grabbed_keys: Mutex<HashSet<(u8, u16)>>,
    conflicting_keys: Mutex<HashSet<(u8, u16)>>,
    xi_devices: Mutex<Vec<crate::x11::activity::DeviceInfo>>,
    xi2_available: Mutex<bool>,
    incoming: Mutex<VecDeque<XEventKind>>,
    incarnations: Mutex<HashMap<u32, u64>>,
    bump_incarnation_on_image: Mutex<bool>,
}

impl FakeX {
    /// Create a new in-memory FakeX instance.
    pub fn new(root: XRootGeometry, composite_available: bool) -> Self {
        Self {
            overlay: Default::default(),
            windows: Mutex::new(HashMap::new()),
            root,
            active_window: Mutex::new(None),
            stacking_order: Mutex::new(Vec::new()),
            composite_available,
            pixmaps: Mutex::new(HashMap::new()),
            next_pixmap_id: Mutex::new(0x200_0000),
            root_image: Mutex::new(None),
            xtest_available: Mutex::new(true),
            mouse_pos: Mutex::new((0, 0)),
            pressed_buttons: Mutex::new(Vec::new()),
            pressed_keys: Mutex::new(Vec::new()),
            keymap: Mutex::new(HashMap::new()),
            activate_fails: Mutex::new(false),
            events: Mutex::new(Vec::new()),
            grabbed_keys: Mutex::new(HashSet::new()),
            conflicting_keys: Mutex::new(HashSet::new()),
            xi_devices: Mutex::new(Vec::new()),
            xi2_available: Mutex::new(true),
            incoming: Mutex::new(VecDeque::new()),
            incarnations: Mutex::new(HashMap::new()),
            bump_incarnation_on_image: Mutex::new(false),
        }
    }

    /// Set the lifetime token scoped capture uses to detect a reused window id.
    pub fn set_window_incarnation(&self, window: u32, incarnation: u64) {
        lock_m(&self.incarnations).insert(window, incarnation);
    }

    /// After the next pixmap read, increment that window's incarnation.
    ///
    /// Scoped capture must drop the pixels when the token changes during the read.
    pub fn set_bump_incarnation_on_image(&self, enabled: bool) {
        *lock_m(&self.bump_incarnation_on_image) = enabled;
    }

    /// Queue one event for the next [`XServer::poll_event`] call.
    pub fn push_event(&self, event: XEventKind) {
        lock_m(&self.incoming).push_back(event);
    }

    /// Register a mock window.
    pub fn add_window(&self, window: FakeWindow) {
        let id = window.id;
        lock_m(&self.windows).insert(id, window);
        let mut stack = lock_m(&self.stacking_order);
        if !stack.contains(&id) {
            stack.push(id);
        }
    }

    /// Set the active window ID.
    pub fn set_active_window(&self, window: Option<u32>) {
        *lock_m(&self.active_window) = window;
    }

    /// Set stacking order (bottom to top).
    pub fn set_stacking_order(&self, order: Vec<u32>) {
        *lock_m(&self.stacking_order) = order;
    }

    /// Set the full root window image for crop testing.
    pub fn set_root_image(&self, img: RawImage) {
        *lock_m(&self.root_image) = Some(img);
    }

    /// Set whether XTEST extension is available.
    pub fn set_xtest_available(&self, avail: bool) {
        *lock_m(&self.xtest_available) = avail;
    }

    /// Set whether window activation should fail.
    pub fn set_activate_fails(&self, fails: bool) {
        *lock_m(&self.activate_fails) = fails;
    }

    /// Retrieve all recorded simulated events.
    pub fn get_events(&self) -> Vec<FakeXEvent> {
        lock_m(&self.events).clone()
    }

    /// Clear recorded simulated events.
    pub fn clear_events(&self) {
        lock_m(&self.events).clear();
    }

    /// Get current pressed buttons.
    pub fn get_pressed_buttons(&self) -> Vec<u8> {
        lock_m(&self.pressed_buttons).clone()
    }

    /// Get current pressed keys.
    pub fn get_pressed_keys(&self) -> Vec<u8> {
        lock_m(&self.pressed_keys).clone()
    }

    /// Get current simulated mouse position.
    pub fn get_mouse_pos(&self) -> (i16, i16) {
        *lock_m(&self.mouse_pos)
    }

    /// Configure whether XI2 extension is supported.
    pub fn set_xi2_available(&self, avail: bool) {
        *lock_m(&self.xi2_available) = avail;
    }

    /// Set simulated XI2 devices.
    pub fn set_xi_devices(&self, devs: Vec<crate::x11::activity::DeviceInfo>) {
        *lock_m(&self.xi_devices) = devs;
    }

    /// Add a key+modifier combination that triggers BadAccess grab conflict.
    pub fn add_conflicting_key(&self, keycode: u8, modifiers: u16) {
        lock_m(&self.conflicting_keys).insert((keycode, modifiers));
    }

    /// Check whether a key+modifier combination is currently grabbed.
    pub fn is_key_grabbed(&self, keycode: u8, modifiers: u16) -> bool {
        lock_m(&self.grabbed_keys).contains(&(keycode, modifiers))
    }
}

fn lock_m<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl XServer for FakeX {
    fn overlay_capture_scope(
        &self,
        target: u32,
    ) -> Result<Box<dyn super::overlay::CaptureScope + '_>, XError> {
        self.overlay.begin(target)
    }
    fn client_list(&self) -> Result<Vec<u32>, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(wins.keys().copied().collect())
    }

    fn window_incarnation(&self, window: u32) -> Result<Option<u64>, XError> {
        let wins = lock_m(&self.windows);
        if !wins.contains_key(&window) {
            return Err(XError::WindowNotFound(window));
        }
        Ok(lock_m(&self.incarnations).get(&window).copied())
    }

    fn window_pid(&self, window: u32) -> Result<Option<u32>, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        wins.get(&window)
            .map(|w| w.pid)
            .ok_or(XError::WindowNotFound(window))
    }

    fn window_name(&self, window: u32) -> Result<Option<String>, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        wins.get(&window)
            .map(|w| w.name.clone())
            .ok_or(XError::WindowNotFound(window))
    }

    fn active_window(&self) -> Result<Option<u32>, XError> {
        let active = match self.active_window.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(*active)
    }

    fn window_geometry(&self, window: u32) -> Result<XWindowGeometry, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        wins.get(&window)
            .map(|w| w.geometry)
            .ok_or(XError::WindowNotFound(window))
    }

    fn root_geometry(&self) -> Result<XRootGeometry, XError> {
        Ok(self.root)
    }

    fn query_tree(&self, _root: u32) -> Result<Vec<u32>, XError> {
        let stack = match self.stacking_order.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(stack.clone())
    }

    fn is_composite_available(&self) -> Result<bool, XError> {
        Ok(self.composite_available)
    }

    fn is_window_mapped(&self, window: u32) -> Result<bool, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        wins.get(&window)
            .map(|w| w.is_mapped)
            .ok_or(XError::WindowNotFound(window))
    }

    fn composite_name_window_pixmap(&self, window: u32) -> Result<u32, XError> {
        if !self.composite_available {
            return Err(XError::ExtensionUnavailable("Composite".into()));
        }
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let win = wins.get(&window).ok_or(XError::WindowNotFound(window))?;
        let img = win.image.clone().unwrap_or_else(|| RawImage {
            width: win.geometry.width,
            height: win.geometry.height,
            data: vec![240; (win.geometry.width * win.geometry.height * 4) as usize],
        });

        let mut next_id = match self.next_pixmap_id.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let pixmap_id = *next_id;
        *next_id += 1;

        let mut pixmaps = match self.pixmaps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pixmaps.insert(pixmap_id, (window, img));
        Ok(pixmap_id)
    }

    fn free_pixmap(&self, pixmap: u32) -> Result<(), XError> {
        let mut pixmaps = match self.pixmaps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        pixmaps.remove(&pixmap);
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
        let w = width as usize;
        let h = height as usize;
        let mut result_data = vec![0u8; w * h * 4];

        if drawable == self.root.root_id {
            let root_img = match self.root_image.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(ref img) = *root_img {
                crop_into(&mut result_data, w, h, img, x as i32, y as i32);
            } else {
                for pixel in result_data.as_chunks_mut::<4>().0 {
                    pixel[0] = 200;
                    pixel[1] = 200;
                    pixel[2] = 200;
                    pixel[3] = 255;
                }
            }
            return Ok(RawImage {
                width: width as u32,
                height: height as u32,
                data: result_data,
            });
        }

        let pixmaps = match self.pixmaps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((window, img)) = pixmaps.get(&drawable) {
            let window = *window;
            crop_into(&mut result_data, w, h, img, x as i32, y as i32);
            drop(pixmaps);
            if *lock_m(&self.bump_incarnation_on_image) {
                if let Some(token) = lock_m(&self.incarnations).get_mut(&window) {
                    *token = token.saturating_add(1);
                }
            }
            return Ok(RawImage {
                width: width as u32,
                height: height as u32,
                data: result_data,
            });
        }

        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(win) = wins.get(&drawable) {
            if let Some(ref img) = win.image {
                crop_into(&mut result_data, w, h, img, x as i32, y as i32);
            } else {
                for pixel in result_data.as_chunks_mut::<4>().0 {
                    pixel[0] = 245;
                    pixel[1] = 245;
                    pixel[2] = 245;
                    pixel[3] = 255;
                }
            }
            return Ok(RawImage {
                width: width as u32,
                height: height as u32,
                data: result_data,
            });
        }

        Err(XError::WindowNotFound(drawable))
    }

    fn is_xtest_available(&self) -> Result<bool, XError> {
        let xt = match self.xtest_available.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        Ok(*xt)
    }

    fn xtest_fake_motion(&self, root_x: i16, root_y: i16) -> Result<(), XError> {
        if !self.is_xtest_available()? {
            return Err(XError::ExtensionUnavailable("XTEST".into()));
        }
        let mut pos = match self.mouse_pos.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *pos = (root_x, root_y);
        let mut ev = match self.events.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        ev.push(FakeXEvent::Motion {
            x: root_x,
            y: root_y,
        });
        Ok(())
    }

    fn xtest_fake_button(&self, button: u8, is_press: bool) -> Result<(), XError> {
        if !self.is_xtest_available()? {
            return Err(XError::ExtensionUnavailable("XTEST".into()));
        }
        let mut btns = match self.pressed_buttons.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if is_press {
            if !btns.contains(&button) {
                btns.push(button);
            }
        } else {
            btns.retain(|&b| b != button);
        }
        let mut ev = match self.events.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        ev.push(FakeXEvent::Button { button, is_press });
        Ok(())
    }

    fn xtest_fake_key(&self, keycode: u8, is_press: bool) -> Result<(), XError> {
        if !self.is_xtest_available()? {
            return Err(XError::ExtensionUnavailable("XTEST".into()));
        }
        let mut keys = match self.pressed_keys.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if is_press {
            if !keys.contains(&keycode) {
                keys.push(keycode);
            }
        } else {
            keys.retain(|&k| k != keycode);
        }
        let mut ev = match self.events.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        ev.push(FakeXEvent::Key { keycode, is_press });
        Ok(())
    }

    fn activate_window(&self, window: u32) -> Result<(), XError> {
        let fails = match self.activate_fails.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if *fails {
            return Err(XError::Protocol("mock window activation failed".into()));
        }
        let mut active = match self.active_window.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *active = Some(window);
        Ok(())
    }

    fn keyboard_bounds(&self) -> Result<(u8, u8), XError> {
        Ok((8, 255))
    }

    fn get_keyboard_mapping(&self, first_keycode: u8, count: u8) -> Result<(Vec<u32>, u8), XError> {
        let map = match self.keymap.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let per_kc = 2u8;
        let mut keysyms = Vec::with_capacity(count as usize * per_kc as usize);
        for kc in first_keycode..(first_keycode.saturating_add(count)) {
            if let Some(entry) = map.get(&kc) {
                for i in 0..per_kc as usize {
                    keysyms.push(entry.get(i).copied().unwrap_or(0));
                }
            } else {
                let sym = if (32..127).contains(&kc) {
                    kc as u32
                } else {
                    0
                };
                keysyms.push(sym);
                keysyms.push(0);
            }
        }
        Ok((keysyms, per_kc))
    }

    fn change_keyboard_mapping(
        &self,
        first_keycode: u8,
        keysyms_per_keycode: u8,
        keysyms: &[u32],
    ) -> Result<(), XError> {
        let mut map = match self.keymap.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let per = keysyms_per_keycode.max(1) as usize;
        for (i, chunk) in keysyms.chunks(per).enumerate() {
            let kc = first_keycode.saturating_add(i as u8);
            map.insert(kc, chunk.to_vec());
        }
        Ok(())
    }

    fn flush(&self) -> Result<(), XError> {
        Ok(())
    }

    fn is_xi2_available(&self) -> Result<bool, XError> {
        let g = match self.xi2_available.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        Ok(*g)
    }

    fn query_xi_devices(&self) -> Result<Vec<crate::x11::activity::DeviceInfo>, XError> {
        let g = match self.xi_devices.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        Ok(g.clone())
    }

    fn grab_key(&self, keycode: u8, modifiers: u16) -> Result<(), XError> {
        let conflicts = match self.conflicting_keys.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if conflicts.contains(&(keycode, modifiers)) {
            return Err(XError::Protocol(format!(
                "BadAccess: grab conflict for keycode {keycode} modifiers {modifiers:#x}"
            )));
        }
        let mut grabbed = match self.grabbed_keys.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        grabbed.insert((keycode, modifiers));
        Ok(())
    }

    fn ungrab_key(&self, keycode: u8, modifiers: u16) -> Result<(), XError> {
        let mut grabbed = match self.grabbed_keys.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        grabbed.remove(&(keycode, modifiers));
        Ok(())
    }

    fn poll_event(&self) -> Result<Option<XEventKind>, XError> {
        Ok(lock_m(&self.incoming).pop_front())
    }
}
