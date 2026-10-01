//! Pure, testable Quartz capture/coordinate contract. Native acquisition stays
//! in the Host. Screen points are never assumed to be screenshot pixels.

use sha2::{Digest, Sha256};

pub const WINDOW_CAPTURE_LIST_OPTIONS: u32 = 1 << 3; // IncludingWindow only.
pub const WINDOW_CAPTURE_IMAGE_OPTIONS: u32 = (1 << 0) | (1 << 3); // IgnoreFraming | BestResolution.
pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowKey {
    pub pid: u32,
    pub wid: u32,
    pub birth_seconds: u64,
    pub birth_microseconds: u64,
}

impl WindowKey {
    pub fn new(pid: u32, wid: u32, seconds: u64, microseconds: u64) -> Result<Self, String> {
        if pid == 0
            || pid > i32::MAX as u32
            || wid == 0
            || seconds == 0
            || microseconds >= 1_000_000
        {
            return Err("invalid macOS target identity".into());
        }
        Ok(Self {
            pid,
            wid,
            birth_seconds: seconds,
            birth_microseconds: microseconds,
        })
    }

    /// Full birth values stay in the opaque string. A lossy/hash-only numeric
    /// lifecycle stamp must never substitute for the native identity check.
    pub fn target_id(self) -> String {
        format!(
            "mac:{}:{}:{}:{}",
            self.pid, self.wid, self.birth_seconds, self.birth_microseconds
        )
    }

    pub fn lifecycle_stamp(self) -> u64 {
        let mut hash = Sha256::new();
        hash.update(b"mac-process-window-v1:");
        hash.update(self.target_id().as_bytes());
        (u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap())
            & crate::protocol::JS_MAX_SAFE_INTEGER)
            .max(1)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let mut parts = value.split(':');
        if parts.next() != Some("mac") {
            return Err("invalid macOS target".into());
        }
        let pid = parts.next().and_then(|v| v.parse::<u32>().ok());
        let wid = parts.next().and_then(|v| v.parse::<u32>().ok());
        let seconds = parts.next().and_then(|v| v.parse::<u64>().ok());
        let microseconds = parts.next().and_then(|v| v.parse::<u64>().ok());
        match (pid, wid, seconds, microseconds, parts.next()) {
            (Some(pid), Some(wid), Some(seconds), Some(microseconds), None) => {
                let key = Self::new(pid, wid, seconds, microseconds)?;
                if key.target_id() != value {
                    return Err("noncanonical macOS target identity".into());
                }
                Ok(key)
            }
            _ => Err("invalid macOS target".into()),
        }
    }
}

/// Host-issued identity for one retained native AX window. The full process
/// birth stays explicit; the nonce is never reconstructed from title/bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowInstance {
    pub window: WindowKey,
    pub nonce: uuid::Uuid,
}

impl WindowInstance {
    pub fn fresh(window: WindowKey) -> Self {
        Self {
            window,
            nonce: uuid::Uuid::new_v4(),
        }
    }

    pub fn target_id(self) -> String {
        format!("{}:{}", self.window.target_id(), self.nonce.simple())
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        let (window, nonce) = value
            .rsplit_once(':')
            .ok_or("missing macOS window instance")?;
        let instance = Self {
            window: WindowKey::parse(window)?,
            nonce: uuid::Uuid::parse_str(nonce).map_err(|_| "invalid macOS window instance")?,
        };
        if instance.nonce.is_nil() || instance.target_id() != value {
            return Err("noncanonical macOS window instance".into());
        }
        Ok(instance)
    }

