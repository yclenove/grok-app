//! One key at a time from a private SQLite snapshot; chat bodies stay unread.

use super::errors::{ErrorCode, ImportError};
use super::sqlite_ro::{classify_query_err, open_readonly, table_names_at, ReadonlySnapshot};
use super::types::{Provider, SourceCandidate};
use rusqlite::{params, OptionalExtension};
use std::path::{Path, PathBuf};

pub struct ComposerDiscovery {
    snapshot: ReadonlySnapshot,
    path: PathBuf,
    tables: Vec<String>,
    table: usize,
    prefix: usize,
    after: String,
    revision: String,
    cwd: Option<String>,
}

impl ComposerDiscovery {
    pub fn open(path: &Path) -> Result<Self, ImportError> {
        let snapshot = open_readonly(path)?;
        let tables: Vec<_> = table_names_at(&snapshot, path)?
            .into_iter()
            .filter(|t| t == "ItemTable" || t == "cursorDiskKV")
            .collect();
        if tables.is_empty() {
            return Err(ImportError::new(
                ErrorCode::SchemaUnsupported,
                "unrecognized Composer schema",
            ));
        }
        let revision = snapshot.source_revision();
        let cwd = path
            .parent()
            .and_then(super::adapters::cursor::read_workspace_path);
        Ok(Self {
            snapshot,
            path: path.into(),
            tables,
            table: 0,
            prefix: 0,
            after: String::new(),
            revision,
            cwd,
        })
    }

    pub fn next_candidate(&mut self) -> Result<Option<SourceCandidate>, ImportError> {
        while let Some(table) = self.tables.get(self.table) {
            let (prefix, end, completeness) = match self.prefix {
                0 => ("composerData:", "composerData;", "composer"),
                _ => ("chatData:", "chatData;", "legacy-chat"),
            };
            // Table names come exclusively from the allowlist above. Key range
            // queries use the source primary key and never read value blobs.
            let sql = format!(
                "SELECT key FROM {table} WHERE key>=?1 AND key<?2 AND key>?3 ORDER BY key LIMIT 1"
            );
            let key: Option<String> = self
                .snapshot
                .query_row(&sql, params![prefix, end, self.after], |r| r.get(0))
                .optional()
                .map_err(|e| classify_query_err(&self.path, &e))?;
            let Some(key) = key else {
                self.prefix += 1;
                if self.prefix == 2 {
                    self.table += 1;
                    self.prefix = 0;
                }
                self.after.clear();
                continue;
            };
            self.after = key.clone();
            let id = key.strip_prefix(prefix).unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            if id.len() > 4096 {
                return Err(ImportError::new(
                    ErrorCode::ResourceLimit,
                    "Composer identity exceeds metadata budget",
                ));
            }
            return Ok(Some(SourceCandidate {
                candidate_id: format!("cursor-{completeness}:{id}"),
                provider: Provider::Cursor,
                native_session_id: super::adapters::cursor::database_identity(&key)?,
                title: id.chars().take(80).collect(),
                cwd: self.cwd.clone(),
                started_at: None,
                updated_at: None,
                locator: format!("{}#{key}", self.path.display()),
                source_revision: self.revision.clone(),
                archived: None,
                completeness: completeness.into(),
            }));
        }
        Ok(None)
    }
}
