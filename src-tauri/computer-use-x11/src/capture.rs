use crate::client::{err, Client, Geometry};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};

impl Client {
    pub fn capture(&mut self, id: &str) -> Result<(Geometry, Vec<u8>), String> {
        let (g, raw, layout) = self.fenced(|c| {
            let g = c.geometry(id)?;
            if u64::from(g.width) * u64::from(g.height) > 16_777_216 {
                return Err("target capture exceeds the pixel budget".into());
            }
            let image = c
                .conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    g.window,
                    0,
                    0,
                    g.width,
                    g.height,
                    u32::MAX,
                )
                .map_err(err)?
                .reply()
                .map_err(err)?;
            let setup = c.conn.setup();
            let format = setup
                .pixmap_formats
                .iter()
                .find(|f| f.depth == image.depth)
                .ok_or("native pixel format unavailable")?;
            let visual = setup
                .roots
                .iter()
                .flat_map(|s| &s.allowed_depths)
                .flat_map(|d| &d.visuals)
                .find(|v| v.visual_id == image.visual)
                .ok_or("native visual unavailable")?;
            if image.visual != g.visual || image.depth != g.depth {
                return Err("native visual changed during capture".into());
            }
            let layout = PixelLayout {
                width: g.width,
                height: g.height,
                bits: format.bits_per_pixel,
                pad: format.scanline_pad,
                little_endian: setup.image_byte_order == ImageOrder::LSB_FIRST,
                masks: [visual.red_mask, visual.green_mask, visual.blue_mask],
            };
            if c.geometry(id)? != g {
                return Err("target changed during native capture".into());
            }
            Ok((g, image.data, layout))
        })?;
        let rgb = decode(&raw, layout)?;
        let image = image::RgbImage::from_raw(u32::from(g.width), u32::from(g.height), rgb)
            .ok_or("invalid decoded image dimensions")?;
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(err)?;
        Ok((g, png))
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PixelLayout {
    pub width: u16,
    pub height: u16,
    pub bits: u8,
    pub pad: u8,
    pub little_endian: bool,
    pub masks: [u32; 3],
}

pub(crate) fn decode(raw: &[u8], layout: PixelLayout) -> Result<Vec<u8>, String> {
    let PixelLayout {
        width,
        height,
        bits,
        pad,
        little_endian,
        masks,
    } = layout;
    if width == 0 || height == 0 || ![16, 24, 32].contains(&bits) || ![8, 16, 32].contains(&pad) {
        return Err("unsupported native image layout".into());
    }
    let pixels = usize::from(width) * usize::from(height);
    if pixels > 16_777_216 {
        return Err("native pixel budget exceeded".into());
    }
    let row_bits = usize::from(width) * usize::from(bits);
    let stride = row_bits.div_ceil(usize::from(pad)) * usize::from(pad) / 8;
    if raw.len() < stride * usize::from(height) {
        return Err("truncated native image".into());
    }
    for (index, mask) in masks.into_iter().enumerate() {
        let shifted = mask.checked_shr(mask.trailing_zeros()).unwrap_or(0);
        if mask == 0
            || (bits < 32 && mask >> bits != 0)
            || shifted & shifted.wrapping_add(1) != 0
            || masks[..index].iter().any(|other| other & mask != 0)
        {
            return Err("unsupported native visual masks".into());
        }
    }
    let mut rgb = Vec::with_capacity(pixels * 3);
    let bytes = usize::from(bits / 8);
    for row in raw[..stride * usize::from(height)].chunks_exact(stride) {
        for pixel in row[..usize::from(width) * bytes].chunks_exact(bytes) {
            let value = if little_endian {
                pixel
                    .iter()
                    .enumerate()
                    .fold(0u32, |v, (i, b)| v | (u32::from(*b) << (8 * i)))
            } else {
                pixel.iter().fold(0u32, |v, b| (v << 8) | u32::from(*b))
            };
            for mask in masks {
                let shift = mask.trailing_zeros();
                let channel = (value & mask) >> shift;
                let max = mask >> shift;
                rgb.push(((u64::from(channel) * 255 + u64::from(max) / 2) / u64::from(max)) as u8);
            }
        }
    }
    Ok(rgb)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_pixels_honor_padding_endian_and_visual_masks() {
        let layout = PixelLayout {
            width: 1,
            height: 2,
            bits: 24,
            pad: 32,
            little_endian: true,
            masks: [0xff0000, 0xff00, 0xff],
        };
        assert_eq!(
            decode(&[3, 2, 1, 99, 6, 5, 4, 98], layout).unwrap(),
            [1, 2, 3, 4, 5, 6]
        );
        assert_eq!(
            decode(
                &[1, 2, 3, 99, 4, 5, 6, 98],
                PixelLayout {
                    little_endian: false,
                    ..layout
                }
            )
            .unwrap(),
            [1, 2, 3, 4, 5, 6]
        );
        let rgb565 = PixelLayout {
            width: 2,
            height: 1,
            bits: 16,
            pad: 32,
            little_endian: true,
            masks: [0xf800, 0x7e0, 0x1f],
        };
        assert_eq!(
            decode(&[0, 248, 224, 7], rgb565).unwrap(),
            [255, 0, 0, 0, 255, 0]
        );
    }
    #[test]
    fn malformed_native_buffers_are_not_synthetic_black_successes() {
        let layout = PixelLayout {
            width: 1,
            height: 1,
            bits: 32,
            pad: 32,
            little_endian: true,
            masks: [0xff0000, 0xff00, 0xff],
        };
        assert!(decode(&[0; 3], layout).is_err());
        assert!(decode(
            &[0; 4],
            PixelLayout {
                masks: [0xff, 0xff, 0xff],
                ..layout
            }
        )
        .is_err());
        assert!(decode(
            &[0; 4],
            PixelLayout {
                masks: [0x0500, 0xf0, 0xf],
                ..layout
            }
        )
        .is_err());
        assert!(decode(&[0; 4], PixelLayout { bits: 8, ..layout }).is_err());
    }
}
