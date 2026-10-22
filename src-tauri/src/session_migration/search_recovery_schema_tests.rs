use super::*;
use crate::session_migration::types::{ContentBlock, EventBase, MessageRole, TimeOrigin};
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Copy)]
enum Artifact {
    Missing,
    Current,
    Old,
    Future,
    FutureWithoutEvents,
    Corrupt,
}

fn fixture(layout: [Artifact; 3]) -> Fixture {
    let fixture =
        Fixture(std::env::temp_dir().join(format!("idx-future-recovery-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&fixture.0).unwrap();
    let event = ImportedEvent::Message {
        base: EventBase {
            id: "preserve".into(),
            ordinal: 0,
            spans: vec![],
            timestamp: None,
            time_origin: TimeOrigin::Unknown,
        },
        role: MessageRole::User,
        blocks: vec![ContentBlock::text("immutable synthetic archive")],
        complete: true,
    };
    let archive = format!("{}\n", serde_json::to_string(&event).unwrap());
    std::fs::write(fixture.0.join("events.jsonl"), archive).unwrap();
    build_index(&fixture.0, &[event]).unwrap();
    let live = index_path(&fixture.0);
    let bytes = std::fs::read(&live).unwrap();
    std::fs::remove_file(&live).unwrap();
    let paths = [
        live.clone(),
        index_backup_path(&live),
        fixture.0.join("event-index.sqlite.tmp"),
    ];
    for (path, artifact) in paths.iter().zip(layout) {
        match artifact {
            Artifact::Missing => continue,
            Artifact::Corrupt => {
                std::fs::write(path, b"synthetic-invalid-sqlite").unwrap();
            }
            _ => {
                std::fs::write(path, &bytes).unwrap();
                let conn = Connection::open(path).unwrap();
                match artifact {
                    Artifact::Old => conn.execute_batch("PRAGMA user_version=1;").unwrap(),
                    Artifact::Future => conn.execute_batch("PRAGMA user_version=99;").unwrap(),
                    Artifact::FutureWithoutEvents => conn
                        .execute_batch("DROP TABLE events; PRAGMA user_version=99;")
                        .unwrap(),
                    _ => {}
                }
            }
        }
    }
    fixture
}

fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

fn assert_fail_closed(layout: [Artifact; 3]) {
    let fixture = fixture(layout);
    let before = snapshot(&fixture.0);
    for rebuild in [false, true] {
        let result = if rebuild {
            rebuild_index(&fixture.0, &|| false)
        } else {
            ensure_index(&fixture.0)
        };
        assert!(
            before == snapshot(&fixture.0),
            "future-schema recovery must preserve every artifact, rebuild={rebuild}"
        );
        assert_eq!(
            result.expect_err("future schema must fail closed").code,
            ErrorCode::ImportSchemaTooNew,
            "rebuild={rebuild}"
        );
    }
}

#[test]
fn future_live_with_valid_backup_is_not_downgraded() {
    assert_fail_closed([Artifact::Future, Artifact::Current, Artifact::Missing]);
}

#[test]
fn future_live_without_current_table_is_not_discarded() {
    assert_fail_closed([
        Artifact::FutureWithoutEvents,
        Artifact::Current,
        Artifact::Missing,
    ]);
}

#[test]
fn future_backup_with_valid_live_is_not_deleted() {
    assert_fail_closed([Artifact::Current, Artifact::Future, Artifact::Missing]);
}

#[test]
fn future_backup_with_corrupt_live_is_not_discarded() {
    assert_fail_closed([Artifact::Corrupt, Artifact::Future, Artifact::Missing]);
}

#[test]
fn future_backup_with_missing_live_and_current_tmp_is_not_replaced() {
    assert_fail_closed([Artifact::Missing, Artifact::Future, Artifact::Current]);
}

#[test]
fn future_tmp_with_missing_live_and_valid_backup_is_not_promoted() {
    assert_fail_closed([Artifact::Missing, Artifact::Current, Artifact::Future]);
}

#[test]
fn future_tmp_with_valid_live_is_not_ignored_then_deleted() {
    assert_fail_closed([Artifact::Current, Artifact::Missing, Artifact::Future]);
}

#[test]
fn explicit_rebuild_preserves_future_live_without_backup() {
    assert_fail_closed([Artifact::Future, Artifact::Missing, Artifact::Missing]);
}

#[test]
fn future_backup_without_live_is_not_renamed() {
    assert_fail_closed([Artifact::Missing, Artifact::Future, Artifact::Missing]);
}

#[test]
fn future_tmp_with_corrupt_live_is_preserved() {
    assert_fail_closed([Artifact::Corrupt, Artifact::Missing, Artifact::Future]);
}

#[test]
fn future_schema_guard_precedes_old_schema_rebuild() {
    assert_fail_closed([Artifact::Old, Artifact::Current, Artifact::Future]);
}

#[test]
fn future_backup_without_current_table_is_not_deleted() {
    assert_fail_closed([
        Artifact::Current,
        Artifact::FutureWithoutEvents,
        Artifact::Missing,
    ]);
}

#[test]
fn future_tmp_without_current_table_is_not_discarded() {
    assert_fail_closed([
        Artifact::Current,
        Artifact::Missing,
        Artifact::FutureWithoutEvents,
    ]);
}

#[path = "search_recovery_lock_tests.rs"]
mod lock_tests;
