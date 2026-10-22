//! Human-only image reading. Scope comes from a verified preview lease or
//! published receipt, never a renderer-supplied path, digest or MIME override.
//! This module does not create or widen model selections/read grants.

use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use image::{ImageFormat, ImageReader, Limits};
use serde::{Deserialize, Serialize};

use super::types::AssetState;
use super::{budget, catalog, hash, jobs, preview_lifecycle, preview_store};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ImageScope {
    Preview {
        #[serde(rename = "previewId")]
        preview_id: String,
    },
    Published {
        #[serde(rename = "importId")]
        import_id: String,
        #[serde(rename = "revisionId")]
        revision_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ImageState {
    Stored,
    Missing,
    Unresolved,
    Excluded,
    Oversize,
    Unsupported,
    Invalid,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResolution {
    pub state: ImageState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    pub mime: &'static str,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug)]
pub(crate) struct VerifiedImage {
    pub bytes: Vec<u8>,
    pub info: ImageInfo,
}

// IPC resolution and HTTP thumbnail/original reads share one memory/decode
// budget, including requests left running after their renderer is closed.
static READERS: OnceLock<(Mutex<usize>, Condvar)> = OnceLock::new();
pub(crate) struct ImagePermit;

pub(crate) fn acquire() -> ImagePermit {
    let (active, available) = READERS.get_or_init(|| (Mutex::new(0), Condvar::new()));
    let mut count = active.lock().unwrap_or_else(|error| error.into_inner());
    while *count >= budget::READ_CONCURRENCY {
        count = available
            .wait(count)
            .unwrap_or_else(|error| error.into_inner());
    }
    *count += 1;
    ImagePermit
}

impl Drop for ImagePermit {
    fn drop(&mut self) {
        if let Some((active, available)) = READERS.get() {
            let mut count = active.lock().unwrap_or_else(|error| error.into_inner());
            *count = count.saturating_sub(1);
            available.notify_one();
        }
    }
}

pub fn resolve(scope: &ImageScope, asset_id: &str) -> ImageResolution {
    let _permit = acquire();
    match read(scope, asset_id) {
        Ok(image) => ImageResolution {
            state: ImageState::Stored,
            image: Some(image.info),
        },
        Err(state) => ImageResolution { state, image: None },
    }
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn snapshot(scope: &ImageScope) -> Result<PathBuf, ImageState> {
    let dir = match scope {
        ImageScope::Preview { preview_id } => {
            if !canonical_uuid(preview_id) {
                return Err(ImageState::Invalid);
            }
            let dir = catalog::staging_dir().join("previews").join(preview_id);
            require_snapshot_paths(&dir)?;
            // Always revalidate disk integrity, even if an in-memory preview exists.
            let verified =
                preview_store::disk_snapshot_path(preview_id).map_err(|_| ImageState::Invalid)?;
            if verified != dir {
                return Err(ImageState::Invalid);
            }
            dir
        }
        ImageScope::Published {
            import_id,
            revision_id,
        } => {
            if !canonical_uuid(revision_id) {
                return Err(ImageState::Invalid);
            }
            let dir =
                catalog::revision_dir(import_id, revision_id).map_err(|_| ImageState::Invalid)?;
            require_snapshot_paths(&dir)?;
            let conn = catalog::open_catalog_readonly().map_err(|_| ImageState::Invalid)?;
            let auth = jobs::load_published_auth_with(&conn, import_id, Some(revision_id), None)
                .map_err(|_| ImageState::Invalid)?;
            if auth.import_id != *import_id || auth.revision_id != *revision_id {
                return Err(ImageState::Invalid);
            }
            dir
        }
    };
    Ok(dir)
}

fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0 || meta.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn regular_path(path: &Path, directory: bool) -> Result<(), ImageState> {
    // Check every existing ancestor, including the app-data/imports root.
    for ancestor in path.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(|_| ImageState::Invalid)?;
        if is_reparse(&meta) || (ancestor != path && !meta.is_dir()) {
            return Err(ImageState::Invalid);
        }
        if ancestor == path
            && if directory {
                !meta.is_dir()
            } else {
                !meta.is_file()
            }
        {
            return Err(ImageState::Invalid);
        }
    }
    catalog::ensure_inside_imports(path).map_err(|_| ImageState::Invalid)
}

fn require_snapshot_paths(dir: &Path) -> Result<(), ImageState> {
    regular_path(dir, true)?;
    // Verification reads these files and blob hashes. Reject junctions as well
    // as symlinks before handing the snapshot to the shared verifier.
    for entry in fs::read_dir(dir).map_err(|_| ImageState::Invalid)? {
        let path = entry.map_err(|_| ImageState::Invalid)?.path();
        let meta = fs::symlink_metadata(&path).map_err(|_| ImageState::Invalid)?;
        if is_reparse(&meta) {
            return Err(ImageState::Invalid);
        }
        if path.file_name().is_some_and(|name| name == "blobs") {
            regular_path(&path, true)?;
            for blob in fs::read_dir(&path).map_err(|_| ImageState::Invalid)? {
                regular_path(&blob.map_err(|_| ImageState::Invalid)?.path(), false)?;
            }
        }
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<File, ImageState> {
    regular_path(path, false)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file: File = options.open(path).map_err(|_| ImageState::Invalid)?;
    let meta = file.metadata().map_err(|_| ImageState::Invalid)?;
    if is_reparse(&meta) || !meta.is_file() {
        return Err(ImageState::Invalid);
    }
    Ok(file)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ImageState> {
    let file = open_regular(path)?;
    let meta = file.metadata().map_err(|_| ImageState::Invalid)?;
    if meta.len() > limit {
        return Err(ImageState::Oversize);
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ImageState::Invalid)?;
    if bytes.len() as u64 > limit {
        return Err(ImageState::Oversize);
    }
    if bytes.len() as u64 != meta.len() {
        return Err(ImageState::Invalid);
    }
    regular_path(path, false)?;
    Ok(bytes)
}

pub(crate) fn read(scope: &ImageScope, asset_id: &str) -> Result<VerifiedImage, ImageState> {
    if asset_id.is_empty() || asset_id.len() > 512 || asset_id.chars().any(char::is_control) {
        return Err(ImageState::Invalid);
    }
    let _operation = preview_lifecycle::operation().map_err(|_| ImageState::Invalid)?;
    let dir = snapshot(scope)?;
    let catalog_file = open_regular(&dir.join("assets.json"))?;
    if catalog_file
        .metadata()
        .map_err(|_| ImageState::Invalid)?
        .len()
        > budget::SESSION_BODY_MAX_BYTES
    {
        return Err(ImageState::Oversize);
    }
    let asset = super::image_catalog::read(
        catalog_file.take(budget::SESSION_BODY_MAX_BYTES + 1),
        asset_id,
    )?;
    match asset.state {
        AssetState::Missing => return Err(ImageState::Missing),
        AssetState::Unresolved => return Err(ImageState::Unresolved),
        AssetState::Excluded => return Err(ImageState::Excluded),
        AssetState::Oversize => return Err(ImageState::Oversize),
        AssetState::Stored => {}
    }
    if asset.size > budget::IMAGE_MAX_BYTES {
        return Err(ImageState::Oversize);
    }
    let digest = asset
        .digest
        .as_deref()
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(ImageState::Invalid)?;
    let bytes = read_bounded(&dir.join("blobs").join(digest), budget::IMAGE_MAX_BYTES)?;
    if bytes.len() as u64 != asset.size || hash::sha256_bytes(&bytes) != digest {
        return Err(ImageState::Invalid);
    }
    let format = image::guess_format(&bytes).map_err(|_| ImageState::Unsupported)?;
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::Gif => "image/gif",
        ImageFormat::WebP => "image/webp",
        ImageFormat::Bmp => "image/bmp",
        _ => return Err(ImageState::Unsupported),
    };
    if asset
        .mime
        .as_deref()
        .is_some_and(|value| value != mime && value != "application/octet-stream")
    {
        return Err(ImageState::Invalid);
    }
    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let (width, height) = reader.into_dimensions().map_err(|_| ImageState::Invalid)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 50_000_000 {
        return Err(ImageState::Oversize);
    }
    let info = ImageInfo {
        mime,
        bytes: bytes.len() as u64,
        width,
        height,
    };
    Ok(VerifiedImage { bytes, info })
}

#[cfg(test)]
#[path = "image_reader_tests.rs"]
mod tests;
