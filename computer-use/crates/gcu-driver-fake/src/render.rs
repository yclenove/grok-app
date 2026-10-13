//! Small deterministic fixture rasterizer, independent of core's text/tree renderer.
use gcu_core::{
    driver::{Capture, DriverError, Result},
    model::{Geometry, NodeValue, RawNode, Rect, Role, States},
};

/// Deliberately conspicuous password-area color for downstream redaction tests.
/// Fake screenshots contain this marker, never plaintext password glyphs.
pub const PASSWORD_COLOR: [u8; 4] = [255, 0, 180, 255];
/// Ordinary editable-field background, used by coordinate alignment tests.
pub const FIELD_COLOR: [u8; 4] = [220, 240, 255, 255];

pub(crate) fn capture(root: Option<&RawNode>, geometry: Geometry) -> Result<Capture> {
    let width = geometry.frame.w as u32;
    let height = geometry.frame.h as u32;
    let mut canvas = Canvas {
        width,
        height,
        pixels: vec![245; width as usize * height as usize * 4],
    };
    for pixel in canvas.pixels.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    if let Some(root) = root {
        canvas.node(
            root,
            Rect {
                x: 0.0,
                y: 0.0,
                w: f64::from(width),
                h: f64::from(height),
            },
        );
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(encoding_error)?;
        writer
            .write_image_data(&canvas.pixels)
            .map_err(encoding_error)?;
        writer.finish().map_err(encoding_error)?;
    }
    Ok(Capture {
        bytes,
        width,
        height,
        mime: "image/png".into(),
        geometry,
    })
}
fn encoding_error(_: png::EncodingError) -> DriverError {
    DriverError::Native {
        message: "PNG encoding failed".into(),
        maybe_executed: false,
    }
}
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
impl Canvas {
    fn pixel(&mut self, x: i64, y: i64, color: [u8; 4]) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].copy_from_slice(&color);
    }
    fn node(&mut self, node: &RawNode, clip: Rect) {
        if node.states.contains(States::OFFSCREEN) {
            return;
        }
        let clip = node.frame.map_or(clip, |frame| intersect(frame, clip));
        if clip.w <= 0.0 || clip.h <= 0.0 {
            return;
        }
        if let Some(frame) = node.frame {
            let color = if node.states.contains(States::PASSWORD) {
                PASSWORD_COLOR
            } else if matches!(
                node.role,
                Role::TextField | Role::TextArea | Role::SearchField
            ) {
                FIELD_COLOR
            } else if node.states.contains(States::MODAL) {
                [255, 250, 220, 255]
            } else {
                [245, 245, 245, 255]
            };
            let x0 = clip.x.floor() as i64;
            let y0 = clip.y.floor() as i64;
            let x1 = (clip.x + clip.w).ceil() as i64;
            let y1 = (clip.y + clip.h).ceil() as i64;
            for y in y0..y1 {
                for x in x0..x1 {
                    let border = x == frame.x.floor() as i64
                        || y == frame.y.floor() as i64
                        || x == (frame.x + frame.w).ceil() as i64 - 1
                        || y == (frame.y + frame.h).ceil() as i64 - 1;
                    self.pixel(x, y, if border { [50, 70, 90, 255] } else { color });
                }
            }
            if !node.states.contains(States::PASSWORD) {
                let value = match &node.value {
                    Some(NodeValue::Text(value)) => value.clone(),
                    Some(NodeValue::Number(value) | NodeValue::Range { value, .. }) => {
                        value.to_string()
                    }
                    Some(NodeValue::Bool(value)) => value.to_string(),
                    None => String::new(),
                };
                let label = format!("{} {}", node.name.as_deref().unwrap_or(""), value);
                self.text(&label, frame.x as i64 + 3, frame.y as i64 + 3, clip);
            }
        }
        for child in &node.children {
            self.node(child, clip);
        }
    }
    fn text(&mut self, text: &str, mut x: i64, y: i64, clip: Rect) {
        for ch in text.chars() {
            if x as f64 + 5.0 > clip.x + clip.w {
                break;
            }
            let glyph = glyph(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    let px = x + col;
                    let py = y + row as i64;
                    if bits & (1 << (4 - col)) != 0
                        && px as f64 >= clip.x
                        && py as f64 >= clip.y
                        && (py as f64) < clip.y + clip.h
                    {
                        self.pixel(px, py, [20, 30, 40, 255]);
                    }
                }
            }
            x += 6;
        }
    }
}
fn intersect(a: Rect, b: Rect) -> Rect {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Rect {
        x,
        y,
        w: ((a.x + a.w).min(b.x + b.w) - x).max(0.0),
        h: ((a.y + a.h).min(b.y + b.h) - y).max(0.0),
    }
}
// Original five-by-seven bitmap glyphs. Unsupported Unicode uses a visible box;
// the full Unicode accessible name remains byte-exact in the raw tree.
fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 25, 21, 19, 19, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 27, 17],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ' ' => [0; 7],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 6, 6],
        _ => [31, 17, 17, 17, 17, 17, 31],
    }
}
