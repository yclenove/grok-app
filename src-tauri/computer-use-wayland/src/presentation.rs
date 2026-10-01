//! Upright, content-only screenshots paired with their original input authority.
//! SPA describes the transform ALREADY applied to the buffer. Presentation must
//! undo it (including reflection order), not rotate the image a second time.
use crate::{frame::validate_size, CapturedFrame, InputAction, ObservedFrame, VideoTransform};
use std::sync::Arc;

/// Immutable packed RGBA image. Dimensions describe exactly these bytes, not
/// the padded PipeWire allocation or the compositor's logical coordinate space.
pub struct PresentedImage {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}
impl PresentedImage {
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(crate) fn render(
        frame: &CapturedFrame,
        max_width: u32,
        max_height: u32,
    ) -> Result<Self, String> {
        validate_size(max_width, max_height)?;
        validate_size(frame.buffer_width, frame.buffer_height)?;
        let crop = frame.region;
        validate_size(crop.width, crop.height)?;
        if crop
            .x
            .checked_add(crop.width)
            .is_none_or(|v| v > frame.buffer_width)
            || crop
                .y
                .checked_add(crop.height)
                .is_none_or(|v| v > frame.buffer_height)
            || frame.rgba.len() != crop.width as usize * crop.height as usize * 4
        {
            return Err("invalid capture geometry or packed pixel length".into());
        }
        use VideoTransform::*;
        let swapped = matches!(
            frame.transform,
            Rotate90 | Rotate270 | Flipped90 | Flipped270
        );
        let (uw, uh) = if swapped {
            (crop.height, crop.width)
        } else {
            (crop.width, crop.height)
        };
        // Integer aspect fit, never upscale, never silently exceed either limit.
        let (width, height) = if uw <= max_width && uh <= max_height {
            (uw, uh)
        } else if u64::from(uw) * u64::from(max_height) > u64::from(uh) * u64::from(max_width) {
            (
                max_width,
                (u64::from(uh) * u64::from(max_width) / u64::from(uw)).max(1) as u32,
            )
        } else {
            (
                (u64::from(uw) * u64::from(max_height) / u64::from(uh)).max(1) as u32,
                max_height,
            )
        };
        if frame.transform == Normal && width == crop.width && height == crop.height {
            return Ok(Self {
                width,
                height,
                rgba: frame.rgba.clone(),
            });
        }
        let bytes = width as usize * height as usize * 4;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(bytes)
            .map_err(|_| "screenshot allocation failed")?;
        rgba.resize(bytes, 0);
        for y in 0..height {
            for x in 0..width {
                // Nearest-neighbour sample at each destination pixel's centre.
                let ux = ((2 * u64::from(x) + 1) * u64::from(uw) / (2 * u64::from(width))) as u32;
                let uy = ((2 * u64::from(y) + 1) * u64::from(uh) / (2 * u64::from(height))) as u32;
                // Map upright destination -> already-transformed source. This
                // inverse sampling avoids holes and preserves all four channels.
                let (sx, sy) = match frame.transform {
                    Normal => (ux, uy),
                    Rotate90 => (uy, crop.height - 1 - ux),
                    Rotate180 => (crop.width - 1 - ux, crop.height - 1 - uy),
                    Rotate270 => (crop.width - 1 - uy, ux),
                    Flipped => (crop.width - 1 - ux, uy),
                    Flipped90 => (uy, ux),
                    Flipped180 => (ux, crop.height - 1 - uy),
                    Flipped270 => (crop.width - 1 - uy, crop.height - 1 - ux),
                };
                let src = (sy as usize * crop.width as usize + sx as usize) * 4;
                let dst = (y as usize * width as usize + x as usize) * 4;
                rgba[dst..dst + 4].copy_from_slice(&frame.rgba[src..src + 4]);
            }
        }
        Ok(Self {
            width,
            height,
            rgba: rgba.into(),
        })
    }
}

