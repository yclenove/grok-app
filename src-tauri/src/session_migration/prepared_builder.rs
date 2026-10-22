use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use uuid::Uuid;

use super::errors::{io_storage_error, ErrorCode, ImportError};
use super::hash::{normalized_digest_from_content, ContentDigestBuilder};
use super::jobs::{prepared_hashes, PreparedRevision};
use super::types::{AdapterOutput, ImportedEvent};

const EVENT_COUNT_MAX: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEventHeader {
    pub event_id: String,
    pub ordinal: u32,
    pub model_visible: bool,
}

#[derive(Debug)]
pub struct PreparedRevisionOutput {
    pub prepared: Arc<PreparedRevision>,
    pub event_headers: Vec<PreparedEventHeader>,
}

/// Owns one private preview directory until every immutable revision file is
/// complete. Any error or unwind removes the entire directory.
pub struct PreparedRevisionBuilder {
    parent: PathBuf,
    path: PathBuf,
    events: Option<BufWriter<File>>,
    content_digest: Option<ContentDigestBuilder>,
    event_headers: Vec<PreparedEventHeader>,
    armed: bool,
    _operation: super::preview_lifecycle::OperationGuard,
}

impl PreparedRevisionBuilder {
    pub fn new() -> Result<Self, ImportError> {
        Self::new_in_preview_dir(&Uuid::new_v4().to_string())
    }

    pub(crate) fn new_in_preview_dir(directory_id: &str) -> Result<Self, ImportError> {
        let operation = super::preview_lifecycle::operation()?;
        let parsed = Uuid::parse_str(directory_id).map_err(|_| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "preview directory identity is not a UUID",
            )
        })?;
        if parsed.to_string() != directory_id {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "preview directory identity is not canonical",
            ));
        }
        let parent = super::catalog::staging_dir().join("previews");
        fs::create_dir_all(&parent).map_err(io_storage_error)?;
        let path = parent.join(directory_id);
        fs::create_dir(&path).map_err(io_storage_error)?;
        let events = match File::create(path.join("events.jsonl")) {
            Ok(file) => BufWriter::new(file),
            Err(error) => {
                let _ = fs::remove_dir_all(&path);
                return Err(io_storage_error(error));
            }
        };
        Ok(Self {
            parent,
            path,
            events: Some(events),
            content_digest: Some(ContentDigestBuilder::new()),
            event_headers: Vec::new(),
            armed: true,
            _operation: operation,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn blobs_path(&self) -> PathBuf {
        self.path.join("blobs")
    }

    pub fn push_event(&mut self, event: &ImportedEvent) -> Result<(), ImportError> {
        if self.event_headers.len() >= EVENT_COUNT_MAX {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "prepared revision exceeded event budget",
            ));
        }
        let bytes = serde_json::to_vec(event).map_err(snapshot_error)?;
        if bytes.len() > super::budget::JSON_RECORD_MAX_BYTES {
            return Err(ImportError::new(
                ErrorCode::ResourceLimit,
                "normalized event exceeds 32 MiB archive record limit",
            ));
        }
        let writer = self.events.as_mut().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "prepared revision writer is already finalized",
            )
        })?;
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Write).map_err(io_storage_error)?;
        writer.write_all(&bytes).map_err(io_storage_error)?;
        writer.write_all(b"\n").map_err(io_storage_error)?;
        self.content_digest
            .as_mut()
            .ok_or_else(|| {
                ImportError::new(
                    ErrorCode::SnapshotUnavailable,
                    "prepared revision digest is already finalized",
                )
            })?
            .push(event)
            .map_err(snapshot_error)?;
        self.event_headers.push(PreparedEventHeader {
            event_id: event.id().to_string(),
            ordinal: event.base().ordinal,
            model_visible: !super::selection::is_never_model_visible(event),
        });
        Ok(())
    }

    pub fn finalize(
        mut self,
        adapter: &AdapterOutput,
        options: &serde_json::Value,
    ) -> Result<PreparedRevisionOutput, ImportError> {
        self.ensure_unique_event_ids()?;
        let mut writer = self.events.take().ok_or_else(|| {
            ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "prepared revision writer is already finalized",
            )
        })?;
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Flush).map_err(io_storage_error)?;
        writer.flush().map_err(io_storage_error)?;
        #[cfg(test)]
        io_tests::maybe_fail(io_tests::Point::Sync).map_err(io_storage_error)?;
        writer.get_ref().sync_all().map_err(io_storage_error)?;
        drop(writer);

        let content_digest = self
            .content_digest
            .take()
            .expect("prepared digest exists")
            .finish(options);
        let digest = normalized_digest_from_content(adapter, &content_digest);
        super::jobs::write_revision_metadata_files(&self.path, adapter, &digest)?;
        super::search::rebuild_index_streaming(&self.path, &|| false)?;
        let file_hashes = prepared_hashes(&self.path)?;
        let prepared = PreparedRevision::from_owned(
            self.path.clone(),
            digest,
            file_hashes,
            self.parent.clone(),
        );
        let event_headers = std::mem::take(&mut self.event_headers);
        self.armed = false;
        Ok(PreparedRevisionOutput {
            prepared,
            event_headers,
        })
    }

    fn ensure_unique_event_ids(&self) -> Result<(), ImportError> {
        // Keep source order intact and avoid cloning attacker-controlled IDs.
        // EVENT_COUNT_MAX bounds this temporary index to at most 8 MiB on x64.
        let mut order: Vec<usize> = (0..self.event_headers.len()).collect();
        order.sort_unstable_by(|left, right| {
            self.event_headers[*left]
                .event_id
                .cmp(&self.event_headers[*right].event_id)
        });
        if order.windows(2).any(|pair| {
            self.event_headers[pair[0]].event_id == self.event_headers[pair[1]].event_id
        }) {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "prepared revision contains duplicate event identity",
            ));
        }
        Ok(())
    }
}

impl Drop for PreparedRevisionBuilder {
    fn drop(&mut self) {
        if self.armed && self.path.parent() == Some(self.parent.as_path()) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn snapshot_error(error: impl std::fmt::Display) -> ImportError {
    ImportError::new(ErrorCode::SnapshotUnavailable, error.to_string())
}

#[cfg(test)]
#[path = "prepared_builder_io_tests.rs"]
mod io_tests;