    pub fn lifecycle_stamp(self) -> u64 {
        let mut hash = Sha256::new();
        hash.update(self.target_id().as_bytes());
        (u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap())
            & crate::protocol::JS_MAX_SAFE_INTEGER)
            .max(1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WindowBounds {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Result<Self, String> {
        if ![x, y, width, height, x + width, y + height]
            .iter()
            .all(|n| n.is_finite())
            || width <= 0.0
            || height <= 0.0
            || width > 32_768.0
            || height > 32_768.0
            || x < i32::MIN as f64
            || y < i32::MIN as f64
            || x + width > i32::MAX as f64
            || y + height > i32::MAX as f64
        {
            return Err("invalid macOS window bounds".into());
        }
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn revision(self, target: WindowKey, display_revision: u64) -> u64 {
        let mut hash = Sha256::new();
        hash.update(target.pid.to_le_bytes());
        hash.update(target.wid.to_le_bytes());
        hash.update(target.birth_seconds.to_le_bytes());
        hash.update(target.birth_microseconds.to_le_bytes());
        hash.update(display_revision.to_le_bytes());
        for n in [self.x, self.y, self.width, self.height] {
            hash.update(n.to_bits().to_le_bytes());
        }
        (u64::from_le_bytes(hash.finalize()[..8].try_into().unwrap())
            & crate::protocol::JS_MAX_SAFE_INTEGER)
            .max(1)
    }
}

pub fn validate_image_size(width: usize, height: usize) -> Result<(u32, u32), String> {
    if width == 0
        || height == 0
        || width > 32_768
        || height > 32_768
        || width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(4))
            .filter(|n| *n <= MAX_IMAGE_BYTES)
            .is_none()
    {
        return Err("macOS screenshot dimensions exceed the bounded image budget".into());
    }
    Ok((width as u32, height as u32))
}

#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub target: WindowKey,
    pub bounds: WindowBounds,
    pub display_revision: u64,
    pub snapshot_id: String,
    pub width: u32,
    pub height: u32,
}

impl CapturedFrame {
    pub fn new(
        target: WindowKey,
        bounds: WindowBounds,
        display_revision: u64,
        snapshot_id: String,
        width: usize,
        height: usize,
    ) -> Result<Self, String> {
        WindowKey::new(
            target.pid,
            target.wid,
            target.birth_seconds,
            target.birth_microseconds,
        )?;
        WindowBounds::new(bounds.x, bounds.y, bounds.width, bounds.height)?;
        let (width, height) = validate_image_size(width, height)?;
        let sx = f64::from(width) / bounds.width;
        let sy = f64::from(height) / bounds.height;
        // Pixel rounding is allowed, but an unrelated desktop-sized image or
        // an image including drop shadows must not mint a coordinate frame.
        if sx < 1.0
            || sy < 1.0
            || sx > 8.0
            || sy > 8.0
            || (sx - sy).abs() > (1.0 / bounds.width + 1.0 / bounds.height)
            || snapshot_id.is_empty()
        {
            return Err("captured image does not match the selected window bounds".into());
        }
        Ok(Self {
            target,
            bounds,
            display_revision,
            snapshot_id,
            width,
            height,
        })
    }

    pub fn revision(&self) -> u64 {
        self.bounds.revision(self.target, self.display_revision)
    }

    // All seven identity/geometry fields are required at the native boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn screen_point(
        &self,
        target: WindowKey,
        snapshot_id: &str,
        revision: u64,
        current_bounds: WindowBounds,
        current_display: u64,
        x: f64,
        y: f64,
    ) -> Result<(f64, f64), String> {
        if target != self.target
            || snapshot_id != self.snapshot_id
            || revision == 0
            || revision != self.revision()
            || current_bounds != self.bounds
            || current_display != self.display_revision
        {
            return Err("stale macOS screenshot binding or geometry".into());
        }
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.0
            || y < 0.0
            || x >= f64::from(self.width)
            || y >= f64::from(self.height)
        {
            return Err("coordinate is outside the selected window image".into());
        }
        Ok((
            self.bounds.x + x * self.bounds.width / f64::from(self.width),
            self.bounds.y + y * self.bounds.height / f64::from(self.height),
        ))
    }
}

