//! Bounded JSONL records and a verified fixed byte prefix.
use super::budget::{JSON_NEST_MAX, JSON_RECORD_MAX_BYTES, SESSION_BODY_MAX_BYTES};
use super::errors::{ErrorCode, ImportError};
use super::types::ImportIssue;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::File;
#[cfg(test)]
use std::io::Cursor;
use std::io::{BufRead, BufReader, Read, Take};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct JsonlRecord {
    /// Physical index: damaged records do not renumber later events.
    pub index: u32,
    pub byte_start: u64,
    pub byte_end: u64,
    pub value: Value,
}
#[derive(Debug, Clone, Default)]
pub struct JsonlPrefix {
    #[cfg(test)]
    pub records: Vec<JsonlRecord>,
    pub complete_prefix_sha256: String,
    pub file_sha256: String,
    pub byte_length: u64,
    pub trailing_incomplete: bool,
    pub issues: Vec<ImportIssue>,
}
pub struct JsonlStream<R> {
    reader: R,
    source: Option<PathBuf>,
    source_len: u64,
    record: u32,
    readable: bool,
    /// Vendor sources only: a stream with no readable record is a diagnosis,
    /// not an empty history. The app's own archives keep empty = empty.
    require_readable: bool,
    finished: bool,
    raw_hash: Sha256,
    prefix_hash: Sha256,
    summary: JsonlPrefix,
}
impl<R: BufRead> JsonlStream<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            source: None,
            source_len: 0,
            record: 0,
            readable: false,
            require_readable: false,
            finished: false,
            raw_hash: Sha256::new(),
            prefix_hash: Sha256::new(),
            summary: JsonlPrefix::default(),
        }
    }
    /// Only after EOF. Verify the fixed prefix, allowing concurrent append.
    pub fn finish(mut self) -> Result<JsonlPrefix, ImportError> {
        if !self.finished {
            return Err(ImportError::new(
                ErrorCode::SnapshotUnavailable,
                "JSONL snapshot was not fully read",
            ));
        }
        self.summary.file_sha256 = hex::encode(self.raw_hash.finalize());
        self.summary.complete_prefix_sha256 = hex::encode(self.prefix_hash.finalize());
        if let Some(path) = self.source {
            let file = File::open(path).map_err(io_error)?;
            if file.metadata().map_err(io_error)?.len() < self.source_len {
                return Err(ImportError::new(
                    ErrorCode::SourceChanged,
                    "JSONL source was truncated",
                ));
            }
            let mut reader = file.take(self.source_len);
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = reader.read(&mut buffer).map_err(io_error)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            if hex::encode(hash.finalize()) != self.summary.file_sha256 {
                return Err(ImportError::new(
                    ErrorCode::SourceChanged,
                    "JSONL source changed during parsing",
                ));
            }
        }
        Ok(self.summary)
    }
    fn next_record(&mut self) -> Result<Option<JsonlRecord>, ImportError> {
        loop {
            let mut bytes = Vec::new();
            // Bound allocation before read_until; CRLF gets two extra bytes.
            let n = (&mut self.reader)
                .take((JSON_RECORD_MAX_BYTES + 3) as u64)
                .read_until(b'\n', &mut bytes)
                .map_err(io_error)?;
            if n == 0 {
                if self.require_readable && !self.readable {
                    return Err(ImportError::new(
                        ErrorCode::FormatUnsupported,
                        self.summary
                            .issues
                            .first()
                            .map_or("empty-jsonl", |issue| issue.code.as_str()),
                    ));
                }
                return Ok(None);
            }
            let start = self.summary.byte_length;
            self.summary.byte_length += n as u64;
            let record = self.record;
            self.record += 1;
            if self.summary.byte_length > SESSION_BODY_MAX_BYTES {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "JSONL body exceeds 1 GiB",
                ));
            }
            self.raw_hash.update(&bytes);
            let has_nl = bytes.ends_with(b"\n");
            let mut line = bytes.as_slice();
            if has_nl {
                line = &line[..line.len() - 1];
            }
            if line.ends_with(b"\r") {
                line = &line[..line.len() - 1];
            }
            if line.len() > JSON_RECORD_MAX_BYTES {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "jsonl-record-too-large",
                ));
            }
            let end = start + line.len() as u64;
            if record == 0 {
                line = line.strip_prefix(b"\xef\xbb\xbf").unwrap_or(line);
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                self.prefix_hash.update(&bytes);
                continue;
            }
            check_depth(line)?;
            let damage = if line
                .iter()
                .any(|&b| b < 0x20 && !matches!(b, b'\t' | b'\r' | b'\n'))
            {
                Some((
                    "binary-record",
                    "JSONL record contains binary control bytes",
                ))
            } else if std::str::from_utf8(line).is_err() {
                Some(("invalid-utf8", "JSONL record is not valid UTF-8"))
            } else {
                None
            };
            if let Some((code, message)) = damage {
                self.prefix_hash.update(&bytes);
                self.summary.issues.push(
                    ImportIssue::new(code, message)
                        .with_span(format!("line:{}", record + 1))
                        .with_error(ErrorCode::PartialHistory),
                );
                continue;
            }
            match serde_json::from_slice::<Value>(line) {
                Ok(value) => {
                    self.readable = true;
                    self.prefix_hash.update(&bytes);
                    return Ok(Some(JsonlRecord {
                        index: record,
                        byte_start: start,
                        byte_end: end,
                        value,
                    }));
                }
                Err(error) => {
                    let incomplete = !has_nl && error.is_eof();
                    self.summary.trailing_incomplete |= incomplete;
                    if !incomplete {
                        self.prefix_hash.update(&bytes);
                    }
                    self.summary.issues.push(ImportIssue::new(
                        if incomplete { "trailing-incomplete" } else { "broken-record" },
                        if incomplete { "source ends inside a JSON record; only the complete prefix is fixed" }
                        else { "JSONL record is not valid JSON" },
                    ).with_span(format!("line:{}", record + 1)).with_error(ErrorCode::PartialHistory));
                }
            }
        }
    }
}
impl<R: BufRead> Iterator for JsonlStream<R> {
    type Item = Result<JsonlRecord, ImportError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.next_record() {
            Ok(Some(record)) => Some(Ok(record)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }
}
pub(crate) fn check_depth(bytes: &[u8]) -> Result<(), ImportError> {
    let (mut depth, mut string, mut escaped) = (0usize, false, false);
    for &b in bytes {
        if string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                string = false;
            }
        } else {
            match b {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > JSON_NEST_MAX {
                        return Err(ImportError::new(
                            ErrorCode::ResourceLimit,
                            "JSON nesting exceeds 64 levels",
                        ));
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}
fn io_error(e: std::io::Error) -> ImportError {
    ImportError::new(
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => ErrorCode::SourcePermissionDenied,
            std::io::ErrorKind::NotFound => ErrorCode::SourceNotFound,
            _ => ErrorCode::SnapshotUnavailable,
        },
        e.to_string(),
    )
}
pub fn open_jsonl(path: &Path) -> Result<JsonlStream<BufReader<Take<File>>>, ImportError> {
    let file = File::open(path).map_err(io_error)?;
    let length = file.metadata().map_err(io_error)?.len();
    if length > SESSION_BODY_MAX_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "JSONL body exceeds 1 GiB",
        ));
    }
    let mut stream = JsonlStream::new(BufReader::new(file.take(length)));
    stream.source = Some(path.to_owned());
    stream.source_len = length;
    Ok(stream)
}
/// A vendor source file: like `open_jsonl`, but a file without any readable
/// record fails with its diagnosis instead of reading as an empty history.
pub(crate) fn open_source_jsonl(
    path: &Path,
) -> Result<JsonlStream<BufReader<Take<File>>>, ImportError> {
    let mut stream = open_jsonl(path)?;
    stream.require_readable = true;
    Ok(stream)
}
/// Exactly the first `limit` bytes of `path`; `finish` re-verifies them.
pub(crate) fn open_jsonl_prefix(
    path: &Path,
    limit: u64,
) -> Result<JsonlStream<BufReader<Take<File>>>, ImportError> {
    let file = File::open(path).map_err(io_error)?;
    let length = file.metadata().map_err(io_error)?.len();
    if limit > SESSION_BODY_MAX_BYTES {
        return Err(ImportError::new(
            ErrorCode::ResourceLimit,
            "JSONL body exceeds 1 GiB",
        ));
    }
    if length < limit {
        return Err(ImportError::new(
            ErrorCode::SourceChanged,
            "JSONL source is shorter than its declared prefix",
        ));
    }
    let mut stream = JsonlStream::new(BufReader::new(file.take(limit)));
    stream.source = Some(path.to_owned());
    stream.source_len = limit;
    Ok(stream)
}
#[cfg(test)]
pub fn parse_jsonl_prefix(bytes: &[u8]) -> Result<JsonlPrefix, ImportError> {
    let mut stream = JsonlStream::new(Cursor::new(bytes));
    stream.require_readable = true;
    collect(stream)
}
#[cfg(test)]
fn collect<R: BufRead>(mut stream: JsonlStream<R>) -> Result<JsonlPrefix, ImportError> {
    let records = stream.by_ref().collect::<Result<Vec<_>, _>>()?;
    let mut summary = stream.finish()?;
    summary.records = records;
    Ok(summary)
}
#[cfg(test)]
mod tests {
    use super::super::hash::sha256_bytes;
    use super::*;
    #[test]
    fn keeps_complete_prefix_and_flags_trailing_half_line() {
        let p = parse_jsonl_prefix(b"{\"a\":1}\n{\"b\":").unwrap();
        assert_eq!(p.records.len(), 1);
        assert!(p.trailing_incomplete);
        assert_eq!(p.records[0].value["a"], 1);
        assert_eq!(p.complete_prefix_sha256, sha256_bytes(b"{\"a\":1}\n"));
    }
    #[test]
    fn middle_broken_record_is_not_treated_as_tail() {
        let raw = b"{\"a\":1}\nNOT JSON\n{\"b\":2}\n";
        let p = parse_jsonl_prefix(raw).unwrap();
        assert_eq!(p.records.len(), 2);
        assert!(!p.trailing_incomplete);
        assert_eq!(p.issues[0].code, "broken-record");
        assert_eq!(p.records[1].index, 2);
        assert_eq!(p.records[1].value["b"], 2);
        assert_eq!(p.complete_prefix_sha256, sha256_bytes(raw));
    }
    #[test]
    fn invalid_final_record_is_not_a_pending_write() {
        let p = parse_jsonl_prefix(b"{\"a\":1}\nnot json").unwrap();
        assert!(!p.trailing_incomplete);
        assert_eq!(p.issues[0].code, "broken-record");
    }
    #[test]
    fn damaged_lines_keep_their_byte_evidence_and_physical_record_numbers() {
        for (damage, code) in [
            (b"{\"text\":\"\xff\"}\n".as_slice(), "invalid-utf8"),
            (b"\x7fELF\0\x01\n".as_slice(), "binary-record"),
        ] {
            let raw = [b"{\"a\":1}\n".as_slice(), damage, b"{\"b\":2}\n"].concat();
            let p = parse_jsonl_prefix(&raw).unwrap();
            assert_eq!(p.records.len(), 2);
            assert_eq!(p.records[1].index, 2);
            assert_eq!(p.issues[0].code, code);
            assert_eq!(p.issues[0].span.as_deref(), Some("line:2"));
            assert_eq!(p.file_sha256, sha256_bytes(&raw));
            assert_eq!(p.complete_prefix_sha256, p.file_sha256);
            assert!(!p.trailing_incomplete);
        }
    }
    #[test]
    fn valid_del_character_in_json_text_is_not_binary_damage() {
        let p = parse_jsonl_prefix(b"{\"text\":\"\x7f\"}\n").unwrap();
        assert!(p.issues.is_empty());
        assert_eq!(p.records[0].value["text"].as_str(), Some("\u{7f}"));
    }
    #[test]
    fn bom_crlf_and_blank_records_keep_byte_provenance() {
        let raw = b"\xef\xbb\xbf{\"a\":1}\r\n  \r\n{\"b\":2}";
        let p = parse_jsonl_prefix(raw).unwrap();
        assert_eq!(p.records.len(), 2);
        assert_eq!(p.records[1].index, 2);
        assert_eq!(p.file_sha256, sha256_bytes(raw));
        assert_eq!(p.complete_prefix_sha256, p.file_sha256);
    }
    #[test]
    fn depth_limit_counts_containers_but_not_quoted_brackets() {
        let nested = format!("{}0{}", "[".repeat(65), "]".repeat(65));
        assert_eq!(
            parse_jsonl_prefix(nested.as_bytes()).unwrap_err().code,
            ErrorCode::ResourceLimit
        );
        assert!(parse_jsonl_prefix(serde_json::to_string(&nested).unwrap().as_bytes()).is_ok());
        for &damage in b"\xff\0" {
            let mut raw = nested.as_bytes().to_vec();
            raw.push(damage);
            assert_eq!(
                parse_jsonl_prefix(&raw).unwrap_err().code,
                ErrorCode::ResourceLimit
            );
        }
    }
    #[test]
    fn active_file_append_is_allowed_but_rewrite_is_rejected() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("grok-jsonl-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"{\"a\":1}\n").unwrap();
        let mut stream = open_jsonl(&path).unwrap();
        assert_eq!(stream.next().unwrap().unwrap().value["a"], 1);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"b\":2}\n")
            .unwrap();
        assert!(stream.next().is_none());
        assert_eq!(stream.finish().unwrap().byte_length, 8);
        let mut changed = open_jsonl(&path).unwrap();
        for record in changed.by_ref() {
            record.unwrap();
        }
        std::fs::write(&path, b"{\"a\":3}\n{\"b\":2}\n").unwrap();
        assert_eq!(changed.finish().unwrap_err().code, ErrorCode::SourceChanged);
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":3}\n{\"b\":2}\n");
        let mut truncated = open_jsonl(&path).unwrap();
        for record in truncated.by_ref() {
            record.unwrap();
        }
        std::fs::write(&path, b"{\"a\":3}\n").unwrap();
        assert_eq!(
            truncated.finish().unwrap_err().code,
            ErrorCode::SourceChanged
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":3}\n");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn only_vendor_sources_treat_no_readable_record_as_a_diagnosis() {
        let dir = std::env::temp_dir().join(format!("jsonl-empty-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("events.jsonl");
        std::fs::write(&path, b"").unwrap();
        // The app's own archives: an empty file is an empty history.
        let mut archive = open_jsonl(&path).unwrap();
        assert!(archive.next().is_none());
        assert!(archive.finish().is_ok());
        // A vendor source without any readable record is a diagnosis.
        let mut source = open_source_jsonl(&path).unwrap();
        assert_eq!(
            source.next().unwrap().unwrap_err().code,
            ErrorCode::FormatUnsupported
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
