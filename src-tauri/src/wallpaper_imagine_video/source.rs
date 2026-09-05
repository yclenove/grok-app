//! Bounded source normalization. Never ask the agent to run image converters.

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use image::{ImageFormat, ImageReader, Limits};

const MAX_SOURCE_BYTES: u64 = 40 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;
const MAX_EDGE: u32 = 2048;

pub(super) fn materialize(
    encoded: Option<&str>,
    original: &Path,
    output_dir: &Path,
) -> Result<PathBuf, &'static str> {
    let bytes = if let Some(encoded) = encoded {
        // Bound the text before base64 allocates its output buffer.
        if encoded.is_empty() || encoded.len() > MAX_PNG_BYTES.div_ceil(3) * 4 {
            return Err("imagine_source_invalid");
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "imagine_source_invalid")?;
        if bytes.len() > MAX_PNG_BYTES || image::guess_format(&bytes).ok() != Some(ImageFormat::Png)
        {
            return Err("imagine_source_invalid");
        }
        bytes
    } else {
        let mut file = fs::File::open(original).map_err(|_| "imagine_source_invalid")?;
        if file.metadata().map_err(|_| "imagine_source_invalid")?.len() > MAX_SOURCE_BYTES {
            return Err("imagine_source_invalid");
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_SOURCE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "imagine_source_invalid")?;
        if bytes.len() as u64 > MAX_SOURCE_BYTES {
            return Err("imagine_source_invalid");
        }
        bytes
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let make_reader = || -> Result<_, &'static str> {
        let mut reader = ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|_| "imagine_source_invalid")?;
        reader.limits(limits.clone());
        Ok(reader)
    };
    let (width, height) = make_reader()?
        .into_dimensions()
        .map_err(|_| "imagine_source_invalid")?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 50_000_000 {
        return Err("imagine_source_invalid");
    }
    let mut image = make_reader()?
        .decode()
        .map_err(|_| "imagine_source_invalid")?;
    if width > MAX_EDGE || height > MAX_EDGE {
        image = image.thumbnail(MAX_EDGE, MAX_EDGE);
    }
    // Re-encode even PNG/JPEG: strip metadata and snapshot the selected pixels
    // so later edits to the original cannot change this generation's source.
    let path = output_dir.join(".video-source.png");
    image
        .save_with_format(&path, ImageFormat::Png)
        .map_err(|_| "imagine_failed")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallpaper_imagine_video::tests::temp_dir;

    #[test]
    fn converts_supported_formats_to_valid_bounded_png() {
        let root = temp_dir("source-conversion");
        for format in [
            ImageFormat::Jpeg,
            ImageFormat::Png,
            ImageFormat::WebP,
            ImageFormat::Gif,
        ] {
            let path = root.join(format!("input.{}", format.extensions_str()[0]));
            image::DynamicImage::new_rgb8(2400, 24)
                .save_with_format(&path, format)
                .unwrap();
            let result = materialize(None, &path, &root).unwrap();
            let image = image::open(&result).unwrap();
            assert_eq!(image.width(), MAX_EDGE);
            assert_eq!(image.height(), 20);
            assert!(path.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_fake_png_and_accepts_browser_encoded_pixels() {
        let root = temp_dir("browser-source");
        let original = root.join("input.avif");
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(32, 24)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes.into_inner());
        assert!(materialize(Some(&encoded), &original, &root).is_ok());
        let fake =
            base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nnot an image");
        assert_eq!(
            materialize(Some(&fake), &original, &root),
            Err("imagine_source_invalid")
        );
        assert_eq!(
            materialize(Some("not base64"), &original, &root),
            Err("imagine_source_invalid")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
