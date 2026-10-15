//! Wayland ScreenCast video frame receiving, boundary verification, and delivery validation.
//!
//! Enforces ADR 0004 (no frame delivery when tree is incomplete or capture bounds are unknown),
//! cross-origin frame rejection, stale frame detection, expired credential eviction, and
//! produces raw `gcu_core::driver::Capture` for consumption by core's redaction pipeline.

use crate::wayland::portal::PortalStream;
use async_trait::async_trait;
use gcu_core::{
    driver::Capture,
    model::{Geometry, RawTree, Rect},
};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Video buffer transform flags aligned with PipeWire / Wayland transforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VideoTransform {
    /// Normal orientation without rotation or flip.
    #[default]
    Normal,
    /// Rotated 90 degrees clockwise.
    Rotate90,
    /// Rotated 180 degrees clockwise.
    Rotate180,
    /// Rotated 270 degrees clockwise.
    Rotate270,
    /// Flipped horizontally.
    Flipped,
    /// Flipped horizontally and rotated 90 degrees.
    Flipped90,
    /// Flipped horizontally and rotated 180 degrees.
    Flipped180,
    /// Flipped horizontally and rotated 270 degrees.
    Flipped270,
}

/// Pixel rectangle crop within a negotiated buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    /// Horizontal offset in pixels.
    pub x: u32,
    /// Vertical offset in pixels.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl PixelRegion {
    /// Validate that this crop region fits cleanly within the specified buffer bounds.
    pub fn is_valid_for(&self, buffer_width: u32, buffer_height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self.x.saturating_add(self.width) <= buffer_width
            && self.y.saturating_add(self.height) <= buffer_height
    }
}

/// Raw video frame delivered from a PipeWire video stream before sanitization.
#[derive(Clone, Debug)]
pub struct RawCapturedFrame {
    /// PipeWire stream node ID from which this frame was received.
    pub stream_node_id: u32,
    /// Monotonically increasing sequence number from the stream.
    pub sequence: u64,
    /// Negotiated buffer width in pixels.
    pub width: u32,
    /// Negotiated buffer height in pixels.
    pub height: u32,
    /// Crop region within the buffer, if specified by metadata.
    pub crop_region: Option<PixelRegion>,
    /// Buffer orientation transform.
    pub transform: VideoTransform,
    /// Timestamp in seconds when this frame was captured on the system.
    pub captured_at_secs: u64,
    /// Raw unpadded 32-bit RGBA pixel bytes (top-to-bottom).
    pub rgba: Arc<[u8]>,
}

/// Errors occurring during Wayland ScreenCast frame capture or validation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CaptureError {
    /// Underlying stream transport error.
    #[error("PipeWire capture stream error: {0}")]
    Stream(String),

    /// Frame capture bounds or dimensions are unknown, zero, or out-of-bounds (ADR 0004).
    #[error("capture bounds are unknown or invalid (ADR 0004)")]
    UnknownCaptureBounds,

    /// Frame was captured before the current observation permit was issued.
    #[error(
        "captured frame is stale: captured at {captured_at}, observation issued at {issued_at}"
    )]
    StaleFrame {
        /// Frame capture timestamp.
        captured_at: u64,
        /// Observation permit issuance timestamp.
        issued_at: u64,
    },

    /// Frame originated from an unexpected PipeWire stream node.
    #[error("cross-origin frame rejected: expected stream node {expected}, got {actual}")]
    CrossOriginFrame {
        /// Expected stream node ID.
        expected: u32,
        /// Actual frame stream node ID.
        actual: u32,
    },

    /// Observation permit or portal session credentials have expired.
    #[error("capture credentials or authorization expired")]
    CredentialsExpired,

    /// Tree traversal was incomplete or sensitive bounds cannot be verified (ADR 0004).
    #[error("sensitive area localization failed: handoff required: {0}")]
    HandoffRequired(String),

    /// Frame capture timed out waiting for next frame.
    #[error("timed out waiting for PipeWire frame")]
    Timeout,
}

