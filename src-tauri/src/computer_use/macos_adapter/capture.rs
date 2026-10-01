//! The selected CGWindow is the complete capture allowlist. No desktop retry.

use super::*;
use base64::Engine;
use grok_computer_use_core::execution::ActionCancellation;
use grok_computer_use_core::quartz_frame::{
    validate_image_size, MAX_IMAGE_BYTES, WINDOW_CAPTURE_IMAGE_OPTIONS, WINDOW_CAPTURE_LIST_OPTIONS,
};
use sha2::{Digest, Sha256};

pub(super) fn display_revision() -> Result<u64, String> {
    let mut ids = [0u32; 32];
    let mut count = 0u32;
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0
        || count == 0
        || count as usize >= ids.len()
    {
        return Err("cannot establish bounded macOS display topology".into());
    }
    ids[..count as usize].sort_unstable();
    let mut hash = Sha256::new();
    for &id in &ids[..count as usize] {
        let rect = unsafe { CGDisplayBounds(id) };
        let bounds = WindowBounds::new(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
        )?;
        let mode = OwnedCf::new(
            unsafe { CGDisplayCopyDisplayMode(id) },
            "display mode missing",
        )?;
        let pw = unsafe { CGDisplayModeGetPixelWidth(mode.0) };
        let ph = unsafe { CGDisplayModeGetPixelHeight(mode.0) };
        let rotation = unsafe { CGDisplayRotation(id) };
        if pw == 0 || ph == 0 || !rotation.is_finite() {
            return Err("invalid macOS display mode".into());
        }
        hash.update(id.to_le_bytes());
        hash.update((pw as u64).to_le_bytes());
        hash.update((ph as u64).to_le_bytes());
        for n in [bounds.x, bounds.y, bounds.width, bounds.height, rotation] {
            hash.update(n.to_bits().to_le_bytes());
        }
    }
    Ok(
        (u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap())
            & super::super::protocol::JS_MAX_SAFE_INTEGER)
            .max(1),
    )
}

pub(super) fn capture(
    adapter: &MacosAdapter,
    target_id: &str,
    cancellation: &ActionCancellation,
) -> Result<(Observation, CapturedFrame), String> {
    cancellation.check()?;
    if !MacosAdapter::screen_ok() {
        return Err("Screen Recording permission missing".into());
    }
    let target = WindowInstance::parse(target_id)?.window;
    let bounds = adapter.bound_window(target_id, cancellation)?;
    let display = display_revision()?;
    identity::validate(target)?;
    cancellation.check()?;
    // CGRectNull fits precisely this window. IncludingWindow without
    // OnScreenOnly/Above/Below is essential: those flags add other windows.
    // IgnoreFraming removes shadows; BestResolution keeps Retina detail.
    let image = OwnedCf::new(
        unsafe {
            CGWindowListCreateImage(
                CGRectNull,
                WINDOW_CAPTURE_LIST_OPTIONS,
                target.wid,
                WINDOW_CAPTURE_IMAGE_OPTIONS,
            )
        },
        "selected-window capture failed; desktop fallback forbidden",
    )?;
    cancellation.check()?;
    let width = unsafe { CGImageGetWidth(image.0) };
    let height = unsafe { CGImageGetHeight(image.0) };
    let frame = CapturedFrame::new(
        target,
        bounds,
        display,
        format!("mac-snap-{}", uuid::Uuid::new_v4()),
        width,
        height,
    )?;
    let png = encode_png(image.0, width, height)?;
    cancellation.check()?;
    // A moved/resized window or changed display cannot publish a frame whose
    // pixels and current Quartz geometry describe different objects/spaces.
    if adapter.bound_window(target_id, cancellation)? != bounds || display_revision()? != display {
        return Err("macOS target geometry changed during capture".into());
    }
    identity::validate(target)?;
    let observation = Observation {
        version: PROTOCOL_VERSION,
        run_id: String::new(),
        target_id: target_id.into(),
        target_generation: 0,
        snapshot_id: frame.snapshot_id.clone(),
        captured_at: chrono::Utc::now().to_rfc3339(),
        geometry_revision: frame.revision(),
        coordinate_space: super::super::protocol::COORDINATE_SPACE_IMAGE_PIXELS.into(),
        image: ObservationImage {
            width: frame.width,
            height: frame.height,
            content_id: format!("sha256-{:x}", Sha256::digest(&png)),
            png_base64: Some(base64::engine::general_purpose::STANDARD.encode(png)),
        },
        text: String::new(),
        nodes: Vec::new(), // No fabricated AX root or semantic action.
        truncated: false,
        crop_x: 0,
        crop_y: 0,
        crop_width: frame.width,
        crop_height: frame.height,
        scale: f64::from(frame.width) / bounds.width,
        dpi: 72.0 * f64::from(frame.width) / bounds.width,
        origin_x: bounds.x.round() as i32,
        origin_y: bounds.y.round() as i32,
        topology_revision: display,
    };
    Ok((observation, frame))
}

fn encode_png(image: CfTypeRef, width: usize, height: usize) -> Result<Vec<u8>, String> {
    validate_image_size(width, height)?;
    // ImageIO understands the actual image's color space, alpha and pixel
    // format. Never reinterpret a CGDataProvider as assumed 32-bit BGRA.
    let data = OwnedCf::new(
        unsafe { CFDataCreateMutable(std::ptr::null(), 0) },
        "PNG buffer allocation failed",
    )?;
    let image_type = OwnedCf::new(cf_str("public.png"), "PNG type allocation failed")?;
    let destination = OwnedCf::new(
        unsafe {
            CGImageDestinationCreateWithData(
                data.0 as *mut c_void,
                image_type.0,
                1,
                std::ptr::null(),
            )
        },
        "PNG encoder creation failed",
    )?;
    unsafe { CGImageDestinationAddImage(destination.0, image, std::ptr::null()) };
    if !unsafe { CGImageDestinationFinalize(destination.0) } {
        return Err("native PNG encoding failed".into());
    }
    let len = usize::try_from(unsafe { CFDataGetLength(data.0) })
        .map_err(|_| "invalid native PNG length")?;
    let ptr = unsafe { CFDataGetBytePtr(data.0) };
    if ptr.is_null() || len == 0 || len > MAX_IMAGE_BYTES {
        return Err("native PNG exceeds bounded image budget".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}