/// One-use observation and the exact image shown to the consumer. Callers cannot
/// substitute dimensions/bytes or combine another screenshot with this ticket.
/// A Host still must bind model responses to this object and validate intent.
pub struct PresentedFrame {
    observation: ObservedFrame,
    image: PresentedImage,
}
impl ObservedFrame {
    /// Undo SPA orientation and fit the valid content within these pixel limits.
    /// No padding, letterboxing, upscaling, desktop fallback or implicit DPI guess.
    /// Presentation never refreshes the original observation's action deadline
    /// or its source image's delivery timestamp.
    pub fn present(self, max_width: u32, max_height: u32) -> Result<PresentedFrame, String> {
        let image = PresentedImage::render(self.frame(), max_width, max_height)?;
        Ok(PresentedFrame {
            observation: self,
            image,
        })
    }
}
impl PresentedFrame {
    pub(crate) fn observation(&self) -> &ObservedFrame {
        &self.observation
    }
    pub(crate) fn resize(self, width: u32, height: u32) -> Result<Self, String> {
        self.observation.present(width, height)
    }
    pub fn image(&self) -> &PresentedImage {
        &self.image
    }
    pub fn source_frame(&self) -> &CapturedFrame {
        self.observation.frame()
    }

    /// Consume the original observation for non-positional input. This does not
    /// mint a new ticket or bypass the EI capability/generation/frame checks.
    pub(crate) fn into_observation(self) -> ObservedFrame {
        self.observation
    }

