//! Core X11 server trait and protocol representations.

/// Geometry of an X11 window in root-relative coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XWindowGeometry {
    /// X coordinate relative to the root window.
    pub x: i32,
    /// Y coordinate relative to the root window.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Border width in pixels.
    pub border_width: u32,
    /// Color depth (typically 24 or 32).
    pub depth: u8,
}

/// Root window geometry and display metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XRootGeometry {
    /// Root window ID.
    pub root_id: u32,
    /// Screen width in physical pixels.
    pub width: u32,
    /// Screen height in physical pixels.
    pub height: u32,
    /// Display DPI scale factor (1.0 for standard 96 DPI).
    pub scale: f64,
}

/// Uncompressed 32-bit RGBA image buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixel data (`width * height * 4` bytes).
    pub data: Vec<u8>,
}

/// Errors originating from X11 protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum XError {
    /// Failed to connect to X11 display.
    #[error("X11 connection failed: {0}")]
    Connection(String),
    /// Protocol error during request or reply processing.
    #[error("X11 protocol error: {0}")]
    Protocol(String),
    /// Window not found or destroyed.
    #[error("X11 window 0x{0:x} not found")]
    WindowNotFound(u32),
    /// Requested X11 extension is unavailable.
    #[error("X11 extension {0} unavailable")]
    ExtensionUnavailable(String),
    /// Image conversion or buffer size error.
    #[error("X11 image error: {0}")]
    ImageError(String),
}

/// Abstraction over the X11 server connection and protocol operations.
pub trait XServer: Send + Sync {
    /// Query `_NET_CLIENT_LIST` from the root window.
    fn client_list(&self) -> Result<Vec<u32>, XError>;

    /// Query `_NET_WM_PID` for a window.
    fn window_pid(&self, window: u32) -> Result<Option<u32>, XError>;

    /// Query `_NET_WM_NAME` (falling back to `WM_NAME`) for a window.
    fn window_name(&self, window: u32) -> Result<Option<String>, XError>;

    /// Query `_NET_ACTIVE_WINDOW` from the root window.
    fn active_window(&self) -> Result<Option<u32>, XError>;

    /// Query window geometry in root-relative coordinates.
    fn window_geometry(&self, window: u32) -> Result<XWindowGeometry, XError>;

    /// Query root window metrics.
    fn root_geometry(&self) -> Result<XRootGeometry, XError>;

    /// Query window tree children of `root` in stacking order (bottom to top).
    fn query_tree(&self, root: u32) -> Result<Vec<u32>, XError>;

    /// Check if the XComposite extension is present and supported.
    fn is_composite_available(&self) -> Result<bool, XError>;

    /// Check whether a window is mapped (viewable).
    fn is_window_mapped(&self, window: u32) -> Result<bool, XError>;

    /// Create a named pixmap for the given window using XComposite.
    fn composite_name_window_pixmap(&self, window: u32) -> Result<u32, XError>;

    /// Free a pixmap resource.
    fn free_pixmap(&self, pixmap: u32) -> Result<(), XError>;

    /// Fetch raw image pixels from a drawable (window or pixmap) converted to RGBA8.
    fn get_image(
        &self,
        drawable: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> Result<RawImage, XError>;
}

/// Converts raw ZPixmap bytes to 32-bit RGBA8 format.
pub fn zpixmap_to_rgba(
    data: &[u8],
    width: u32,
    height: u32,
    depth: u8,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
) -> Result<Vec<u8>, XError> {
    let num_pixels = (width as usize) * (height as usize);
    if depth == 24 || depth == 32 {
        if data.len() < num_pixels * 4 {
            return Err(XError::ImageError(format!(
                "buffer length {} is less than expected {}",
                data.len(),
                num_pixels * 4
            )));
        }
        let mut rgba = Vec::with_capacity(num_pixels * 4);
        if red_mask == 0x00ff_0000 && green_mask == 0x0000_ff00 && blue_mask == 0x0000_00ff {
            for chunk in data.as_chunks::<4>().0.iter().take(num_pixels) {
                rgba.push(chunk[2]);
                rgba.push(chunk[1]);
                rgba.push(chunk[0]);
                rgba.push(255);
            }
        } else {
            let r_shift = red_mask.trailing_zeros();
            let g_shift = green_mask.trailing_zeros();
            let b_shift = blue_mask.trailing_zeros();
            for chunk in data.as_chunks::<4>().0.iter().take(num_pixels) {
                let pixel = u32::from_ne_bytes(*chunk);
                let r = (((pixel & red_mask) >> r_shift) & 0xff) as u8;
                let g = (((pixel & green_mask) >> g_shift) & 0xff) as u8;
                let b = (((pixel & blue_mask) >> b_shift) & 0xff) as u8;
                rgba.push(r);
                rgba.push(g);
                rgba.push(b);
                rgba.push(255);
            }
        }
        Ok(rgba)
    } else {
        Err(XError::ImageError(format!(
            "unsupported visual depth {depth}"
        )))
    }
}