/// Abstract receiver for PipeWire video frames.
#[async_trait]
pub trait PipeWireFrameReceiver: Send + Sync {
    /// Receive a single raw video frame from the stream.
    async fn receive_frame(&self) -> Result<RawCapturedFrame, CaptureError>;
}

/// In-memory fake frame receiver for deterministic unit and cross-platform tests.
#[derive(Debug, Clone)]
pub struct FakeFrameReceiver {
    frame: Arc<Mutex<Option<RawCapturedFrame>>>,
    error_injection: Arc<Mutex<Option<CaptureError>>>,
}

impl FakeFrameReceiver {
    /// Create a new fake receiver preloaded with a valid 2x2 RGBA frame.
    pub fn new(stream_node_id: u32, now_secs: u64) -> Self {
        let default_frame = RawCapturedFrame {
            stream_node_id,
            sequence: 1,
            width: 2,
            height: 2,
            crop_region: None,
            transform: VideoTransform::Normal,
            captured_at_secs: now_secs,
            rgba: Arc::new([
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
            ]),
        };
        Self {
            frame: Arc::new(Mutex::new(Some(default_frame))),
            error_injection: Arc::new(Mutex::new(None)),
        }
    }

    /// Update the frame to be returned by next receive call.
    pub fn set_frame(&self, frame: Option<RawCapturedFrame>) {
        *self.frame.lock().unwrap_or_else(|e| e.into_inner()) = frame;
    }

    /// Inject an error to be returned on the next receive call.
    pub fn inject_error(&self, error: CaptureError) {
        *self
            .error_injection
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(error);
    }
}

#[async_trait]
impl PipeWireFrameReceiver for FakeFrameReceiver {
    async fn receive_frame(&self) -> Result<RawCapturedFrame, CaptureError> {
        if let Some(err) = self
            .error_injection
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Err(err);
        }
        self.frame
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(CaptureError::Timeout)
    }
}

#[cfg(all(target_os = "linux", feature = "pipewire"))]
/// Real PipeWire frame receiver connected to a screencast stream FD and node ID.
pub struct RealPipeWireFrameReceiver {
    stream_node_id: u32,
    frame_rx:
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<Result<RawCapturedFrame, CaptureError>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(all(target_os = "linux", feature = "pipewire"))]
impl RealPipeWireFrameReceiver {
    /// Create a new PipeWire frame receiver connected to the given file descriptor and node ID.
    pub fn new(fd: std::os::fd::OwnedFd, stream_node_id: u32) -> Result<Self, CaptureError> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();

        std::thread::Builder::new()
            .name("gcu-pipewire-capture".into())
            .spawn(move || {
                if let Err(e) = run_pipewire_loop(fd, stream_node_id, tx.clone(), stop_clone) {
                    let _ = tx.blocking_send(Err(e));
                }
            })
            .map_err(|e| {
                CaptureError::Stream(format!("failed to spawn PipeWire capture thread: {e}"))
            })?;

        Ok(Self {
            stream_node_id,
            frame_rx: tokio::sync::Mutex::new(rx),
            stop,
        })
    }

    /// Stream node ID associated with this receiver.
    pub fn stream_node_id(&self) -> u32 {
        self.stream_node_id
    }
}

#[cfg(all(target_os = "linux", feature = "pipewire"))]
#[async_trait]
impl PipeWireFrameReceiver for RealPipeWireFrameReceiver {
    async fn receive_frame(&self) -> Result<RawCapturedFrame, CaptureError> {
        use tokio::time::{timeout, Duration};
        let mut rx = self.frame_rx.lock().await;
        match timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(frame_res)) => frame_res,
            Ok(None) => Err(CaptureError::Stream(
                "PipeWire stream channel closed".into(),
            )),
            Err(_) => Err(CaptureError::Timeout),
        }
    }
}

