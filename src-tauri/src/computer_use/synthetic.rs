//! Synthetic screenshot used by P0 image/coord probes. Oracle stays out of tool text.

use std::io::Cursor;

use image::{ImageBuffer, ImageFormat, Rgb, RgbImage};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DIGIT_FONT: [[u8; 5]; 10] = [
    [0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
    [0b00100, 0b01100, 0b00100, 0b00100, 0b01110],
    [0b01110, 0b10001, 0b00010, 0b00100, 0b11111],
    [0b11110, 0b00001, 0b01110, 0b00001, 0b11110],
    [0b10001, 0b10001, 0b11111, 0b00001, 0b00001],
    [0b11111, 0b10000, 0b11110, 0b00001, 0b11110],
    [0b01110, 0b10000, 0b11110, 0b10001, 0b01110],
    [0b11111, 0b00001, 0b00010, 0b00100, 0b00100],
    [0b01110, 0b10001, 0b01110, 0b10001, 0b01110],
    [0b01110, 0b10001, 0b01111, 0b00001, 0b01110],
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticOracle {
    pub seed: u64,
    pub code: String,
    pub target_cx: u32,
    pub target_cy: u32,
    pub box_x: u32,
    pub box_y: u32,
    pub box_w: u32,
    pub box_h: u32,
    pub width: u32,
    pub height: u32,
    pub sha256: String,
}

fn draw_digit(
    img: &mut RgbImage,
    origin_x: u32,
    origin_y: u32,
    digit: u8,
    scale: u32,
    color: Rgb<u8>,
) {
    let glyph = DIGIT_FONT[digit as usize];
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5u32 {
            if bits & (1 << (4 - col)) != 0 {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let x = origin_x + col * scale + dx;
                        let y = origin_y + (row as u32) * scale + dy;
                        if x < img.width() && y < img.height() {
                            img.put_pixel(x, y, color);
                        }
                    }
                }
            }
        }
    }
}

pub fn render_synthetic_png(seed: u64, width: u32, height: u32) -> (Vec<u8>, SyntheticOracle) {
    let mut rng = StdRng::seed_from_u64(seed);
    let code = format!("{:04}", rng.gen_range(0..10000u32));
    let box_w = 120u32;
    let box_h = 80u32;
    let box_x = rng.gen_range(20..width.saturating_sub(box_w + 20).max(21));
    let box_y = rng.gen_range(20..height.saturating_sub(box_h + 20).max(21));
    let mut img: RgbImage = ImageBuffer::from_pixel(width, height, Rgb([24, 28, 36]));
    for x in (0..width).step_by(40) {
        for y in 0..height {
            img.put_pixel(x, y, Rgb([48, 54, 64]));
        }
    }
    for y in (0..height).step_by(40) {
        for x in 0..width {
            img.put_pixel(x, y, Rgb([48, 54, 64]));
        }
    }
    for y in box_y..box_y + box_h {
        for x in box_x..box_x + box_w {
            img.put_pixel(x, y, Rgb([180, 32, 32]));
        }
    }
    let scale = 6u32;
    let digit_w = 5 * scale + 4;
    let start_x = box_x + 8;
    let start_y = box_y + 16;
    for (i, ch) in code.chars().enumerate() {
        let d = ch.to_digit(10).unwrap_or(0) as u8;
        draw_digit(
            &mut img,
            start_x + i as u32 * digit_w,
            start_y,
            d,
            scale,
            Rgb([255, 255, 255]),
        );
    }
    // Marker pixels for coord mapping (top-left lime, bottom-right magenta).
    img.put_pixel(0, 0, Rgb([0, 255, 0]));
    img.put_pixel(width - 1, height - 1, Rgb([255, 0, 255]));

    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .expect("png encode");
    let sha = hex::encode(Sha256::digest(&png));
    let oracle = SyntheticOracle {
        seed,
        code,
        target_cx: box_x + box_w / 2,
        target_cy: box_y + box_h / 2,
        box_x,
        box_y,
        box_w,
        box_h,
        width,
        height,
        sha256: sha,
    };
    (png, oracle)
}

/// Model-visible tool text. Must not contain the oracle code or coordinates.
pub fn model_visible_observe_text(content_id: &str, width: u32, height: u32) -> String {
    serde_json::json!({
        "snapshotId": content_id,
        "coordinateSpace": "image_pixels",
        "image": { "width": width, "height": height, "contentId": content_id },
        "truncated": false
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_absent_from_tool_text() {
        let (png, oracle) = render_synthetic_png(42, 400, 300);
        assert!(png.starts_with(&[137, 80, 78, 71]));
        let text = model_visible_observe_text("cid-1", oracle.width, oracle.height);
        assert!(!text.contains(&oracle.code), "oracle leaked into tool text");
        assert!(!text.contains(&oracle.target_cx.to_string()) || oracle.target_cx < 10);
        assert!(!text.contains("oracle"));
        assert!(!text.contains(&oracle.sha256));
        let (png2, oracle2) = render_synthetic_png(42, 400, 300);
        assert_eq!(oracle.code, oracle2.code);
        assert_eq!(png, png2);
    }
}
