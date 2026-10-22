//! Formal scan contracts for explicitly claimed source locations/hosts.
//! All files are synthetic and owned by the test's isolated App home.
use super::*;
use crate::session_migration::{catalog, sources, test_home::IsolatedAppHome};
use serde_json::{json, Value};

struct Fixture {
    home: IsolatedAppHome,
    primary: PathBuf,
    other: PathBuf,
    source: sources::SourceRegistration,
    source_row: (String, Option<String>),
    primary_bytes: Vec<u8>,
    other_bytes: Vec<u8>,
}

impl Fixture {
    fn new() -> Self {
        let home = IsolatedAppHome::acquire("source-location-scan");
        let primary = home.path.join("primary source");
        let other = home.path.join("other source");
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let primary_bytes = br#"[{"role":"user","content":"PRIMARY_LOCATION_MARKER"}]"#.to_vec();
        let other_bytes = br#"[{"role":"user","content":"OTHER_LOCATION_MARKER"}]"#.to_vec();
        std::fs::write(primary.join("history.json"), &primary_bytes).unwrap();
        std::fs::write(other.join("history.json"), &other_bytes).unwrap();
        let conn = catalog::open_catalog().unwrap();
        let source =
            sources::register(&conn, Provider::GenericFile, &primary, None, "fixture").unwrap();
        let source_row = conn
            .query_row(
                "SELECT root_path,profile FROM sources WHERE installation_id=?1",
                [&source.source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        drop(conn);
        Self {
            home,
            primary,
            other,
            source,
            source_row,
            primary_bytes,
            other_bytes,
        }
    }

    fn scan(&self, request: Value) -> ScanState {
        let request: ScanSourcePath = serde_json::from_value(request).unwrap();
        let started = begin(vec![request]).unwrap();
        run(&started.scan_id).unwrap();
        state(&started.scan_id, started.epoch).unwrap()
    }

    fn assert_rejected_without_registration_mutation(&self, done: &ScanState, code: ErrorCode) {
        assert_eq!(
            std::fs::read(self.primary.join("history.json")).unwrap(),
            self.primary_bytes
        );
        assert_eq!(
            std::fs::read(self.other.join("history.json")).unwrap(),
            self.other_bytes
        );
        let conn = catalog::open_catalog().unwrap();
        let row: (String, Option<String>) = conn
            .query_row(
                "SELECT root_path,profile FROM sources WHERE installation_id=?1",
                [&self.source.source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let registered = sources::list(&conn).unwrap();
        assert_eq!(
            (done.discovered_count, row, registered.len()),
            (0, self.source_row.clone(), 1),
            "An unresolved location/host must not scan or relocate: {done:?}"
        );
        assert!(done.candidates.is_empty());
        assert_eq!(done.issue_count, 1);
        assert_eq!(done.issues[0].code, code.as_str());
        assert!(self.home.path.exists());
    }
}

#[test]
fn claimed_unknown_location_is_not_ignored_or_used_as_relocation() {
    let fixture = Fixture::new();
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.other,
        "sourceId":fixture.source.source_id, "locationId":uuid::Uuid::new_v4().to_string(),
        "host":"local", "profile":"fixture"
    }));
    fixture.assert_rejected_without_registration_mutation(&done, ErrorCode::SourceNotFound);
}

#[test]
fn claimed_remote_host_is_not_silently_scanned_as_a_local_source() {
    let fixture = Fixture::new();
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.other,
        "sourceId":fixture.source.source_id, "host":"ssh:synthetic-host", "profile":"fixture"
    }));
    fixture.assert_rejected_without_registration_mutation(&done, ErrorCode::FormatUnsupported);
}

#[test]
fn claimed_location_without_installation_does_not_auto_register_a_new_source() {
    let fixture = Fixture::new();
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.other,
        "locationId":uuid::Uuid::new_v4().to_string(), "host":"local", "profile":"fixture"
    }));
    fixture.assert_rejected_without_registration_mutation(&done, ErrorCode::MappingConflict);
}

