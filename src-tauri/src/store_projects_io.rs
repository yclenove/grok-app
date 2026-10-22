//! Strict project snapshots. Unknown row fields survive edits from older Apps.
use super::Project;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
};

const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub(super) const IMPORT_ORIGIN_FIELD: &str = "importPublicationOrigin";
const KNOWN_FIELDS: &[&str] = &[
    "id",
    "name",
    "path",
    "trusted",
    "lastOpenedAt",
    "pathOk",
    "pinned",
    "system",
    "modelId",
    "effort",
    "mode",
    "permissionPolicy",
    "sandboxProfile",
    "color",
    "sshAlias",
];

pub(super) fn read_value(path: &Path, limit: u64) -> Result<Option<Value>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("project store is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("project store exceeds byte limit".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("invalid {}: {error}", path.display()))
}

pub(super) struct Snapshot {
    pub projects: Vec<Project>,
    raw: Value,
    new_import_origins: HashMap<String, String>,
}

/// Only this snapshot's checked new-row branch can construct this capability.
/// A reader holding an existing Project cannot ask the signer to adopt it.
pub(crate) struct NewImportProject<'snapshot> {
    project: &'snapshot Project,
}

impl NewImportProject<'_> {
    pub(crate) fn project(&self) -> &Project {
        self.project
    }
}

impl Snapshot {
    #[cfg(test)]
    pub fn has_unknown_fields(&self, id: &str) -> bool {
        self.has_unknown_fields_except(id, None)
    }

    /// Only a caller that verified this exact origin may exempt its one field.
    /// It remains unknown to ordinary/older mutation and removal paths.
    pub(super) fn has_unknown_fields_except_origin(&self, id: &str) -> bool {
        self.has_unknown_fields_except(id, Some(IMPORT_ORIGIN_FIELD))
    }

    fn has_unknown_fields_except(&self, id: &str, allowed: Option<&str>) -> bool {
        self.raw.as_array().into_iter().flatten().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(id)
                && row.as_object().is_some_and(|object| {
                    object.keys().any(|key| {
                        !KNOWN_FIELDS.contains(&key.as_str()) && Some(key.as_str()) != allowed
                    })
                })
        })
    }

    pub(super) fn import_origin(&self, id: &str) -> Result<Option<&str>, String> {
        let field = self
            .raw
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .and_then(|row| row.get(IMPORT_ORIGIN_FIELD));
        match field {
            None => Ok(None),
            Some(Value::String(value)) if value.len() <= 8192 => Ok(Some(value)),
            _ => Err("invalid native project creation metadata".into()),
        }
    }

    /// Never annotate a reused row. Origin and its new project share one atomic
    /// file replacement; an existing ID cannot be adopted into ownership.
    pub(super) fn push_import_project(
        &mut self,
        project: Project,
        creation: &crate::session_migration::ImportProjectCreation<'_>,
    ) -> Result<(), String> {
        if self.projects.iter().any(|row| row.id == project.id)
            || self
                .raw
                .as_array()
                .into_iter()
                .flatten()
                .any(|row| row.get("id").and_then(Value::as_str) == Some(project.id.as_str()))
        {
            return Err("cannot attach creation origin to an existing project".into());
        }
        let origin = creation
            .seal_new(NewImportProject { project: &project })
            .map_err(|error| error.code.as_str().to_string())?;
        self.new_import_origins.insert(project.id.clone(), origin);
        self.projects.push(project);
        Ok(())
    }

    pub fn read() -> Result<Self, String> {
        let raw =
            read_value(&crate::paths::projects_file(), MAX_BYTES)?.unwrap_or(Value::Array(vec![]));
        let projects: Vec<Project> = serde_json::from_value(raw.clone())
            .map_err(|error| format!("invalid projects: {error}"))?;
        validate_ids(&projects)?;
        Ok(Self {
            projects,
            raw,
            new_import_origins: HashMap::new(),
        })
    }

    /// Caller holds the projects lock. A no-op keeps the exact file bytes.
    pub fn save(&self) -> Result<bool, String> {
        validate_ids(&self.projects)?;
        let original: HashMap<_, _> = self
            .raw
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| Some((row.get("id")?.as_str()?, row.as_object()?)))
            .collect();
        let mut rows = Vec::new();
        for project in &self.projects {
            let mut value = serde_json::to_value(project).map_err(|error| error.to_string())?;
            if let Some(previous) = original.get(project.id.as_str()) {
                let object = value
                    .as_object_mut()
                    .ok_or("project serialization is not an object")?;
                for (key, field) in previous.iter() {
                    if !KNOWN_FIELDS.contains(&key.as_str()) {
                        object.insert(key.clone(), field.clone());
                    }
                }
            }
            if let Some(origin) = self.new_import_origins.get(&project.id) {
                if original.contains_key(project.id.as_str()) {
                    return Err("cannot replace native creation origin".into());
                }
                value
                    .as_object_mut()
                    .ok_or("project serialization is not an object")?
                    .insert(IMPORT_ORIGIN_FIELD.into(), Value::String(origin.clone()));
            }
            rows.push(value);
        }
        let after = Value::Array(rows);
        // Normalize only the representation for comparison; optional/default
        // fields absent in legacy JSON do not force unrelated metadata writes.
        let before_typed =
            serde_json::from_value::<Vec<Project>>(self.raw.clone()).map_err(|e| e.to_string())?;
        if self.new_import_origins.is_empty()
            && serde_json::to_value(&before_typed).map_err(|e| e.to_string())?
                == serde_json::to_value(&self.projects).map_err(|e| e.to_string())?
        {
            return Ok(false);
        }
        let bytes = serde_json::to_vec_pretty(&after).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("project store exceeds 16 MiB".into());
        }
        crate::store_lock::write_bytes_replace(&crate::paths::projects_file(), &bytes)?;
        Ok(true)
    }
}

