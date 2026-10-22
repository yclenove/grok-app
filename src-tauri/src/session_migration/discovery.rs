//! Bounded metadata reads and incremental traversal. Full contents are fixed at preview.

use serde_json::Value;
use std::fs::{File, ReadDir};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::errors::{ErrorCode, ImportError};
use super::hash::{options_hash, sha256_bytes};
use super::types::{Provider, SourceCandidate};

const HEADER_BYTES: u64 = 64 * 1024;
const MAX_DEPTH: usize = 12;

fn io_error(error: std::io::Error) -> ImportError {
    ImportError::new(
        if error.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::SourceNotFound
        } else {
            ErrorCode::SourcePermissionDenied
        },
        error.to_string(),
    )
}

fn sample(path: &Path) -> Result<(Vec<u8>, Vec<u8>, String), ImportError> {
    let mut file = File::open(path).map_err(io_error)?;
    let size = file.metadata().map_err(io_error)?.len();
    let mut head = Vec::new();
    file.by_ref()
        .take(HEADER_BYTES)
        .read_to_end(&mut head)
        .map_err(io_error)?;
    let mut tail = Vec::new();
    if size > HEADER_BYTES {
        file.seek(SeekFrom::Start(size.saturating_sub(HEADER_BYTES)))
            .map_err(io_error)?;
        file.take(HEADER_BYTES)
            .read_to_end(&mut tail)
            .map_err(io_error)?;
    }
    let hint = options_hash(
        &serde_json::json!({"size":size,"head":sha256_bytes(&head),"tail":sha256_bytes(&tail)}),
    );
    Ok((head, tail, format!("metadata-v1:{hint}")))
}

pub fn metadata_revision(path: &Path) -> Result<String, ImportError> {
    sample(path).map(|(_, _, h)| h)
}

pub fn file_candidate(
    provider: Provider,
    path: &Path,
    root: &Path,
) -> Result<SourceCandidate, ImportError> {
    file_candidate_scoped(provider, path, root, None)
}

pub(crate) fn file_candidate_in_location(
    provider: Provider,
    path: &Path,
    root: &Path,
    location_scope: Option<&str>,
) -> Result<SourceCandidate, ImportError> {
    match location_scope {
        None => file_candidate(provider, path, root),
        Some(scope) => file_candidate_scoped(provider, path, root, Some(scope)),
    }
}

fn file_candidate_scoped(
    provider: Provider,
    path: &Path,
    root: &Path,
    location_scope: Option<&str>,
) -> Result<SourceCandidate, ImportError> {
    let (header, tail, revision) = sample(path)?;
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut native = if provider == Provider::GenericFile {
        path.strip_prefix(root)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(path.file_name().unwrap_or_default()))
            .to_string_lossy()
            .into_owned()
    } else {
        name.clone()
    };
    let mut title = None;
    let mut cwd = None;
    let mut timestamp = None;
    let mut cursor_identity = if provider == Provider::Cursor {
        Some(super::adapters::cursor::TranscriptIdentity::new_scoped(
            path,
            Some(root),
            location_scope,
        )?)
    } else {
        None
    };
    for line in header.split(|b| *b == b'\n').take(64) {
        let Ok(value) =
            serde_json::from_slice::<Value>(line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line))
        else {
            continue;
        };
        if let Some(identity) = &mut cursor_identity {
            identity.observe(&value)?;
        }
        let payload = value.get("payload").unwrap_or(&value);
        if provider == Provider::Codex
            && value.get("type").and_then(Value::as_str) == Some("session_meta")
        {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            native = super::adapters::codex::history_base::branch_identity(&stem, payload)
                .or_else(|| payload.get("id").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or(native);
        }
        if provider == Provider::ClaudeCode {
            native = value
                .get("sessionId")
                .and_then(Value::as_str)
                .unwrap_or(&native)
                .to_owned();
        }
        if cwd.is_none() {
            cwd = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if timestamp.is_none() {
            timestamp = value
                .get("timestamp")
                .or_else(|| payload.get("timestamp"))
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if title.is_none() {
            if provider == Provider::Codex {
                title = super::adapters::codex::first_user_title(&value);
                continue;
            }
            if provider == Provider::Cursor {
                title = super::adapters::cursor::transcript_title(&value);
                continue;
            }
            if provider == Provider::ClaudeCode {
                continue;
            }
            let text = value
                .get("summary")
                .or_else(|| payload.get("message"))
                .and_then(Value::as_str)
                .or_else(|| value.pointer("/message/content").and_then(Value::as_str))
                .or_else(|| {
                    value
                        .pointer("/message/content/0/text")
                        .and_then(Value::as_str)
                })
                .or_else(|| payload.pointer("/content/0/text").and_then(Value::as_str));
            title = text.map(|s| s.chars().take(80).collect());
        }
    }
    if provider == Provider::ClaudeCode {
        // Renames are appended at the end. Reuse the already bounded samples;
        // do not scan a large transcript in full just to list its title.
        let mut titles = super::adapters::titles::ClaudeTitles::default();
        for line in header
            .split(|b| *b == b'\n')
            .chain(tail.split(|b| *b == b'\n'))
        {
            if let Ok(value) = serde_json::from_slice::<Value>(
                line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line),
            ) {
                titles.observe(&value);
            }
        }
        title = titles.finish();
    }
    if let Some(identity) = cursor_identity {
        native = identity.finish();
    }
    Ok(SourceCandidate {
        candidate_id: format!(
            "{}:{}",
            match provider {
                Provider::GenericFile => "file",
                Provider::ClaudeCode => "claude",
                Provider::Codex => "codex",
                Provider::Cursor => "cursor-transcript",
            },
            path.display()
        ),
        provider,
        title: title.unwrap_or_else(|| {
            if matches!(provider, Provider::Codex | Provider::ClaudeCode) {
                native.clone()
            } else {
                name
            }
        }),
        native_session_id: native,
        cwd,
        started_at: timestamp.clone(),
        updated_at: timestamp,
        locator: path.to_string_lossy().into_owned(),
        source_revision: revision,
        archived: (provider == Provider::Codex).then(|| {
            path.components()
                .any(|c| c.as_os_str() == "archived_sessions")
        }),
        completeness: "metadata-preview".into(),
    })
}