#[test]
fn ordinary_explicit_relocation_keeps_the_existing_installation() {
    let fixture = Fixture::new();
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.other,
        "sourceId":fixture.source.source_id, "profile":"fixture"
    }));
    assert_eq!(done.discovered_count, 1);
    assert_eq!(done.issue_count, 0);
    let conn = catalog::open_catalog().unwrap();
    let registration = sources::get(&conn, &fixture.source.source_id).unwrap();
    assert_eq!(
        registration.root_path,
        sources::canonical_root(&fixture.other)
            .unwrap()
            .to_string_lossy()
    );
    assert_eq!(sources::list(&conn).unwrap().len(), 1);
    assert_eq!(
        std::fs::read(fixture.primary.join("history.json")).unwrap(),
        fixture.primary_bytes
    );
    assert_eq!(
        std::fs::read(fixture.other.join("history.json")).unwrap(),
        fixture.other_bytes
    );
}

fn primary_location(
    fixture: &Fixture,
) -> crate::session_migration::source_locations::SourceLocation {
    let page = tauri::async_runtime::block_on(crate::commands::session_import_source_locations(
        fixture.source.source_id.clone(),
        "generic-file".into(),
        Some("local".into()),
        None,
        Some(1),
    ))
    .unwrap();
    assert_eq!(page.total_count, 1);
    assert!(page.next_cursor.is_none());
    assert_eq!(page.locations.len(), 1);
    page.locations.into_iter().next().unwrap()
}

fn schema(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    conn.prepare("SELECT name,type,sql FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect()
}

#[test]
fn primary_location_command_is_readonly_and_id_survives_explicit_relocation() {
    let fixture = Fixture::new();
    let before = schema(&catalog::open_catalog_readonly().unwrap());
    let location = primary_location(&fixture);
    assert_eq!(location.source_id, fixture.source.source_id);
    assert_eq!(location.root_path, fixture.source.root_path);
    assert_eq!(location.version, 1);
    assert_eq!(location.provider, Provider::GenericFile);
    assert_eq!(location.host, "local");
    assert_eq!(location.kind, "primary");
    assert_eq!(schema(&catalog::open_catalog_readonly().unwrap()), before);
    let conn = catalog::open_catalog().unwrap();
    let row: (String, Option<String>) = conn
        .query_row(
            "SELECT root_path,profile FROM sources WHERE installation_id=?1",
            [&fixture.source.source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, fixture.source_row);
    sources::register(
        &conn,
        Provider::GenericFile,
        &fixture.other,
        Some(&fixture.source.source_id),
        "fixture",
    )
    .unwrap();
    drop(conn);
    let moved = primary_location(&fixture);
    assert_eq!(moved.location_id, location.location_id);
    assert_ne!(moved.root_path, location.root_path);
    assert_eq!(schema(&catalog::open_catalog_readonly().unwrap()), before);
}

#[test]
fn pinned_primary_scan_reads_the_selected_alias_not_the_new_primary() {
    use crate::session_migration::{engine, job_control, jobs::JobState};
    let fixture = Fixture::new();
    engine::reset_runtime();
    let location = primary_location(&fixture);
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.primary,
        "sourceId":fixture.source.source_id, "locationId":location.location_id,
        "host":"local", "profile":"fixture"
    }));
    assert_eq!(done.discovered_count, 1);
    assert_eq!(done.issue_count, 0);
    let candidate = &done.candidates[0];
    let pinned = super::candidate(&candidate.candidate_id).unwrap();
    assert_eq!(pinned.location.as_ref(), Some(&location));
    let conn = catalog::open_catalog().unwrap();
    sources::register(
        &conn,
        Provider::GenericFile,
        &fixture.other,
        Some(&fixture.source.source_id),
        "fixture",
    )
    .unwrap();
    drop(conn);
    let (preview, details) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let text = serde_json::to_string(&details.events).unwrap();
    assert!(text.contains("PRIMARY_LOCATION_MARKER"));
    assert!(!text.contains("OTHER_LOCATION_MARKER"));
    let plan = engine::plan(vec![preview], vec![]).unwrap();
    let queued = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap();
    job_control::run_pending().unwrap();
    let job = engine::job_get(&queued.job_id).unwrap();
    assert_eq!(job.state, JobState::Completed);
    let import_id = job.items[0].import_id.as_ref().unwrap();
    let revision = job.items[0].revision_id.as_ref().unwrap();
    engine::reset_runtime();
    let (events, next, _) =
        engine::events_page(import_id, Some(revision), None, Some(20), None).unwrap();
    assert!(next.is_none());
    let text = serde_json::to_string(&events).unwrap();
    assert!(text.contains("PRIMARY_LOCATION_MARKER"));
    assert!(!text.contains("OTHER_LOCATION_MARKER"));
    assert_eq!(primary_location(&fixture).location_id, location.location_id);
    assert_eq!(
        std::fs::read(fixture.primary.join("history.json")).unwrap(),
        fixture.primary_bytes
    );
    assert_eq!(
        std::fs::read(fixture.other.join("history.json")).unwrap(),
        fixture.other_bytes
    );
}