#[cfg(all(target_os = "linux", feature = "pipewire"))]
impl Drop for RealPipeWireFrameReceiver {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(all(target_os = "linux", feature = "pipewire"))]
fn run_pipewire_loop(
    fd: std::os::fd::OwnedFd,
    stream_node_id: u32,
    tx: tokio::sync::mpsc::Sender<Result<RawCapturedFrame, CaptureError>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), CaptureError> {
    use pipewire as pw;
    use pw::spa;
    use std::sync::atomic::Ordering;

    pw::init();
    let mainloop = pw::main_loop::MainLoopBox::new(None)
        .map_err(|e| CaptureError::Stream(format!("failed to create PipeWire mainloop: {e}")))?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None)
        .map_err(|e| CaptureError::Stream(format!("failed to create PipeWire context: {e}")))?;
    let core = context
        .connect_fd(fd, None)
        .map_err(|e| CaptureError::Stream(format!("failed to connect PipeWire core FD: {e}")))?;

    let stream = pw::stream::StreamBox::new(
        &core,
        "gcu-screencast-client",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|e| CaptureError::Stream(format!("failed to create PipeWire stream: {e}")))?;

    #[derive(Default)]
    struct StreamUserData {
        format: Option<spa::param::video::VideoInfoRaw>,
        sequence: u64,
    }

    let user_data = StreamUserData::default();
    let tx_clone = tx.clone();
    let stop_thread = stop.clone();

    let _listener = stream
        .add_local_listener_with_user_data(user_data)
        .state_changed(|_, _, _old, new| {
            if let pw::stream::StreamState::Error(err) = new {
                tracing::error!("PipeWire stream state error: {err}");
            }
        })
        .param_changed(|_, data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let (media_type, media_subtype) = match spa::param::format_utils::parse_format(param) {
                Ok(res) => res,
                Err(_) => return,
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }
            let mut info = spa::param::video::VideoInfoRaw::default();
            if info.parse(param).is_ok() {
                data.format = Some(info);
            }
        })
        .process(move |stream, data| {
            if stop_thread.load(Ordering::SeqCst) {
                return;
            }
            let Some(info) = &data.format else {
                return;
            };
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };

            let width = info.size().width;
            let height = info.size().height;
            if width == 0 || height == 0 {
                return;
            }

            let datas = buffer.datas_mut();
            if datas.is_empty() {
                return;
            }
            let plane = &mut datas[0];
            let Some(raw_slice) = plane.data() else {
                return;
            };

            let expected_size = (width as usize) * (height as usize) * 4;
            let rgba_vec: Vec<u8> = if raw_slice.len() >= expected_size {
                raw_slice[..expected_size].to_vec()
            } else {
                let mut v = raw_slice.to_vec();
                v.resize(expected_size, 0);
                v
            };

            data.sequence = data.sequence.saturating_add(1);
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let frame = RawCapturedFrame {
                stream_node_id,
                sequence: data.sequence,
                width,
                height,
                crop_region: None,
                transform: VideoTransform::Normal,
                captured_at_secs: now_secs,
                rgba: Arc::from(rgba_vec.into_boxed_slice()),
            };

            let _ = tx_clone.blocking_send(Ok(frame));
        })
        .register()
        .map_err(|e| CaptureError::Stream(format!("failed to register stream listener: {e}")))?;

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRx,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 8192,
                height: 8192
            }
        ),
    );

    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(|e| CaptureError::Stream(format!("failed to serialize pod format offer: {e}")))?
    .0
    .into_inner();

    let mut params = [spa::pod::Pod::from_bytes(&values)
        .ok_or_else(|| CaptureError::Stream("invalid pod bytes".into()))?];

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(stream_node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|e| CaptureError::Stream(format!("failed to connect PipeWire stream: {e}")))?;

    while !stop.load(Ordering::SeqCst) {
        let res = mainloop
            .loop_()
            .iterate(std::time::Duration::from_millis(50));
        if res < 0 && res != -4 {
            return Err(CaptureError::Stream(format!(
                "PipeWire loop iteration failed: {res}"
            )));
        }
    }

    Ok(())
}

/// Pipeline for validating, cropping, and encoding single Wayland ScreenCast frames.
pub struct WaylandCapturePipeline;

