//! codex-cli 0.160 resume branches. Rewinding an earlier turn writes a new
//! paginated rollout (`rollout-<time>-<thread>_<branch>.jsonl`) whose
//! session_meta repeats the parent thread id and points at the parent prefix:
//! `history_base {thread_id, end_ordinal_exclusive, end_byte_offset}`. The
//! branch is its own session: parent prefix records (read once, bounded and
//! verified) followed by the branch records. Nothing is spliced when the
//! parent cannot be proven; the branch alone is then imported as partial.

use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use std::rc::Rc;

use serde_json::Value;

use super::{normalize_file, normalize_sourced};
use crate::session_migration::errors::{ErrorCode, ImportError};
use crate::session_migration::jsonl::{
    open_jsonl_prefix, open_source_jsonl, JsonlPrefix, JsonlRecord, JsonlStream,
};
use crate::session_migration::types::{
    AdapterOutput, ArtifactEvidence, ImportIssue, ImportOutcome,
};

const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 200_000;

pub(crate) struct HistoryBase {
    pub thread_id: String,
    pub end_ordinal: u32,
    pub end_byte: u64,
}

/// `payload` is the session_meta payload.
pub(crate) fn history_base(payload: &Value) -> Option<HistoryBase> {
    let base = payload.get("history_base")?.as_object()?;
    let thread_id = base.get("thread_id")?.as_str()?;
    let end_ordinal = u32::try_from(base.get("end_ordinal_exclusive")?.as_u64()?).ok()?;
    let end_byte = base.get("end_byte_offset")?.as_u64()?;
    (valid_id(thread_id) && end_ordinal > 0 && end_byte > 0).then(|| HistoryBase {
        thread_id: thread_id.into(),
        end_ordinal,
        end_byte,
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Identity of a resume branch, shared by discovery and normalization so the
/// branch never reuses (and later replaces) the parent session's identity.
pub(crate) fn branch_identity(file_stem: &str, payload: &Value) -> Option<String> {
    let base = history_base(payload)?;
    let suffix = file_stem
        .rsplit_once(&format!("{}_", base.thread_id))
        .map(|(_, suffix)| suffix)
        .filter(|suffix| valid_id(suffix) && suffix.len() <= 64);
    Some(match suffix {
        Some(suffix) => format!("{}_{suffix}", base.thread_id),
        None => format!("{}@{}", base.thread_id, base.end_ordinal),
    })
}

/// Reads the first record of `path` only.
pub(crate) fn history_base_of(path: &Path) -> Option<HistoryBase> {
    let first = crate::session_migration::jsonl::open_jsonl(path)
        .ok()?
        .next()?
        .ok()?;
    (first.value.get("type")?.as_str()? == "session_meta")
        .then(|| history_base(first.value.get("payload")?))
        .flatten()
}

pub(crate) enum ParentError {
    Missing,
    Ambiguous,
    Mismatch,
}

impl ParentError {
    pub fn code(&self) -> &'static str {
        match self {
            ParentError::Missing => "codex-history-base-missing",
            ParentError::Ambiguous => "codex-history-base-ambiguous",
            ParentError::Mismatch => "codex-history-base-mismatch",
        }
    }
}

/// Finds the one parent rollout inside the registered root; symlinks are not
/// followed and the walk is bounded.
pub(crate) fn find_parent(root: &Path, thread_id: &str) -> Result<PathBuf, ParentError> {
    let suffix = format!("-{thread_id}.jsonl");
    let mut found = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return Err(ParentError::Missing);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() && depth < MAX_DEPTH {
                stack.push((path, depth + 1));
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with("rollout-") && name.ends_with(&suffix) {
                    found.push(path);
                }
            }
        }
    }
    match found.len() {
        0 => Err(ParentError::Missing),
        1 => Ok(found.pop().unwrap()),
        _ => Err(ParentError::Ambiguous),
    }
}

pub(crate) type PrefixStream = JsonlStream<BufReader<std::io::Take<File>>>;

