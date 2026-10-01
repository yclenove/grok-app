//! Owned CPU frames. Compositor coordinates are deliberately not pixel coordinates.
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const MAX_SIDE: u32 = 8192;
pub(crate) const DEFAULT_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// PipeWire buffer orientation metadata. `rgba` is buffer-oriented; consumers
/// must explicitly interpret it for presentation, never silently assume upright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoTransform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}
impl VideoTransform {
    pub(crate) fn from_raw(raw: u32) -> Result<Self, String> {
        use pipewire::spa::sys::{
            SPA_META_TRANSFORMATION_Flipped as FLIPPED,
            SPA_META_TRANSFORMATION_Flipped180 as FLIPPED180,
            SPA_META_TRANSFORMATION_Flipped270 as FLIPPED270,
            SPA_META_TRANSFORMATION_Flipped90 as FLIPPED90, SPA_META_TRANSFORMATION_None as NORMAL,
            SPA_META_TRANSFORMATION_180 as ROTATED180, SPA_META_TRANSFORMATION_270 as ROTATED270,
            SPA_META_TRANSFORMATION_90 as ROTATED90,
        };
        Ok(match raw {
            NORMAL => Self::Normal,
            ROTATED90 => Self::Rotate90,
            ROTATED180 => Self::Rotate180,
            ROTATED270 => Self::Rotate270,
            FLIPPED => Self::Flipped,
            FLIPPED90 => Self::Flipped90,
            FLIPPED180 => Self::Flipped180,
            FLIPPED270 => Self::Flipped270,
            _ => return Err("unknown PipeWire video transform".into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct CapturedFrame {
    pub run_id: String,
    pub node_id: u32,
    pub pipewire_serial: u64,
    pub sequence: u64,
    pub format_generation: u64,
    /// The valid crop in the negotiated buffer, not a global desktop rectangle.
    pub region: PixelRegion,
    pub buffer_width: u32,
    pub buffer_height: u32,
    pub transform: VideoTransform,
    /// Local delivery time of these pixels, not the observation issue time or
    /// proof that the compositor rendered a new image at that instant.
    pub captured_at: Instant,
    /// Packed RGBA, top-to-bottom, cropped to `region`, without row padding.
    pub rgba: Arc<[u8]>,
}

pub(crate) struct FrameStore(Mutex<Slot>, Duration);
impl Default for FrameStore {
    fn default() -> Self {
        Self::new(DEFAULT_OBSERVATION_TIMEOUT)
    }
}
#[derive(Default)]
struct Slot {
    revoked: bool,
    epoch: u64,
    observation: u64,
    outstanding: Option<u64>,
    frame: Option<CapturedFrame>,
}
impl Slot {
    fn invalidate(&mut self) {
        self.frame = None;
        self.outstanding = None;
        match self.epoch.checked_add(1) {
            Some(next) => self.epoch = next,
            None => self.revoked = true,
        }
    }
}
/// Opaque capture authority; never constructed from caller-supplied frame fields.
pub(crate) struct FramePermit {
    pub(crate) frame: CapturedFrame,
    pub(crate) store: Arc<FrameStore>,
    epoch: u64,
    observation: u64,
    pub(crate) issued_at: Instant,
    pub(crate) expires_at: Instant,
}
impl FramePermit {
    pub(crate) fn geometry_revision(&self) -> u64 {
        self.epoch
    }
    /// Keep the frame lock across native submission. Clear/revoke/geometry
    /// changes cannot race between validation and the actual FFI calls.
    /// Lock order is always EI gate -> frame store, never the reverse.
    pub(crate) fn dispatch<T>(
        &self,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut slot = self.store.0.lock().map_err(|_| "frame owner poisoned")?;
        if slot.revoked || slot.epoch != self.epoch {
            return Err("observed capture authority or geometry changed".into());
        }
        if slot.outstanding != Some(self.observation) {
            return Err("observation superseded or already consumed".into());
        }
        // Burn authority before FFI, even on a native error. Neither retries nor
        // subsequent video buffers can renew a model's original action ticket.
        slot.outstanding = None;
        if Instant::now() >= self.expires_at || slot.frame.is_none() {
            return Err("observation expired or capture unavailable".into());
        }
        action()
    }
}
impl FrameStore {
    pub(crate) fn geometry_revision(&self) -> u64 {
        self.0.lock().map(|s| s.epoch).unwrap_or(0)
    }
    pub(crate) fn new(observation_timeout: Duration) -> Self {
        Self(Mutex::new(Slot::default()), observation_timeout)
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.invalidate();
        }
    }
    pub(crate) fn revoke(&self) {
        if let Ok(mut slot) = self.0.lock() {
            slot.revoked = true;
            slot.frame = None;
            slot.outstanding = None;
        }
    }
    pub(crate) fn publish(&self, frame: CapturedFrame) {
        if let Ok(mut slot) = self.0.lock() {
            if !slot.revoked {
                if slot.frame.as_ref().is_none_or(|old| {
                    old.run_id != frame.run_id
                        || old.node_id != frame.node_id
                        || old.pipewire_serial != frame.pipewire_serial
                        || old.format_generation != frame.format_generation
                        || old.region != frame.region
                        || old.transform != frame.transform
                        || old.buffer_width != frame.buffer_width
                        || old.buffer_height != frame.buffer_height
                        || old.sequence >= frame.sequence
                        || old.captured_at > frame.captured_at
                }) {
                    slot.invalidate();
                }
                if slot.revoked {
                    return;
                }
                slot.frame = Some(frame);
            }
        }
    }
    pub(crate) fn observe(self: &Arc<Self>) -> Result<Option<FramePermit>, String> {
        let mut slot = self.0.lock().map_err(|_| "frame owner poisoned")?;
        if slot.revoked {
            return Err("capture authorization revoked".into());
        }
        let Some(frame) = slot.frame.clone() else {
            return Ok(None);
        };
        let Some(observation) = slot.observation.checked_add(1) else {
            slot.revoked = true;
            slot.invalidate();
            return Err("observation identity exhausted".into());
        };
        let issued_at = Instant::now();
        let expires_at = issued_at
            .checked_add(self.1)
            .ok_or("observation deadline overflow")?;
        slot.observation = observation;
        slot.outstanding = Some(observation);
        Ok(Some(FramePermit {
            frame,
            store: self.clone(),
            epoch: slot.epoch,
            observation,
            issued_at,
            expires_at,
        }))
    }
    pub(crate) fn latest(&self) -> Result<Option<CapturedFrame>, String> {
        let slot = self.0.lock().map_err(|_| "frame owner poisoned")?;
        if slot.revoked {
            return Err("capture authorization revoked".into());
        }
        // Damage-driven sources can remain Streaming without delivering pixels.
        // Preserve the last valid image and its actual delivery timestamp, never
        // invent a fresh sample. Stream invalidation/revocation clears the slot.
        // This does NOT prove producer responsiveness or unchanged UI content.
        Ok(slot.frame.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Rgba,
    Rgbx,
    Bgra,
    Bgrx,
    Rgb,
    Bgr,
}
impl Layout {
    fn bpp(self) -> usize {
        if matches!(self, Self::Rgb | Self::Bgr) {
            3
        } else {
            4
        }
    }
}

pub(crate) fn validate_size(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err("PipeWire frame dimensions exceed capture limits".into());
    }
    Ok(())
}

pub(crate) struct RawFrame<'a> {
    pub data: &'a [u8],
    pub offset: u32,
    pub size: u32,
    pub stride: i32,
    pub width: u32,
    pub height: u32,
    pub layout: Layout,
    pub crop: Option<PixelRegion>,
}

pub(crate) fn copy_pixels(raw: RawFrame<'_>) -> Result<(PixelRegion, Arc<[u8]>), String> {
    validate_size(raw.width, raw.height)?;
    let region = raw.crop.unwrap_or(PixelRegion {
        x: 0,
        y: 0,
        width: raw.width,
        height: raw.height,
    });
    if region.width == 0
        || region.height == 0
        || region
            .x
            .checked_add(region.width)
            .is_none_or(|x| x > raw.width)
        || region
            .y
            .checked_add(region.height)
            .is_none_or(|y| y > raw.height)
    {
        return Err("invalid PipeWire crop".into());
    }
    let row_bytes = raw.width as usize * raw.layout.bpp();
    let stride = raw.stride.unsigned_abs() as usize;
    if stride < row_bytes {
        return Err("invalid PipeWire row stride".into());
    }
    let needed = stride
        .checked_mul(raw.height as usize - 1)
        .and_then(|v| v.checked_add(row_bytes))
        .ok_or("PipeWire plane size overflow")?;
    let end = (raw.offset as usize)
        .checked_add(raw.size as usize)
        .ok_or("PipeWire chunk overflow")?;
    if needed > raw.size as usize || end > raw.data.len() || raw.data.len() > MAX_BYTES {
        return Err("PipeWire chunk exceeds mapped plane".into());
    }
    let count = region.width as usize * region.height as usize * 4;
    if count > MAX_BYTES {
        return Err("PipeWire output exceeds capture limit".into());
    }
    let mut pixels = vec![0; count];
    for y in 0..region.height as usize {
        let source_y = region.y as usize + y;
        let source_y = if raw.stride < 0 {
            raw.height as usize - 1 - source_y
        } else {
            source_y
        };
        let start = raw.offset as usize + source_y * stride + region.x as usize * raw.layout.bpp();
        let row = &raw.data[start..start + region.width as usize * raw.layout.bpp()];
        let dest = &mut pixels[y * region.width as usize * 4..(y + 1) * region.width as usize * 4];
        for (input, output) in row
            .chunks_exact(raw.layout.bpp())
            .zip(dest.as_chunks_mut::<4>().0.iter_mut())
        {
            let bgr = matches!(raw.layout, Layout::Bgra | Layout::Bgrx | Layout::Bgr);
            output[0] = input[if bgr { 2 } else { 0 }];
            output[1] = input[1];
            output[2] = input[if bgr { 0 } else { 2 }];
            output[3] = if matches!(raw.layout, Layout::Rgba | Layout::Bgra) {
                input[3]
            } else {
                255
            };
        }
    }
    Ok((region, pixels.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> CapturedFrame {
        CapturedFrame {
            run_id: "test".into(),
            node_id: 1,
            pipewire_serial: 10,
            sequence: 1,
            format_generation: 1,
            region: PixelRegion {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            buffer_width: 1,
            buffer_height: 1,
            transform: VideoTransform::Normal,
            captured_at: Instant::now(),
            rgba: Arc::from([1, 2, 3, 255]),
        }
    }
    #[test]
    fn observed_geometry_identity_and_clear_aba_are_never_reusable() {
        type FrameChange = Box<dyn Fn(&mut CapturedFrame)>;
        let changes: Vec<FrameChange> = vec![
            Box::new(|f| f.run_id = "other".into()),
            Box::new(|f| f.node_id += 1),
            Box::new(|f| f.pipewire_serial += 1),
            Box::new(|f| f.format_generation += 1),
            Box::new(|f| f.region.x += 1),
            Box::new(|f| f.region.width += 1),
            Box::new(|f| f.buffer_width += 1),
            Box::new(|f| f.buffer_height += 1),
            Box::new(|f| f.transform = VideoTransform::Rotate90),
            Box::new(|f| f.sequence = 1),
            Box::new(|f| f.captured_at -= Duration::from_secs(1)),
        ];
        for change in changes {
            let store = Arc::new(FrameStore::default());
            let original = sample();
            store.publish(original.clone());
            let permit = store.observe().unwrap().unwrap();
            let mut changed = original.clone();
            changed.sequence = 2;
            change(&mut changed);
            store.publish(changed);
            assert!(permit
                .dispatch::<()>(|| panic!("stale observation dispatched"))
                .is_err());
            let mut restored = original;
            restored.sequence = 3;
            store.publish(restored);
            assert!(permit
                .dispatch::<()>(|| panic!("ABA observation dispatched"))
                .is_err());
        }
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        let permit = store.observe().unwrap().unwrap();
        store.clear();
        store.publish(sample());
        assert!(permit.dispatch(|| Ok(())).is_err());
    }

    #[test]
    fn new_pixels_preserve_geometry_but_cannot_refresh_an_old_observation() {
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        let mut permit = store.observe().unwrap().unwrap();
        let mut next = sample();
        next.sequence = 2;
        next.rgba = Arc::from([5, 6, 7, 255]);
        store.publish(next);
        assert_eq!(&*permit.frame.rgba, &[1, 2, 3, 255]);
        // Only crate-internal test code can alter the private action deadline.
        permit.expires_at = Instant::now();
        assert!(permit.dispatch(|| Ok(())).unwrap_err().contains("expired"));
        let new = store.observe().unwrap().unwrap();
        store.revoke();
        store.publish(sample());
        assert!(new.dispatch(|| Ok(())).is_err());
        assert!(store.observe().is_err());
    }

    #[test]
    fn frame_lock_serializes_native_dispatch_with_capture_invalidation() {
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        let permit = store.observe().unwrap().unwrap();
        let (entered, ready) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let owner = std::thread::spawn(move || {
            permit.dispatch(|| {
                entered.send(()).unwrap();
                wait.recv().unwrap();
                Ok(())
            })
        });
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(
            store.0.try_lock().is_err(),
            "frame guard was released before native action"
        );
        release.send(()).unwrap();
        owner.join().unwrap().unwrap();
        store.clear();
        assert!(store.observe().unwrap().is_none());
    }

    #[test]
    fn frame_epoch_exhaustion_and_unknown_transforms_fail_closed() {
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        store.0.lock().unwrap().epoch = u64::MAX;
        store.clear();
        store.publish(sample());
        assert!(store.observe().is_err());
        for raw in 0..8 {
            assert!(VideoTransform::from_raw(raw).is_ok());
        }
        assert!(VideoTransform::from_raw(8).is_err());
        assert!(VideoTransform::from_raw(u32::MAX).is_err());
    }
    #[test]
    fn retained_static_pixels_keep_delivery_time_but_clear_and_revoke_are_final() {
        let store = FrameStore::default();
        assert!(store.latest().unwrap().is_none());
        store.publish(sample());
        assert!(store.latest().unwrap().is_some());
        store.clear();
        assert!(store.latest().unwrap().is_none());
        let mut stale = sample();
        stale.captured_at -= Duration::from_secs(3);
        let delivered_at = stale.captured_at;
        store.publish(stale);
        assert_eq!(store.latest().unwrap().unwrap().captured_at, delivered_at);
        store.revoke();
        store.publish(sample());
        assert!(store.latest().is_err());
        let other = FrameStore::default();
        other.publish(sample());
        assert!(other.latest().unwrap().is_some());
    }
    #[test]
    fn static_image_observation_is_timed_from_issue_and_consumed_once() {
        let store = Arc::new(FrameStore::default());
        let mut static_image = sample();
        static_image.captured_at -= Duration::from_secs(3600);
        let delivered_at = static_image.captured_at;
        store.publish(static_image);
        let before = Instant::now();
        let permit = store.observe().unwrap().unwrap();
        assert!(permit.issued_at >= before);
        assert_eq!(
            permit.expires_at - permit.issued_at,
            DEFAULT_OBSERVATION_TIMEOUT
        );
        assert_eq!(permit.frame.captured_at, delivered_at);
        // Passive image reads are not new observations and do not retire tickets.
        assert_eq!(store.latest().unwrap().unwrap().captured_at, delivered_at);
        assert_eq!(permit.dispatch(|| Ok(123)).unwrap(), 123);
        assert!(permit.dispatch::<()>(|| panic!("ticket replayed")).is_err());
    }
    #[test]
    fn new_observation_supersedes_old_without_retiming_or_changing_pixels() {
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        let first = store.observe().unwrap().unwrap();
        let second = store.observe().unwrap().unwrap();
        assert_eq!(first.frame.captured_at, second.frame.captured_at);
        assert!(Arc::ptr_eq(&first.frame.rgba, &second.frame.rgba));
        assert!(second.issued_at >= first.issued_at);
        assert!(first
            .dispatch::<()>(|| panic!("superseded"))
            .unwrap_err()
            .contains("superseded"));
        assert_eq!(second.dispatch(|| Ok(123)).unwrap(), 123);
    }
    #[test]
    fn native_failure_also_consumes_observation_authority() {
        let store = Arc::new(FrameStore::default());
        store.publish(sample());
        let permit = store.observe().unwrap().unwrap();
        assert_eq!(
            permit
                .dispatch::<()>(|| Err("native failure".into()))
                .unwrap_err(),
            "native failure"
        );
        assert!(permit
            .dispatch::<()>(|| panic!("failed FFI replayed"))
            .is_err());
    }
    #[test]
    fn observation_identity_exhaustion_revokes_without_affecting_other_owners() {
        let store = Arc::new(FrameStore::default());
        let other = Arc::new(FrameStore::default());
        store.publish(sample());
        other.publish(sample());
        let ticket = store.observe().unwrap().unwrap();
        store.0.lock().unwrap().observation = u64::MAX;
        assert!(store.observe().is_err());
        assert!(ticket.dispatch::<()>(|| panic!("overflow ticket")).is_err());
        assert!(store.latest().is_err());
        other
            .observe()
            .unwrap()
            .unwrap()
            .dispatch(|| Ok(()))
            .unwrap();
    }
    #[test]
    fn padded_offset_crop_and_bottom_up_are_copied_without_padding() {
        let bytes = [
            77, 77, 3, 2, 1, 0, 6, 5, 4, 0, 88, 88, 9, 8, 7, 0, 12, 11, 10, 0,
        ];
        let make = |stride| RawFrame {
            data: &bytes,
            offset: 2,
            size: 18,
            stride,
            width: 2,
            height: 2,
            layout: Layout::Bgrx,
            crop: Some(PixelRegion {
                x: 1,
                y: 0,
                width: 1,
                height: 2,
            }),
        };
        assert_eq!(
            &*copy_pixels(make(10)).unwrap().1,
            &[4, 5, 6, 255, 10, 11, 12, 255]
        );
        assert_eq!(
            &*copy_pixels(make(-10)).unwrap().1,
            &[10, 11, 12, 255, 4, 5, 6, 255]
        );
    }
    #[test]
    fn malformed_dimensions_chunks_and_crops_fail_closed() {
        let bytes = [1; 16];
        let make = || RawFrame {
            data: &bytes,
            offset: 0,
            size: 16,
            stride: 8,
            width: 2,
            height: 2,
            layout: Layout::Rgba,
            crop: None,
        };
        for raw in [
            RawFrame {
                offset: 1,
                ..make()
            },
            RawFrame {
                stride: 7,
                ..make()
            },
            RawFrame { size: 15, ..make() },
            RawFrame {
                width: u32::MAX,
                ..make()
            },
            RawFrame {
                height: 0,
                ..make()
            },
            RawFrame {
                crop: Some(PixelRegion {
                    x: u32::MAX,
                    y: 0,
                    width: 2,
                    height: 1,
                }),
                ..make()
            },
        ] {
            assert!(copy_pixels(raw).is_err());
        }
    }
    #[test]
    fn all_packed_layouts_and_alpha_are_explicit() {
        for (layout, expected) in [
            (Layout::Rgba, [1, 2, 3, 4]),
            (Layout::Rgbx, [1, 2, 3, 255]),
            (Layout::Bgra, [3, 2, 1, 4]),
            (Layout::Bgrx, [3, 2, 1, 255]),
            (Layout::Rgb, [1, 2, 3, 255]),
            (Layout::Bgr, [3, 2, 1, 255]),
        ] {
            let (_, bytes) = copy_pixels(RawFrame {
                data: &[1, 2, 3, 4],
                offset: 0,
                size: 4,
                stride: 4,
                width: 1,
                height: 1,
                layout,
                crop: None,
            })
            .unwrap();
            assert_eq!(&*bytes, &expected);
        }
    }
}