pub struct Walker {
    root: PathBuf,
    stack: Vec<(ReadDir, usize)>,
    first_file: Option<PathBuf>,
    provider: Provider,
}

fn is_link(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}

impl Walker {
    pub fn new(provider: Provider, root: &Path) -> Result<Self, ImportError> {
        let root = super::sources::canonical_root(root)?;
        let metadata = std::fs::metadata(&root).map_err(io_error)?;
        if metadata.is_file() && provider != Provider::GenericFile {
            return Err(ImportError::new(
                ErrorCode::FormatUnsupported,
                "source must be a session directory",
            ));
        }
        let (stack, first_file) = if metadata.is_file() {
            (Vec::new(), Some(root.clone()))
        } else {
            (vec![(std::fs::read_dir(&root).map_err(io_error)?, 0)], None)
        };
        Ok(Self {
            root,
            stack,
            first_file,
            provider,
        })
    }

    fn accepts(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(
            name,
            "auth.json" | "secrets.json" | "credentials.json" | "config.json"
        ) {
            return false;
        }
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match self.provider {
            Provider::GenericFile => matches!(extension, "json" | "md" | "markdown" | "txt"),
            Provider::Codex | Provider::ClaudeCode => extension == "jsonl",
            Provider::Cursor => extension == "jsonl" || matches!(name, "store.db" | "state.vscdb"),
        }
    }
}

impl Walker {
    /// Visit at most one directory entry. The caller checks cancellation and
    /// its traversal budget even when no matching artifact was discovered.
    pub fn step(&mut self) -> Option<Result<Option<PathBuf>, ImportError>> {
        if let Some(file) = self.first_file.take() {
            return Some(if self.accepts(&file) {
                Ok(Some(file))
            } else {
                Err(ImportError::new(
                    ErrorCode::FormatUnsupported,
                    "unsupported file extension",
                ))
            });
        }
        while let Some((directory, depth)) = self.stack.last_mut() {
            let depth = *depth;
            let entry = match directory.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => return Some(Err(io_error(error))),
                None => {
                    self.stack.pop();
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(e) => return Some(Err(io_error(e))),
            };
            if is_link(&metadata) {
                return Some(Ok(None));
            }
            if metadata.is_dir() {
                let name = entry.file_name();
                if matches!(
                    name.to_str(),
                    Some(
                        ".git"
                            | "node_modules"
                            | "target"
                            | "cache"
                            | "Cache"
                            | "skills"
                            | "plugins"
                            | "automations"
                            | ".sandbox-secrets"
                    )
                ) {
                    return Some(Ok(None));
                }
                if depth >= MAX_DEPTH {
                    return Some(Err(ImportError::new(
                        ErrorCode::ResourceLimit,
                        "source traversal depth exceeded",
                    )));
                }
                match super::sources::canonical_root(&path) {
                    Ok(real) if real.starts_with(&self.root) => {}
                    Ok(_) => return Some(Ok(None)),
                    Err(error) => return Some(Err(error)),
                }
                match std::fs::read_dir(path) {
                    Ok(dir) => self.stack.push((dir, depth + 1)),
                    Err(e) => return Some(Err(io_error(e))),
                }
            } else if metadata.is_file() && self.accepts(&path) {
                return Some(Ok(Some(path)));
            }
            return Some(Ok(None));
        }
        None
    }
}
