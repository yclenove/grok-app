//! Immutable Host-selected file content; never deserialized from model JSON.

use std::fmt;
use std::sync::Arc;

pub const MAX_UPLOAD_BYTES: usize = 8 * 1024 * 1024;

/// A copy of exactly the file selected by the Host. An adapter never reopens
/// its original path. Construction is not file consent or task authority.
#[derive(Clone)]
pub struct HostSelectedUploadFile {
    filename: String,
    mime: String,
    bytes: Arc<[u8]>,
}

impl fmt::Debug for HostSelectedUploadFile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostSelectedUploadFile")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl HostSelectedUploadFile {
    /// Read once from the original Host-opened handle, never reopen its path.
    /// The caller owns that handle until this bounded read physically returns.
    pub fn from_host_reader(
        filename: String,
        mime: String,
        reader: impl std::io::Read,
    ) -> Result<Self, String> {
        use std::io::Read;
        let mut selected = Self::from_host_selection(filename, mime, Vec::new())?;
        let mut bytes = Vec::new();
        reader
            .take((MAX_UPLOAD_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "Host-selected upload could not be read")?;
        if bytes.len() > MAX_UPLOAD_BYTES {
            return Err("Host-selected upload exceeds 8 MiB".into());
        }
        selected.bytes = bytes.into();
        Ok(selected)
    }

    pub fn from_host_selection(
        filename: String,
        mime: String,
        bytes: Vec<u8>,
    ) -> Result<Self, String> {
        let sanitized = crate::staging::sanitize_filename(&filename)
            .map_err(|_| "invalid Host-selected upload filename")?;
        if sanitized != filename
            || filename.chars().any(char::is_control)
            || bytes.len() > MAX_UPLOAD_BYTES
        {
            return Err("invalid Host-selected upload file".into());
        }
        let mime_part = |part: &str| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte))
        };
        if mime.len() > 127
            || !mime
                .split_once('/')
                .is_some_and(|(kind, subtype)| mime_part(kind) && mime_part(subtype))
        {
            return Err("invalid Host-selected upload MIME type".into());
        }
        Ok(Self {
            filename,
            mime: mime.to_ascii_lowercase(),
            bytes: bytes.into(),
        })
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn mime(&self) -> &str {
        &self.mime
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct UploadRequest {
    pub run_id: String,
    pub target_id: String,
    pub target_generation: u64,
    pub action_id: String,
    pub snapshot_id: String,
    pub element_ref: String,
    pub file: HostSelectedUploadFile,
    pub cancellation: crate::execution::ActionCancellation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    NotDispatched(String),
    DispatchedUnknown(String),
}

/// Assignment has completed; publish it only under original native document
/// and Broker task authority. A failed commit must not replay the assignment.
pub trait StagedUpload: Send {
    fn result(&self) -> &super::AdapterActResult;
    fn commit(
        self: Box<Self>,
        commit: &mut dyn FnMut() -> Result<(), crate::error::BrokerError>,
    ) -> Result<(), crate::error::BrokerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_reader_is_bounded_and_never_leaks_io_diagnostics() {
        for length in [0, 11, MAX_UPLOAD_BYTES, MAX_UPLOAD_BYTES + 1] {
            let file = HostSelectedUploadFile::from_host_reader(
                "file.bin".into(),
                "application/octet-stream".into(),
                std::io::Cursor::new(vec![7; length]),
            );
            if length > MAX_UPLOAD_BYTES {
                assert!(file.is_err());
            } else {
                assert_eq!(file.unwrap().bytes(), vec![7; length]);
            }
        }
        struct Failed;
        impl std::io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("private/path/contents"))
            }
        }
        let error = HostSelectedUploadFile::from_host_reader(
            "file.bin".into(),
            "application/octet-stream".into(),
            Failed,
        )
        .unwrap_err();
        assert!(!error.contains("private"));
    }

    #[test]
    fn original_open_file_survives_later_path_replacement() {
        use std::io::Write;
        let directory =
            std::env::temp_dir().join(format!("cu-upload-handle-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("original.bin");
        let retired_path = directory.join("retired.bin");
        let mut initial = std::fs::File::create(&path).unwrap();
        initial.write_all(b"original immutable bytes").unwrap();
        drop(initial);
        let original = std::fs::File::open(&path).unwrap();
        std::fs::rename(&path, &retired_path).unwrap();
        std::fs::write(&path, b"replacement bytes").unwrap();
        let selected = HostSelectedUploadFile::from_host_reader(
            "original.bin".into(),
            "application/octet-stream".into(),
            original,
        )
        .unwrap();
        assert_eq!(selected.bytes(), b"original immutable bytes");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(retired_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn upload_content_is_bounded_immutable_and_debug_does_not_expose_it() {
        let file = HostSelectedUploadFile::from_host_selection(
            "示例.txt".into(),
            "Text/Plain".into(),
            b"private contents".to_vec(),
        )
        .unwrap();
        assert_eq!(file.mime(), "text/plain");
        assert_eq!(file.bytes(), b"private contents");
        let cloned = file.clone();
        assert!(Arc::ptr_eq(&file.bytes, &cloned.bytes));
        let debug = format!("{file:?}");
        assert!(!debug.contains("private") && !debug.contains("示例"));
        assert!(HostSelectedUploadFile::from_host_selection(
            "empty.txt".into(),
            "text/plain".into(),
            vec![],
        )
        .is_ok());
        assert!(HostSelectedUploadFile::from_host_selection(
            "full.bin".into(),
            "application/octet-stream".into(),
            vec![0; MAX_UPLOAD_BYTES],
        )
        .is_ok());
        assert!(HostSelectedUploadFile::from_host_selection(
            "large.bin".into(),
            "application/octet-stream".into(),
            vec![0; MAX_UPLOAD_BYTES + 1],
        )
        .is_err());
    }

    #[test]
    fn upload_name_and_mime_cannot_supply_paths_headers_or_silent_renames() {
        for filename in [
            "../file",
            "C:\\file",
            "/file",
            "file\n.txt",
            " file.txt",
            "CON",
            "a..b",
        ] {
            assert!(
                HostSelectedUploadFile::from_host_selection(
                    filename.into(),
                    "text/plain".into(),
                    vec![1],
                )
                .is_err(),
                "{filename:?}"
            );
        }
        assert!(HostSelectedUploadFile::from_host_selection(
            format!("{}.txt", "a".repeat(81)),
            "text/plain".into(),
            vec![1],
        )
        .is_err());
        for mime in [
            "",
            "text",
            "/plain",
            "text/",
            "text/plain; charset=utf8",
            "text/plain\r\nX: secret",
            "text/plain/other",
        ] {
            assert!(
                HostSelectedUploadFile::from_host_selection(
                    "file.txt".into(),
                    mime.into(),
                    vec![1],
                )
                .is_err(),
                "{mime:?}"
            );
        }
    }
}
