//! Mapping groups are source-installation + source cwd, never cwd alone.
use super::{
    errors::{ErrorCode, ImportError},
    hash::sha256_bytes,
    workspace::{self, WorkspaceMapping},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MappingSource {
    pub source_id: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MappingSubject {
    pub source: MappingSource,
    pub cwd: Option<String>,
}

impl MappingSubject {
    pub fn new(source_id: String, provider: String, cwd: Option<String>) -> Self {
        Self {
            source: MappingSource {
                source_id,
                provider,
            },
            cwd: cwd.filter(|value| !value.trim().is_empty()),
        }
    }
    pub fn root_id(&self) -> String {
        let bytes = serde_json::to_vec(&(&self.source.source_id, &self.source.provider, &self.cwd))
            .expect("mapping string tuple serializes");
        format!("root:source:{}", sha256_bytes(&bytes))
    }
}

fn conflict(message: &str) -> ImportError {
    ImportError::new(ErrorCode::MappingConflict, message)
}

pub fn propose(subjects: &[MappingSubject], fallback: Option<String>) -> Vec<WorkspaceMapping> {
    let mut seen = HashSet::new();
    subjects
        .iter()
        .filter_map(|subject| {
            let root_id = subject.root_id();
            if !seen.insert(root_id.clone()) {
                return None;
            }
            Some(WorkspaceMapping {
                root_id,
                source: Some(subject.source.clone()),
                target_project_id: fallback.clone(),
                explicit_target: None,
                unassigned: fallback.is_none(),
                user_choice_version: 1,
                primary: seen.len() == 1,
                suggested_target: subject.cwd.clone(),
                create_project: false,
                import_unassigned_anyway: false,
            })
        })
        .collect()
}

/// Every new plan gets scoped rows. Only unambiguous old-client input can be
/// upgraded here; already persisted legacy plans keep their original meaning.
pub fn normalize(
    subjects: &[MappingSubject],
    mappings: Vec<WorkspaceMapping>,
) -> Result<Vec<WorkspaceMapping>, ImportError> {
    let expected = propose(subjects, None);
    if mappings.is_empty() {
        return Ok(expected);
    }
    let mut ids = HashSet::new();
    if mappings.iter().any(|row| !ids.insert(&row.root_id)) {
        return Err(conflict("duplicate mapping roots"));
    }
    let scoped = mappings.iter().filter(|row| row.source.is_some()).count();
    if scoped > 0 {
        let by_id: BTreeMap<_, _> = mappings
            .iter()
            .map(|row| (row.root_id.as_str(), &row.source))
            .collect();
        if scoped != mappings.len()
            || mappings.len() != expected.len()
            || expected
                .iter()
                .any(|want| by_id.get(want.root_id.as_str()).copied() != Some(&want.source))
        {
            return Err(conflict("mapping sources do not cover the prepared roots"));
        }
        return Ok(mappings);
    }
    let mut used = BTreeMap::new();
    let mut output = Vec::new();
    for want in expected {
        if let Some(legacy) =
            workspace::mapping_for_cwd(&mappings, want.suggested_target.as_deref())
        {
            if used
                .insert(legacy.root_id.clone(), want.root_id.clone())
                .is_some()
            {
                return Err(conflict("legacy mapping is ambiguous across source installations; review mappings again"));
            }
            output.push(WorkspaceMapping {
                root_id: want.root_id,
                source: want.source,
                suggested_target: want.suggested_target,
                ..legacy.clone()
            });
        } else {
            output.push(want);
        }
    }
    if used.len() != mappings.len() {
        return Err(conflict("mapping does not belong to a prepared root"));
    }
    Ok(output)
}

pub fn resolve<'a>(
    mappings: &'a [WorkspaceMapping],
    subject: &MappingSubject,
) -> Result<Option<&'a WorkspaceMapping>, ImportError> {
    if mappings.iter().all(|row| row.source.is_none()) {
        return Ok(workspace::mapping_for_cwd(mappings, subject.cwd.as_deref()));
    }
    if mappings.iter().any(|row| row.source.is_none()) {
        return Err(conflict("mixed mapping identity versions"));
    }
    let id = subject.root_id();
    let mut matches = mappings
        .iter()
        .filter(|row| row.root_id == id && row.source.as_ref() == Some(&subject.source));
    let found = matches
        .next()
        .ok_or_else(|| conflict("mapping for this source root is missing"))?;
    if matches.next().is_some() {
        return Err(conflict("duplicate source mapping"));
    }
    Ok(Some(found))
}

pub fn options(row: Option<&WorkspaceMapping>) -> serde_json::Value {
    let mut options = serde_json::json!({"rawRetention": "normalized-only"});
    if let Some(row) = row.filter(|row| row.source.is_some()) {
        options["mappingScope"] =
            serde_json::json!({"version": 1, "source": row.source, "rootId": row.root_id});
    }
    options
}

pub fn validate_options(
    options: &serde_json::Value,
    subject: &MappingSubject,
) -> Result<(), ImportError> {
    if let Some(scope) = options.get("mappingScope") {
        let expected = serde_json::json!({"version": 1, "source": subject.source, "rootId": subject.root_id()});
        if scope != &expected {
            return Err(conflict(
                "durable mapping source does not match prepared history",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "mapping_scope_tests.rs"]
mod tests;
