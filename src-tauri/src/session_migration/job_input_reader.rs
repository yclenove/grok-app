//! Stored input bounds before an owned body fetch; ordinary v2 keeps its capacity.
use std::io::{self, BufReader, Read};

use rusqlite::{params, Connection, DatabaseName};

use super::{failure, input_wire};
use crate::session_migration::errors::{ErrorCode, ImportError};

const ROW_ID_SQL: &str = "SELECT rowid FROM job_items WHERE job_id=?1 AND candidate_id=?2";

fn row_identity(conn: &Connection, job_id: &str, candidate_id: &str) -> Result<i64, ImportError> {
    conn.query_row(ROW_ID_SQL, params![job_id, candidate_id], |row| row.get(0))
        .map_err(failure)
}

fn byte_reader(
    conn: &Connection,
    row_id: i64,
) -> Result<BufReader<rusqlite::blob::Blob<'_>>, ImportError> {
    let blob = conn
        .blob_open(DatabaseName::Main, "job_items", "result_json", row_id, true)
        .map_err(failure)?;
    Ok(BufReader::with_capacity(64 * 1024, blob))
}

pub(super) fn load(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<String, ImportError> {
    load_with_limit(conn, job_id, candidate_id, None)
}

/// Accepted-input metadata always had a 16 MiB cap, including ordinary v2.
/// Keep that policy distinct from the worker's uncapped ordinary input reader.
pub(super) fn load_registration(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<String, ImportError> {
    load_with_limit(
        conn,
        job_id,
        candidate_id,
        Some(input_wire::ASSOCIATION_BYTES),
    )
}

fn load_with_limit(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
    hard_limit: Option<usize>,
) -> Result<String, ImportError> {
    // Header scan and eventual body fetch must see the exact same row bytes.
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(failure)?;
        return load_with_limit(&tx, job_id, candidate_id, hard_limit);
    }
    let row_id = row_identity(conn, job_id, candidate_id)?;
    let reader = byte_reader(conn, row_id)?;
    // BLOB metadata reports actual byte length without evaluating a SQL CAST
    // that could materialize the cell in SQLite before the Rust-side bound.
    let bytes = reader.get_ref().len();
    if hard_limit.is_some_and(|limit| bytes > limit) {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "registered input exceeds record budget",
        ));
    }
    if bytes > input_wire::ASSOCIATION_BYTES {
        // SQLite's incremental API reads TEXT as bytes too. Never load the full
        // SQL cell just to classify it, and never acquire a writable BLOB handle.
        input_wire::inspect_reader(reader, bytes)?;
    } else {
        drop(reader);
    }
    conn.query_row(
        "SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2 AND rowid=?3",
        params![job_id, candidate_id, row_id],
        |row| row.get(0),
    )
    .map_err(failure)
}

/// Retention is not authorization. Project only the path; an invalid reference
/// refuses cleanup and cannot turn a failed input into permission to delete.
pub(super) fn retained_preview_path(
    conn: &Connection,
    job_id: &str,
    candidate_id: &str,
) -> Result<std::path::PathBuf, ImportError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction().map_err(failure)?;
        return retained_preview_path(&tx, job_id, candidate_id);
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreviewReference {
        preview_path: std::path::PathBuf,
    }
    let row_id = row_identity(conn, job_id, candidate_id)?;
    let mut reader = HeaderReader::new(byte_reader(conn, row_id)?);
    reader.limit_preview_path = true;
    let reference: PreviewReference = serde_json::from_reader(reader)
        .map_err(|_| failure("invalid preview retention reference"))?;
    if !reference.preview_path.is_absolute() {
        return Err(failure("invalid preview retention path"));
    }
    Ok(reference.preview_path)
}

/// IgnoredAny skips bodies without owning their strings. serde_json's streaming
/// parser can still allocate scratch for root field names and wrong-type version
/// scalars; cap those tokens, not legitimate body strings. All supported field
/// names fit even fully JSON-escaped in 256 bytes, and u32 versions fit in 64.
/// Depth matches the full codec's default recursion bound. This is a memory
/// guard around the same serde header parser, not an alternative JSON grammar.
pub(in crate::session_migration) struct HeaderReader<R> {
    inner: R,
    started: bool,
    depth: u16,
    in_string: bool,
    escaped: bool,
    expect_key: bool,
    in_key: bool,
    key: [u8; 256],
    key_len: usize,
    limit_preview_path: bool,
    job_header: bool,
    scalar_limit: Option<usize>,
    scalar_bytes: usize,
}

