//! In-memory mock X11 server for platform-independent testing.

use crate::x11::server::{RawImage, XError, XRootGeometry, XServer, XWindowGeometry};
use std::collections::HashMap;
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

/// In-memory mock X11 server for unit testing.
pub struct FakeX {
    windows: Mutex<HashMap<u32, FakeWindow>>,
    root: XRootGeometry,
    active_window: Mutex<Option<u32>>,
    stacking_order: Mutex<Vec<u32>>,
    composite_available: bool,
    pixmaps: Mutex<HashMap<u32, (u32, RawImage)>>,
    next_pixmap_id: Mutex<u32>,
    root_image: Mutex<Option<RawImage>>,
}

impl FakeX {
    /// Create a new in-memory FakeX instance.
    pub fn new(root: XRootGeometry, composite_available: bool) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            root,
            active_window: Mutex::new(None),
            stacking_order: Mutex::new(Vec::new()),
            composite_available,
            pixmaps: Mutex::new(HashMap::new()),
            next_pixmap_id: Mutex::new(0x200_0000),
            root_image: Mutex::new(None),
        }
    }

    /// Register a mock window.
    pub fn add_window(&self, window: FakeWindow) {
        let id = window.id;
        let mut wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        wins.insert(id, window);
        let mut stack = match self.stacking_order.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !stack.contains(&id) {
            stack.push(id);
        }
    }

    /// Set the active window ID.
    pub fn set_active_window(&self, window: Option<u32>) {
        let mut active = match self.active_window.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *active = window;
    }

    /// Set stacking order (bottom to top).
    pub fn set_stacking_order(&self, order: Vec<u32>) {
        let mut stack = match self.stacking_order.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *stack = order;
    }

    /// Set the full root window image for crop testing.
    pub fn set_root_image(&self, img: RawImage) {
        let mut r = match self.root_image.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *r = Some(img);
    }
}

impl XServer for FakeX {
    fn client_list(&self) -> Result<Vec<u32>, XError> {
        let wins = match self.windows.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(wins.keys().copied().collect())
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
        if let Some((_, img)) = pixmaps.get(&drawable) {
            crop_into(&mut result_data, w, h, img, x as i32, y as i32);
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
}

fn crop_into(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    src: &RawImage,
    offset_x: i32,
    offset_y: i32,
) {
    let src_w = src.width as i32;
    let src_h = src.height as i32;

    for dy in 0..dst_h {
        let sy = offset_y + dy as i32;
        if sy < 0 || sy >= src_h {
            continue;
        }
        for dx in 0..dst_w {
            let sx = offset_x + dx as i32;
            if sx < 0 || sx >= src_w {
                continue;
            }
            let src_idx = (sy as usize * src.width as usize + sx as usize) * 4;
            let dst_idx = (dy * dst_w + dx) * 4;
            if src_idx + 4 <= src.data.len() && dst_idx + 4 <= dst.len() {
                dst[dst_idx..dst_idx + 4].copy_from_slice(&src.data[src_idx..src_idx + 4]);
            }
        }
    }
}
