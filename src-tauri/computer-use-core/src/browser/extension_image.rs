//! Bound the encoded bytes, decoded pixels and geometry before accepting an image.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExtensionScreenshot {
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
}

impl ExtensionScreenshot {
    pub(super) fn validate(&self, viewport_width: u32, viewport_height: u32) -> bool {
        if self.png_base64.len() > crate::protocol::OBSERVATION_PNG_B64_CAP
            || !(1..=1280).contains(&self.width)
            || !(1..=1280).contains(&self.height)
            || !(1..=8192).contains(&self.source_width)
            || !(1..=8192).contains(&self.source_height)
            || u64::from(self.source_width) * u64::from(self.source_height) > 16_777_216
            || self.width > self.source_width
            || self.height > self.source_height
            || !same_aspect(
                self.width,
                self.height,
                self.source_width,
                self.source_height,
            )
            || !same_aspect(
                self.source_width,
                self.source_height,
                viewport_width,
                viewport_height,
            )
        {
            return false;
        }
        let Ok(bytes) = STANDARD.decode(&self.png_base64) else {
            return false;
        };
        if !bounded_chunks(&bytes) {
            return false;
        }
        let mut options = png::DecodeOptions::default();
        options.set_skip_ancillary_crc_failures(false);
        let mut decoder = png::Decoder::new_with_options(Cursor::new(bytes), options);
        decoder.set_limits(png::Limits {
            bytes: 16 * 1024 * 1024,
        });
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        let Ok(mut reader) = decoder.read_info() else {
            return false;
        };
        let info = reader.info();
        if info.width != self.width
            || info.height != self.height
            || info.bit_depth != png::BitDepth::Eight
            || !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba)
            || info.animation_control.is_some()
            || info.interlaced
        {
            return false;
        }
        let Some(size) = reader
            .output_buffer_size()
            .filter(|size| *size <= 1280 * 1280 * 4)
        else {
            return false;
        };
        reader.next_frame(&mut vec![0; size]).is_ok() && reader.finish().is_ok()
    }
}

fn bounded_chunks(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8;
    for _ in 0..2048 {
        let Some(header) = bytes.get(offset..offset + 8) else {
            return false;
        };
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        let Some(end) = offset
            .checked_add(12)
            .and_then(|start| start.checked_add(size))
        else {
            return false;
        };
        if end > bytes.len() {
            return false;
        }
        // Only the small set emitted by a normalized canvas is accepted. No text,
        // compressed profiles, APNG frames, unknown chunks or bytes following IEND.
        match &header[4..] {
            b"IHDR" if offset == 8 && size == 13 => {}
            b"IDAT" | b"sRGB" | b"gAMA" | b"cHRM" | b"pHYs" => {}
            b"IEND" => return size == 0 && end == bytes.len(),
            _ => return false,
        }
        offset = end;
    }
    false
}

fn same_aspect(w: u32, h: u32, other_w: u32, other_h: u32) -> bool {
    (u64::from(w) * u64::from(other_h)).abs_diff(u64::from(h) * u64::from(other_w))
        <= u64::from(other_w) + u64::from(other_h)
}

#[cfg(test)]
pub(super) fn fixture_image() -> ExtensionScreenshot {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 16, 12);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[127; 16 * 12 * 4]).unwrap();
    }
    ExtensionScreenshot {
        png_base64: STANDARD.encode(bytes),
        width: 16,
        height: 12,
        source_width: 800,
        source_height: 600,
    }
}

#[cfg(test)]
pub(crate) fn fixture_noisy_image() -> ExtensionScreenshot {
    let mut pixels = vec![0; 300 * 225 * 3];
    let mut random = 17u32;
    for pixel in &mut pixels {
        random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *pixel = (random >> 24) as u8;
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 300, 225);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    ExtensionScreenshot {
        png_base64: STANDARD.encode(bytes),
        width: 300,
        height: 225,
        source_width: 800,
        source_height: 600,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_requires_complete_decodable_png_not_just_a_header() {
        let image = fixture_image();
        assert!(image.validate(800, 600));
        let bytes = STANDARD.decode(&image.png_base64).unwrap();
        for kind in [
            "header-only",
            "crc",
            "trailing",
            "double-end",
            "bad-base64",
            "wrong-dimensions",
            "wrong-aspect",
            "oversize",
        ] {
            let mut invalid = image.clone();
            let mut damaged = bytes.clone();
            match kind {
                "header-only" => damaged.truncate(33),
                "crc" => damaged[29] ^= 1,
                "trailing" => damaged.push(0),
                "double-end" => damaged.extend_from_slice(&bytes[bytes.len() - 12..]),
                "bad-base64" => invalid.png_base64.push('!'),
                "wrong-dimensions" => invalid.width += 1,
                "wrong-aspect" => invalid.source_height = 400,
                "oversize" => invalid.width = 1281,
                _ => unreachable!(),
            }
            if !matches!(
                kind,
                "bad-base64" | "wrong-dimensions" | "wrong-aspect" | "oversize"
            ) {
                invalid.png_base64 = STANDARD.encode(damaged);
            }
            assert!(!invalid.validate(800, 600), "{kind}");
        }
    }
}