impl<R> HeaderReader<R> {
    pub(in crate::session_migration) fn new(inner: R) -> Self {
        Self {
            inner,
            started: false,
            depth: 0,
            in_string: false,
            escaped: false,
            expect_key: false,
            in_key: false,
            key: [0; 256],
            key_len: 0,
            limit_preview_path: false,
            job_header: false,
            scalar_limit: None,
            scalar_bytes: 0,
        }
    }

    pub(in crate::session_migration) fn for_job(inner: R) -> Self {
        let mut reader = Self::new(inner);
        reader.job_header = true;
        reader
    }

    fn invalid(message: &'static str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, message)
    }

    fn key_byte(&mut self, byte: u8) -> io::Result<()> {
        if self.key_len == self.key.len() {
            return Err(Self::invalid(
                "durable input field name exceeds codec bounds",
            ));
        }
        self.key[self.key_len] = byte;
        self.key_len += 1;
        Ok(())
    }

    fn observe(&mut self, byte: u8) -> io::Result<()> {
        if !self.started && !byte.is_ascii_whitespace() {
            if byte != b'{' {
                return Err(Self::invalid("durable input must be a JSON object"));
            }
            self.started = true;
        }
        if self.depth == 1
            && self.scalar_limit.is_some()
            && !self.expect_key
            && (self.in_string || !byte.is_ascii_whitespace())
        {
            self.scalar_bytes += 1;
            if self.scalar_bytes > self.scalar_limit.unwrap_or(0) {
                return Err(Self::invalid("durable input scalar exceeds codec bounds"));
            }
        }
        if self.in_string {
            if self.in_key {
                self.key_byte(byte)?;
            }
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
                if self.in_key {
                    let key: String = serde_json::from_slice(&self.key[..self.key_len])
                        .map_err(|_| Self::invalid("durable input field name is invalid"))?;
                    self.scalar_limit = match key.as_str() {
                        "version" => Some(64),
                        // Include both quotes and the next root separator.
                        "jobId" | "planId" if self.job_header => Some(36 * 6 + 3),
                        "planHash" if self.job_header => Some(64 * 6 + 3),
                        "stateVersion" | "itemCount" if self.job_header => Some(64),
                        "state" if self.job_header => Some(128),
                        "createdAt" if self.job_header => Some(1024),
                        // No new small request limit: the original complete
                        // association job record was bounded at 16 MiB.
                        "requestId" if self.job_header => Some(input_wire::ASSOCIATION_BYTES),
                        // Above supported native filesystem path lengths even
                        // when every UTF-16 unit is JSON-escaped. Bound scratch
                        // before a corrupt path can allocate the full body.
                        "previewPath" if self.limit_preview_path => Some(128 * 1024 * 6),
                        _ => None,
                    };
                    self.scalar_bytes = 0;
                }
                self.in_key = false;
            }
            return Ok(());
        }
        match byte {
            b'"' => {
                self.in_string = true;
                self.in_key = self.depth == 1 && self.expect_key;
                if self.in_key {
                    self.key_len = 0;
                    self.key_byte(byte)?;
                }
            }
            b'{' | b'[' => {
                self.depth += 1;
                if self.depth > 128 {
                    return Err(Self::invalid("durable input nesting exceeds codec bounds"));
                }
                if self.depth == 1 {
                    self.expect_key = true;
                }
            }
            b'}' | b']' => {
                self.depth = self
                    .depth
                    .checked_sub(1)
                    .ok_or_else(|| Self::invalid("durable input nesting is invalid"))?;
            }
            b':' if self.depth == 1 => self.expect_key = false,
            b',' if self.depth == 1 => {
                self.expect_key = true;
                self.scalar_limit = None;
                self.scalar_bytes = 0;
            }
            _ => {}
        }
        Ok(())
    }
}

impl<R: Read> Read for HeaderReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        for &byte in &buffer[..count] {
            self.observe(byte)?;
        }
        Ok(count)
    }
}

#[cfg(test)]
#[path = "job_input_reader_tests.rs"]
mod legacy_identity_tests;