    pub(crate) fn into_pixel_input(
        self,
        x: u32,
        y: u32,
    ) -> Result<(ObservedFrame, InputAction), String> {
        if x >= self.image.width || y >= self.image.height {
            return Err("pixel outside the presented screenshot".into());
        }
        let region = self
            .observation
            .input_capabilities()
            .absolute_region
            .as_ref()
            .ok_or("no uniquely paired absolute EI region")?;
        if region.width == 0 || region.height == 0 {
            return Err("empty absolute EI region".into());
        }
        // VideoCrop defines valid content, not a desktop offset. The upright
        // content maps to the paired logical EI region; buffer padding/crop x/y,
        // portal compositor position/size, and physical_scale are NOT extra
        // translation/scale factors. The EI owner adds its region origin ONCE.
        let lx = (f64::from(x) + 0.5) * f64::from(region.width) / f64::from(self.image.width);
        let ly = (f64::from(y) + 0.5) * f64::from(region.height) / f64::from(self.image.height);
        let (wx, wy) = region.wire_position(lx, ly)?;
        // A very distant region origin may make the protocol's float32 spacing
        // larger than a screenshot pixel. Never silently aim at another pixel.
        let px = (wx - f64::from(region.x)) * f64::from(self.image.width) / f64::from(region.width);
        let py =
            (wy - f64::from(region.y)) * f64::from(self.image.height) / f64::from(region.height);
        if px < f64::from(x)
            || px >= f64::from(x) + 1.0
            || py < f64::from(y)
            || py >= f64::from(y) + 1.0
        {
            return Err("EI wire precision cannot address the selected screenshot pixel".into());
        }
        let action = InputAction::Absolute { x: lx, y: ly };
        Ok((self.observation, action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PixelRegion;
    use std::time::Instant;

    fn frame(transform: VideoTransform) -> CapturedFrame {
        CapturedFrame {
            run_id: "render".into(),
            node_id: 1,
            pipewire_serial: 2,
            sequence: 3,
            format_generation: 1,
            region: PixelRegion {
                x: 2,
                y: 1,
                width: 3,
                height: 2,
            },
            buffer_width: 8,
            buffer_height: 4,
            transform,
            captured_at: Instant::now(),
            rgba: (1u8..=6)
                .flat_map(|v| [v, v + 10, v + 20, v + 30])
                .collect::<Vec<_>>()
                .into(),
        }
    }
    #[test]
    fn all_eight_inverse_orientations_match_independent_asymmetric_matrices() {
        use VideoTransform::*;
        // Literal expected matrices, not a round-trip through production math.
        let cases = [
            (Normal, 3, 2, [1, 2, 3, 4, 5, 6]),
            (Rotate90, 2, 3, [4, 1, 5, 2, 6, 3]),
            (Rotate180, 3, 2, [6, 5, 4, 3, 2, 1]),
            (Rotate270, 2, 3, [3, 6, 2, 5, 1, 4]),
            (Flipped, 3, 2, [3, 2, 1, 6, 5, 4]),
            (Flipped90, 2, 3, [1, 4, 2, 5, 3, 6]),
            (Flipped180, 3, 2, [4, 5, 6, 1, 2, 3]),
            (Flipped270, 2, 3, [6, 3, 5, 2, 4, 1]),
        ];
        for (transform, width, height, values) in cases {
            let image = PresentedImage::render(&frame(transform), 8192, 8192).unwrap();
            assert_eq!(
                (image.width(), image.height()),
                (width, height),
                "{transform:?}"
            );
            let expected: Vec<_> = values
                .into_iter()
                .flat_map(|v| [v, v + 10, v + 20, v + 30])
                .collect();
            assert_eq!(image.rgba(), expected, "{transform:?}");
        }
    }
    #[test]
    fn scaling_uses_oriented_dimensions_and_centres_without_upscaling() {
        use VideoTransform::*;
        for (transform, w, h, samples) in [
            (Normal, 2, 1, [4, 6]),
            (Rotate90, 1, 2, [1, 3]),
            (Rotate180, 2, 1, [3, 1]),
            (Rotate270, 1, 2, [6, 4]),
            (Flipped, 2, 1, [6, 4]),
            (Flipped90, 1, 2, [4, 6]),
            (Flipped180, 2, 1, [1, 3]),
            (Flipped270, 1, 2, [3, 1]),
        ] {
            let image = PresentedImage::render(&frame(transform), 2, 2).unwrap();
            assert_eq!((image.width(), image.height()), (w, h));
            let expected: Vec<u8> = samples
                .into_iter()
                .flat_map(|v| [v, v + 10, v + 20, v + 30])
                .collect();
            assert_eq!(image.rgba(), expected, "{transform:?}");
        }
        let f = frame(VideoTransform::Normal);
        let image = PresentedImage::render(&f, 2, 2).unwrap();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.rgba(), [4, 14, 24, 34, 6, 16, 26, 36]);
        let image = PresentedImage::render(&frame(VideoTransform::Rotate90), 2, 2).unwrap();
        assert_eq!((image.width(), image.height()), (1, 2));
        assert_eq!(image.rgba(), [1, 11, 21, 31, 3, 13, 23, 33]);
        let image = PresentedImage::render(&f, 1, 1).unwrap();
        assert_eq!(image.rgba(), [5, 15, 25, 35]);
        let image = PresentedImage::render(&f, 8192, 8192).unwrap();
        assert!(Arc::ptr_eq(&f.rgba, &image.rgba));
    }
    #[test]
    fn corrupt_geometry_lengths_and_limits_are_rejected_before_allocation() {
        for (w, h) in [(0, 1), (1, 0), (8193, 1), (u32::MAX, u32::MAX)] {
            assert!(PresentedImage::render(&frame(VideoTransform::Normal), w, h).is_err());
        }
        let mut f = frame(VideoTransform::Normal);
        f.rgba = Arc::from([0; 4]);
        assert!(PresentedImage::render(&f, 10, 10).is_err());
        let mut f = frame(VideoTransform::Normal);
        f.region.x = u32::MAX;
        assert!(PresentedImage::render(&f, 10, 10).is_err());
        f.region.x = 7;
        assert!(PresentedImage::render(&f, 10, 10).is_err());
        f.region.x = 0;
        f.region.height = 0;
        assert!(PresentedImage::render(&f, 10, 10).is_err());
    }
}