#[test]
fn primary_location_rejects_path_and_provider_substitution_without_relocation() {
    let fixture = Fixture::new();
    let location = primary_location(&fixture);
    let changed = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.other,
        "sourceId":fixture.source.source_id, "locationId":location.location_id, "host":"local"
    }));
    fixture.assert_rejected_without_registration_mutation(&changed, ErrorCode::SourceChanged);
    let other_provider = fixture.scan(json!({
        "provider":"cursor", "path":fixture.primary,
        "sourceId":fixture.source.source_id, "locationId":location.location_id, "host":"local"
    }));
    fixture
        .assert_rejected_without_registration_mutation(&other_provider, ErrorCode::SourceNotFound);
}

#[test]
fn preview_rejects_forged_location_reference_version_or_owner() {
    let fixture = Fixture::new();
    let location = primary_location(&fixture);
    let done = fixture.scan(json!({
        "provider":"generic-file", "path":fixture.primary,
        "sourceId":fixture.source.source_id, "locationId":location.location_id, "host":"local"
    }));
    let pinned = super::candidate(&done.candidates[0].candidate_id).unwrap();
    for change in ["version", "owner", "root"] {
        let mut reference = pinned.clone();
        let location = reference.location.as_mut().unwrap();
        match change {
            "version" => location.version += 1,
            "owner" => location.source_id = uuid::Uuid::new_v4().to_string(),
            _ => location.root_path = fixture.other.to_string_lossy().into_owned(),
        }
        let error =
            crate::session_migration::engine::preview_reference(reference, None, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::SourceChanged);
    }
}

#[test]
fn source_register_command_does_not_ignore_location_or_remote_host_claims() {
    let fixture = Fixture::new();
    let location = primary_location(&fixture);
    for (extra, code) in [
        (
            json!({"locationId":location.location_id}),
            "MAPPING_CONFLICT:",
        ),
        (json!({"host":"ssh:synthetic-host"}), "FORMAT_UNSUPPORTED:"),
    ] {
        let mut request = json!({
            "provider":"generic-file", "path":fixture.other,
            "sourceId":fixture.source.source_id, "profile":"fixture"
        });
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let request: ScanSourcePath = serde_json::from_value(request).unwrap();
        let error = tauri::async_runtime::block_on(
            crate::commands::session_import_source_register(request),
        )
        .unwrap_err();
        assert!(error.starts_with(code), "{error}");
    }
    assert_eq!(
        primary_location(&fixture).root_path,
        fixture.source.root_path
    );
    assert_eq!(
        std::fs::read(fixture.other.join("history.json")).unwrap(),
        fixture.other_bytes
    );
}