fn validate_ids(projects: &[Project]) -> Result<(), String> {
    let mut ids = HashSet::new();
    if projects
        .iter()
        .any(|project| project.id.is_empty() || !ids.insert(&project.id))
    {
        return Err("project store has empty or duplicate identities".into());
    }
    Ok(())
}

pub(super) fn update<R>(
    mutate: impl FnOnce(&mut Vec<Project>) -> Result<R, String>,
) -> Result<(R, bool), String> {
    update_snapshot(|snapshot| mutate(&mut snapshot.projects))
}

pub(super) fn update_snapshot<R>(
    mutate: impl FnOnce(&mut Snapshot) -> Result<R, String>,
) -> Result<(R, bool), String> {
    let _lock = crate::store_lock::lock_exclusive(&crate::paths::projects_file())?;
    let mut snapshot = Snapshot::read()?;
    let result = mutate(&mut snapshot)?;
    let changed = snapshot.save()?;
    Ok((result, changed))
}

pub(super) fn load_repaired() -> Result<Vec<Project>, String> {
    let projects_path = crate::paths::projects_file();
    let sessions_path = crate::paths::sessions_index_file();
    let _lock = crate::store_lock::lock_exclusive_many(&[projects_path, sessions_path.clone()])?;
    let mut snapshot = Snapshot::read()?;
    let mut rehomes = super::dedup_ssh_projects_by_alias_path_after_health(&mut snapshot.projects);
    for project in snapshot
        .projects
        .iter()
        .filter(|project| project.is_legacy_general())
    {
        rehomes.push((project.id.clone(), None));
    }
    snapshot
        .projects
        .retain(|project| !project.is_legacy_general());
    rehomes.push((super::GENERAL_PROJECT_ID.into(), None));
    // Rehome before removing source rows. A crash between the two replacements
    // leaves redundant project rows, not sessions bound to a deleted project.
    // Both file locks stay held; the two writes are not a cross-file transaction.
    if let Some(mut sessions) = read_value(&sessions_path, 64 * 1024 * 1024)? {
        let rows = sessions
            .as_array_mut()
            .ok_or("sessions index is not an array")?;
        let mut changed = false;
        for row in rows {
            let object = row
                .as_object_mut()
                .ok_or("sessions index has an invalid row")?;
            if let Some((_, target)) = rehomes.iter().find(|(id, _)| {
                object.get("projectId").and_then(Value::as_str) == Some(id.as_str())
            }) {
                object.insert(
                    "projectId".into(),
                    target.clone().map(Value::String).unwrap_or(Value::Null),
                );
                changed = true;
            }
        }
        if changed {
            crate::store_lock::write_bytes_replace(
                &sessions_path,
                &serde_json::to_vec_pretty(&sessions).map_err(|e| e.to_string())?,
            )?;
        }
    }
    super::apply_project_pin_partition(&mut snapshot.projects);
    snapshot.save()?;
    Ok(snapshot.projects)
}