/// Opens exactly the declared prefix of the parent.
pub(crate) fn open_prefix(parent: &Path, base: &HistoryBase) -> Result<PrefixStream, ParentError> {
    open_jsonl_prefix(parent, base.end_byte).map_err(|_| ParentError::Mismatch)
}

/// First pass: the prefix must be exactly `end_ordinal` complete records that
/// end at `end_byte` and start with the parent's own session_meta.
pub(crate) fn verify_prefix(parent: &Path, base: &HistoryBase) -> Result<JsonlPrefix, ParentError> {
    let mut stream = open_prefix(parent, base)?;
    let mut count = 0u32;
    for record in stream.by_ref() {
        let record: JsonlRecord = record.map_err(|_| ParentError::Mismatch)?;
        if count == 0 {
            let meta = &record.value;
            let own_id = meta
                .get("payload")
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str);
            if meta.get("type").and_then(Value::as_str) != Some("session_meta")
                || own_id != Some(base.thread_id.as_str())
            {
                return Err(ParentError::Mismatch);
            }
        }
        if record.index != count {
            return Err(ParentError::Mismatch);
        }
        count += 1;
    }
    let summary = stream.finish().map_err(|_| ParentError::Mismatch)?;
    if count != base.end_ordinal
        || summary.byte_length != base.end_byte
        || summary.trailing_incomplete
        || !summary.issues.is_empty()
    {
        return Err(ParentError::Mismatch);
    }
    Ok(summary)
}

/// A resume branch is the verified parent prefix followed by its own records.
pub fn normalize_file_in_root(
    path: &Path,
    root: Option<&Path>,
) -> Result<AdapterOutput, ImportError> {
    let Some(base) = history_base_of(path) else {
        return normalize_file(path);
    };
    let verified = root
        .ok_or(ParentError::Missing)
        .and_then(|root| find_parent(root, &base.thread_id))
        .and_then(|parent| verify_prefix(&parent, &base).map(|summary| (parent, summary)));
    let (parent, verified) = match verified {
        Ok(found) => found,
        Err(error) => {
            let mut out = normalize_file(path)?;
            out.issues.push(
                ImportIssue::new(
                    error.code(),
                    "Codex resume branch is imported without an unproven parent prefix",
                )
                .with_span("record:0"),
            );
            out.outcome = ImportOutcome::Partial;
            return Ok(out);
        }
    };
    let changed = || ImportError::new(ErrorCode::SourceChanged, "Codex history base changed");
    let mut parent_stream = open_prefix(&parent, &base).map_err(|_| changed())?;
    let mut branch_stream = open_source_jsonl(path)?;
    let parent_locator: Rc<str> = parent.to_string_lossy().into();
    let branch_locator: Rc<str> = path.to_string_lossy().into();
    let records =
        parent_stream
            .by_ref()
            .map(|record| record.map(|record| (parent_locator.clone(), 0, record)))
            .chain(branch_stream.by_ref().map(|record| {
                record.map(|record| (branch_locator.clone(), base.end_ordinal, record))
            }));
    let mut out = normalize_sourced(records, path, &JsonlPrefix::default())?;
    let parent_summary = parent_stream.finish()?;
    if parent_summary.complete_prefix_sha256 != verified.complete_prefix_sha256 {
        return Err(changed());
    }
    let prefix = branch_stream.finish()?;
    let artifact = &mut out.evidence.artifacts[0];
    artifact.byte_length = prefix.byte_length;
    artifact.sha256 = prefix.file_sha256;
    artifact.complete_prefix_sha256 = Some(prefix.complete_prefix_sha256);
    out.evidence.artifacts.push(ArtifactEvidence {
        artifact_id: parent_locator.to_string(),
        kind: "jsonl-history-base".into(),
        relative_locator: parent_locator.to_string(),
        byte_length: parent_summary.byte_length,
        sha256: parent_summary.file_sha256,
        complete_prefix_sha256: Some(parent_summary.complete_prefix_sha256),
    });
    out.issues.extend(prefix.issues);
    out.validate_graph();
    Ok(out)
}