#[path = "quartz_frame/observations.rs"]
mod observations;
pub use observations::{CaptureTicket, ModelObservation, ObservationRegistry};

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> CapturedFrame {
        CapturedFrame::new(
            WindowKey::new(7, 11, 1_700_000_000, 123_456).unwrap(),
            WindowBounds::new(-1000.0, 80.0, 600.0, 400.0).unwrap(),
            9,
            "snapshot".into(),
            1200,
            800,
        )
        .unwrap()
    }

    #[test]
    fn selection_never_includes_other_onscreen_windows() {
        assert_eq!(WINDOW_CAPTURE_LIST_OPTIONS, 8);
        assert_eq!(WINDOW_CAPTURE_LIST_OPTIONS & (1 | 2 | 4 | 16), 0);
        assert_eq!(WINDOW_CAPTURE_IMAGE_OPTIONS, 9);
    }

    #[test]
    fn target_parser_rejects_ambiguous_or_desktop_ids() {
        assert_eq!(
            WindowKey::parse("mac:7:11:1700000000:123456").unwrap(),
            frame().target
        );
        for id in [
            "mac:7:11",
            "mac:0:11:1700000000:123456",
            "mac:2147483648:11:1700000000:123456",
            "mac:7:0:1700000000:123456",
            "mac:07:11:1700000000:123456",
            "mac:+7:11:1700000000:123456",
            "mac:7:011:1700000000:123456",
            "mac:7:11:1700000000",
            "mac:7:11:0:123456",
            "mac:7:11:1700000000:1000000",
            "mac:7:11:01700000000:123456",
            "mac:7:11:1700000000:0123456",
            "mac:7:11:1700000000:+123456",
            "mac:7:11:1700000000:123456:extra",
            "mac:0:11",
            "mac:7:0",
            "mac:7:11:other",
            "mac:07:11",
            "mac:+7:11",
            "mac:2147483648:11",
            "mac:-1:11",
            "mac:7:",
            "7:11",
        ] {
            assert!(WindowKey::parse(id).is_err(), "{id}");
        }
    }

    #[test]
    fn window_instance_parser_requires_a_canonical_nonzero_nonce() {
        let instance = WindowInstance {
            window: frame().target,
            nonce: uuid::Uuid::parse_str("abcdefab-1234-4234-8234-abcdefabcdef").unwrap(),
        };
        assert_eq!(
            WindowInstance::parse(&instance.target_id()).unwrap(),
            instance
        );
        for suffix in [
            "",
            "0",
            "00000000000000000000000000000000",
            "abcdefab-1234-4234-8234-abcdefabcdef",
            "ABCDEFAB123442348234ABCDEFABCDEFAB",
            "abcdefab123442348234abcdefabcdef:extra",
        ] {
            assert!(
                WindowInstance::parse(&format!("{}:{suffix}", instance.window.target_id()))
                    .is_err(),
                "{suffix}"
            );
        }
        assert!(WindowInstance::parse(&instance.window.target_id()).is_err());
        assert!(
            WindowInstance::parse(&instance.target_id().replacen("mac:7:", "mac:07:", 1)).is_err()
        );
    }

    #[test]
    fn separate_window_instances_cannot_share_frame_authority() {
        let first = WindowInstance::fresh(frame().target);
        let second = WindowInstance::fresh(frame().target);
        assert_ne!(first, second);
        assert_ne!(first.lifecycle_stamp(), second.lifecycle_stamp());
        assert_eq!(
            first.lifecycle_stamp() as f64 as u64,
            first.lifecycle_stamp()
        );
        let registry = ObservationRegistry::default();
        let ticket = registry.begin("run", &first.target_id());
        registry.publish(ticket, frame(), true, ()).unwrap();
        assert!(registry.get("run", &first.target_id()).is_ok());
        assert!(registry.get("run", &second.target_id()).is_err());
        assert!(registry.get("run", &first.window.target_id()).is_err());
        let ticket = registry.begin("run", &second.target_id());
        registry.retire_target("run", &first.target_id());
        registry.publish(ticket, frame(), true, ()).unwrap();
        assert!(registry.get("run", &first.target_id()).is_err());
        assert!(registry.get("run", &second.target_id()).is_ok());
    }

    #[test]
    fn instance_frame_publish_still_checks_full_process_birth_and_window_key() {
        let registry = ObservationRegistry::default();
        let instance = WindowInstance::fresh(frame().target);
        let ticket = registry.begin("run", &instance.target_id());
        let mut wrong = frame();
        wrong.target.birth_microseconds += 1;
        assert!(registry.publish(ticket, wrong, true, ()).is_err());
        assert!(registry.get("run", &instance.target_id()).is_err());
    }

    #[test]
    fn frame_registry_rejects_legacy_process_window_keys_without_instance_authority() {
        let registry = ObservationRegistry::default();
        let legacy = frame().target.target_id();
        let ticket = registry.begin("run", &legacy);
        assert!(registry.publish(ticket, frame(), true, ()).is_err());
        assert!(registry.get("run", &legacy).is_err());
    }

    #[test]
    fn retina_pixels_map_to_quartz_points_with_negative_display_origin() {
        let f = frame();
        assert_eq!(
            f.screen_point(
                f.target,
                "snapshot",
                f.revision(),
                f.bounds,
                9,
                600.0,
                400.0
            )
            .unwrap(),
            (-700.0, 280.0)
        );
    }

    #[test]
    fn process_birth_is_exact_in_ids_frames_and_lifecycle_revisions() {
        let f = frame();
        for target in [
            WindowKey {
                birth_seconds: f.target.birth_seconds + 1,
                ..f.target
            },
            WindowKey {
                birth_microseconds: f.target.birth_microseconds + 1,
                ..f.target
            },
        ] {
            assert_ne!(target.target_id(), f.target.target_id());
            assert_ne!(target.lifecycle_stamp(), f.target.lifecycle_stamp());
            assert!(target.lifecycle_stamp() <= crate::protocol::JS_MAX_SAFE_INTEGER);
            assert_ne!(f.bounds.revision(target, f.display_revision), f.revision());
            assert!(f
                .screen_point(target, "snapshot", f.revision(), f.bounds, 9, 1.0, 1.0)
                .is_err());
        }
        let large = WindowKey::new(7, 11, u64::MAX, 999_999).unwrap();
        assert_eq!(WindowKey::parse(&large.target_id()).unwrap(), large);
        let zero_microseconds = WindowKey::new(7, 11, 1_700_000_000, 0).unwrap();
        assert_eq!(
            WindowKey::parse(&zero_microseconds.target_id()).unwrap(),
            zero_microseconds
        );
        let invalid = WindowKey {
            birth_microseconds: 1_000_000,
            ..f.target
        };
        assert!(CapturedFrame::new(invalid, f.bounds, 9, "snapshot".into(), 1200, 800).is_err());
    }

    #[test]
    fn revision_survives_json_to_javascript_number_roundtrip() {
        let revision = frame().revision();
        assert!(revision <= crate::protocol::JS_MAX_SAFE_INTEGER);
        assert_eq!(revision as f64 as u64, revision);
    }

    #[test]
    fn frame_constructor_revalidates_public_bounds() {
        let f = frame();
        assert!(CapturedFrame::new(
            f.target,
            WindowBounds {
                width: f64::NAN,
                ..f.bounds
            },
            9,
            "snapshot".into(),
            1200,
            800
        )
        .is_err());
    }

    #[test]
    fn stale_frame_identity_move_resize_and_display_change_are_rejected() {
        let f = frame();
        assert!(f
            .screen_point(
                WindowKey { pid: 8, ..f.target },
                "snapshot",
                f.revision(),
                f.bounds,
                9,
                1.0,
                1.0
            )
            .is_err());
        assert!(f
            .screen_point(f.target, "old", f.revision(), f.bounds, 9, 1.0, 1.0)
            .is_err());
        assert!(f
            .screen_point(f.target, "snapshot", 0, f.bounds, 9, 1.0, 1.0)
            .is_err());
        for bounds in [
            WindowBounds {
                x: -999.0,
                ..f.bounds
            },
            WindowBounds {
                width: 601.0,
                ..f.bounds
            },
        ] {
            assert_ne!(bounds.revision(f.target, 9), f.revision());
            assert!(f
                .screen_point(f.target, "snapshot", f.revision(), bounds, 9, 1.0, 1.0)
                .is_err());
        }
        assert!(f
            .screen_point(f.target, "snapshot", f.revision(), f.bounds, 10, 1.0, 1.0)
            .is_err());
    }

    #[test]
    fn invalid_coordinates_never_escape_the_capture() {
        let f = frame();
        for (x, y) in [
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
            (-0.1, 0.0),
            (1200.0, 0.0),
            (0.0, 800.0),
        ] {
            assert!(f
                .screen_point(f.target, "snapshot", f.revision(), f.bounds, 9, x, y)
                .is_err());
        }
    }

    #[test]
    fn malformed_bounds_and_oversized_images_fail_before_copying() {
        for (w, h) in [(0, 1), (1, 0), (usize::MAX, 4), (32768, 32768)] {
            assert!(validate_image_size(w, h).is_err());
        }
        for (x, y, w, h) in [
            (0.0, 0.0, 0.0, 3.0),
            (f64::INFINITY, 0.0, 1.0, 1.0),
            (0.0, 0.0, f64::NAN, 3.0),
            (i32::MAX as f64, 0.0, 20.0, 30.0),
        ] {
            assert!(WindowBounds::new(x, y, w, h).is_err());
        }
    }

    #[test]
    fn full_desktop_or_shadow_sized_image_cannot_be_used_as_window_frame() {
        let f = frame();
        assert!(CapturedFrame::new(f.target, f.bounds, 9, "s".into(), 3840, 2160).is_err());
        assert!(CapturedFrame::new(f.target, f.bounds, 9, "s".into(), 640, 440).is_err());
    }

    #[test]
    fn failed_model_capture_retires_prior_frame_without_affecting_another_run() {
        let registry = ObservationRegistry::default();
        let id = WindowInstance::fresh(frame().target).target_id();
        for run in ["one", "two"] {
            let ticket = registry.begin(run, &id);
            registry.publish(ticket, frame(), true, ()).unwrap();
        }
        let _failed = registry.begin("one", &id);
        assert!(registry.get("one", &id).is_err());
        assert!(registry.get("two", &id).is_ok());
    }

    #[test]
    fn stop_and_replacement_prevent_late_capture_from_restoring_authority() {
        let registry = ObservationRegistry::default();
        let id = WindowInstance::fresh(frame().target).target_id();
        let stopped = registry.begin("run", &id);
        registry.retire_run("run");
        assert!(registry.publish(stopped, frame(), true, ()).is_err());
        let old = registry.begin("run", &id);
        let new = registry.begin("run", &id);
        assert!(registry.publish(old, frame(), true, ()).is_err());
        registry.publish(new, frame(), true, ()).unwrap();
        assert!(registry.get("other", &id).is_err());
        registry.retire_target("run", &id);
        assert!(registry.get("run", &id).is_err());
    }
}