impl WaylandCapturePipeline {
    /// Execute single frame delivery sanitization pipeline.
    ///
    /// Checks credentials, verifies tree completeness (ADR 0004), retrieves a frame from the
    /// receiver, validates bounds, prevents cross-origin or stale delivery, and outputs `Capture`.
    pub async fn capture_frame(
        receiver: &dyn PipeWireFrameReceiver,
        authorized_stream: &PortalStream,
        permit_issued_at_secs: u64,
        permit_expires_at_secs: u64,
        tree: &RawTree,
        now_secs: u64,
    ) -> Result<Capture, CaptureError> {
        // 1. Verify credential lifetime
        if now_secs >= permit_expires_at_secs {
            return Err(CaptureError::CredentialsExpired);
        }

        // 2. ADR 0004: Tree completeness check before capture delivery
        if let Some(ref incomplete) = tree.incomplete {
            return Err(CaptureError::HandoffRequired(format!(
                "accessibility tree traversal is incomplete ({:?}); sensitive bounds cannot be guaranteed",
                incomplete.reason
            )));
        }

        // 3. Receive raw frame from PipeWire receiver
        let frame = receiver.receive_frame().await?;

        // 4. Verify capture dimensions are known and positive
        if frame.width == 0 || frame.height == 0 {
            return Err(CaptureError::UnknownCaptureBounds);
        }

        // 5. Verify crop region bounds if present
        let (out_width, out_height, cropped_rgba) = if let Some(crop) = frame.crop_region {
            if !crop.is_valid_for(frame.width, frame.height) {
                return Err(CaptureError::UnknownCaptureBounds);
            }
            let cropped = extract_crop(
                &frame.rgba,
                frame.width,
                frame.height,
                crop.x,
                crop.y,
                crop.width,
                crop.height,
            )?;
            (crop.width, crop.height, cropped)
        } else {
            let expected_len = (frame.width as usize) * (frame.height as usize) * 4;
            if frame.rgba.len() != expected_len {
                return Err(CaptureError::UnknownCaptureBounds);
            }
            (frame.width, frame.height, frame.rgba.to_vec())
        };

        // 6. Cross-origin check: frame must originate from authorized stream node
        if frame.stream_node_id != authorized_stream.node_id {
            return Err(CaptureError::CrossOriginFrame {
                expected: authorized_stream.node_id,
                actual: frame.stream_node_id,
            });
        }

        // 7. Freshness check: frame must not precede observation permit issuance by > 5s
        if frame.captured_at_secs.saturating_add(5) < permit_issued_at_secs {
            return Err(CaptureError::StaleFrame {
                captured_at: frame.captured_at_secs,
                issued_at: permit_issued_at_secs,
            });
        }

        // 8. Encode to PNG for raw Capture output
        let png_bytes = encode_rgba_to_png(&cropped_rgba, out_width, out_height)?;

        Ok(Capture {
            bytes: png_bytes,
            width: out_width,
            height: out_height,
            mime: "image/png".into(),
            geometry: Geometry {
                frame: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: out_width as f64,
                    h: out_height as f64,
                },
                scale: 1.0,
            },
        })
    }
}

fn extract_crop(
    rgba: &[u8],
    stride_width: u32,
    _height: u32,
    crop_x: u32,
    crop_y: u32,
    crop_w: u32,
    crop_h: u32,
) -> Result<Vec<u8>, CaptureError> {
    let mut out = Vec::with_capacity((crop_w as usize) * (crop_h as usize) * 4);
    for row in 0..crop_h {
        let src_y = crop_y + row;
        let start = ((src_y * stride_width + crop_x) * 4) as usize;
        let end = start + (crop_w as usize) * 4;
        if end > rgba.len() {
            return Err(CaptureError::UnknownCaptureBounds);
        }
        out.extend_from_slice(&rgba[start..end]);
    }
    Ok(out)
}

fn encode_rgba_to_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, CaptureError> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| CaptureError::Stream(e.to_string()))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| CaptureError::Stream(e.to_string()))?;
    writer
        .finish()
        .map_err(|e| CaptureError::Stream(e.to_string()))?;
    Ok(output)
}
