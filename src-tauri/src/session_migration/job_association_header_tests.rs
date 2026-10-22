//! Header/negative row probes only, never accepted import/proof authority.
use super::*;
use crate::session_migration::{identity_association::AssociationConfirmation, jobs};
use serde_json::{json, Value};

const JOB_BYTES: usize = 16 * 1024 * 1024;

struct Fixture {
    conn: Connection,
    job: JobRecord,
    association: AssociationInput,
    raw: String,
}

impl Fixture {
    fn new() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE jobs(job_id TEXT PRIMARY KEY,request_id TEXT NOT NULL UNIQUE,
             plan_hash TEXT NOT NULL,state TEXT NOT NULL,state_version INTEGER NOT NULL,
             json TEXT NOT NULL,created_at TEXT NOT NULL);
             CREATE TABLE job_items(job_id TEXT,candidate_id TEXT,phase TEXT);",
        )
        .unwrap();
        let job = JobRecord {
            job_id: uuid::Uuid::new_v4().to_string(),
            request_id: "SYNTHETIC_HEADER_PROBE".into(),
            plan_hash: "a".repeat(64),
            plan_id: uuid::Uuid::new_v4().to_string(),
            state: jobs::JobState::Queued,
            state_version: 1,
            items: Vec::new(),
            created_at: "2026-10-13T00:00:00Z".into(),
            item_count: 0,
            next_item_cursor: None,
            progress: None,
        };
        // The serializer is real, but this isolated header row has no accepted
        // plan/input/archive/receipt. It cannot prove positive authorization.
        jobs::save_job(&conn, &job).unwrap();
        let association = AssociationInput {
            confirmation: AssociationConfirmation {
                version: 1,
                import_id: format!("imp:{}", uuid::Uuid::new_v4()),
                expected_revision: uuid::Uuid::new_v4().to_string(),
                preview_id: uuid::Uuid::new_v4().to_string(),
                review_digest: "b".repeat(64),
            },
            job_id: job.job_id.clone(),
            request_id: job.request_id.clone(),
            plan_id: job.plan_id.clone(),
            plan_hash: job.plan_hash.clone(),
        };
        let raw = serde_json::to_string(&job).unwrap();
        Self {
            conn,
            job,
            association,
            raw,
        }
    }

    fn replace(&self, raw: &str) {
        self.conn.execute("UPDATE jobs SET json=?1", [raw]).unwrap();
    }

    fn read(&self) -> Result<JobRecord, ImportError> {
        self.association.stored_job(&self.conn, &self.job.job_id)
    }
}

#[test]
fn stored_job_size_classification_does_not_materialize_json_in_sqlite() {
    let f = Fixture::new();
    let opcodes = |sql: &str| {
        f.conn
            .prepare(&format!("EXPLAIN {sql}"))
            .unwrap()
            .query_map([&f.job.job_id], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let codes = opcodes(STORED_JOB_SQL);
    assert!(
        !codes
            .iter()
            .any(|(op, column, flags)| { op == "Column" && *column == 5 && flags & 128 == 0 }),
        "classification must not fetch jobs.json; typeof's metadata flag is allowed"
    );
    assert!(!codes.iter().any(|(op, _, _)| op == "Cast"));
    assert!(
        opcodes("SELECT json FROM jobs WHERE job_id=?1")
            .iter()
            .any(|(op, column, flags)| { op == "Column" && *column == 5 && flags & 128 == 0 }),
        "the control must detect an actual body read"
    );
}

#[test]
fn stored_job_rejects_duplicate_root_fields_including_removed_items() {
    let f = Fixture::new();
    let value: Value = serde_json::from_str(&f.raw).unwrap();
    for (key, part) in value.as_object().unwrap() {
        let raw = format!("{{\"{key}\":{part},{}", &f.raw[1..]);
        f.replace(&raw);
        assert!(
            f.read().is_err(),
            "duplicate {key} must not be rewritten away"
        );
        assert!(f.conn.is_autocommit());
    }
    let escaped = format!("{{\"\\u0069tems\":[],{}", &f.raw[1..]);
    f.replace(&escaped);
    assert!(f.read().is_err());
}

#[test]
fn stored_job_rejects_unknown_future_authority_fields() {
    let f = Fixture::new();
    for extra in [
        "\"version\":2",
        "\"authorized\":true",
        "\"identityAssociation\":null",
    ] {
        f.replace(&format!("{{{extra},{}", &f.raw[1..]));
        assert!(f.read().is_err(), "unknown root field {extra}");
        assert!(f.conn.is_autocommit());
    }
}

#[test]
fn stored_job_requires_the_original_created_at_column() {
    let f = Fixture::new();
    assert!(f.read().is_ok());
    f.conn
        .execute("UPDATE jobs SET created_at='2026-10-14T00:00:00Z'", [])
        .unwrap();
    assert_eq!(f.read().unwrap_err().code, ErrorCode::PlanStale);
    assert!(f.conn.is_autocommit());
}

#[test]
fn stored_job_enforces_its_original_byte_cap_before_body_parsing() {
    let f = Fixture::new();
    let boundary = format!("{}{}", f.raw, " ".repeat(JOB_BYTES - f.raw.len()));
    f.replace(&boundary);
    assert!(f.read().is_ok());
    f.replace(&format!("{boundary} "));
    assert_eq!(f.read().unwrap_err().code, ErrorCode::ResourceLimit);
    assert!(f.conn.is_autocommit());
    f.conn
        .execute("UPDATE jobs SET json=?1", [vec![b'x'; JOB_BYTES + 1]])
        .unwrap();
    assert!(
        f.read().is_err(),
        "a non-TEXT job body must not be accepted"
    );
}

#[test]
fn stored_job_projection_ignores_large_items_and_mutable_progress() {
    let f = Fixture::new();
    let mut value: Value = serde_json::from_str(&f.raw).unwrap();
    value["items"] =
        json!([{"candidateId":"SYNTHETIC_BODY_ONLY", "padding":"汉\\\"{}[]".repeat(40_000)}]);
    value["itemCount"] = json!(1);
    value["state"] = json!("running");
    value["stateVersion"] = json!(2);
    value["progress"] = json!({"done":0,"retryable":1});
    f.replace(&value.to_string());
    f.conn
        .execute("UPDATE jobs SET state='running',state_version=2", [])
        .unwrap();
    jobs::take_job_item_deserializes();
    let read = f.read().unwrap();
    assert!(read.items.is_empty());
    assert_eq!(jobs::take_job_item_deserializes(), 0);
    f.association.require_job(&read).unwrap();
    assert_eq!(read.state, jobs::JobState::Running);
    assert_eq!(read.state_version, 2);
    let before: String = f
        .conn
        .query_row("SELECT json FROM jobs", [], |r| r.get(0))
        .unwrap();
    let tx = f.conn.unchecked_transaction().unwrap();
    assert!(f.association.stored_job(&tx, &f.job.job_id).is_ok());
    assert!(
        !f.conn.is_autocommit(),
        "caller snapshot remains owned by caller"
    );
    drop(tx);
    let after: String = f
        .conn
        .query_row("SELECT json FROM jobs", [], |r| r.get(0))
        .unwrap();
    assert!(
        before == after,
        "header reading must not rewrite the stored job"
    );
}

#[test]
fn stored_job_rejects_missing_wrong_type_and_mismatched_identity_fields() {
    let f = Fixture::new();
    let value: Value = serde_json::from_str(&f.raw).unwrap();
    for key in [
        "jobId",
        "requestId",
        "planId",
        "planHash",
        "state",
        "stateVersion",
        "createdAt",
        "items",
    ] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        f.replace(&missing.to_string());
        assert!(f.read().is_err(), "missing {key}");
    }
    for key in ["jobId", "requestId", "planId", "planHash", "createdAt"] {
        for replacement in [json!(null), json!(17), json!([]), json!({})] {
            let mut wrong = value.clone();
            wrong[key] = replacement;
            f.replace(&wrong.to_string());
            assert!(f.read().is_err(), "wrong type {key}");
        }
        let mut wrong = value.clone();
        wrong[key] = json!("SYNTHETIC_FOREIGN_BINDING");
        f.replace(&wrong.to_string());
        assert!(f.read().is_err(), "mismatched {key}");
    }
    for replacement in [json!(-1), json!(4294967296_u64), json!("1"), json!(1.5)] {
        let mut wrong = value.clone();
        wrong["stateVersion"] = replacement;
        f.replace(&wrong.to_string());
        assert!(f.read().is_err());
    }
    for raw in [
        format!("{{\"progress\":null,\"progress\":null,{}", &f.raw[1..]),
        format!(
            "{{\"nextItemCursor\":null,\"nextItemCursor\":null,{}",
            &f.raw[1..]
        ),
        format!("{}{{}}", f.raw),
        format!(
            "{{\"items\":{}0{},{}",
            "[".repeat(129),
            "]".repeat(129),
            &f.raw[1..]
        ),
    ] {
        f.replace(&raw);
        assert!(f.read().is_err());
        assert!(f.conn.is_autocommit());
    }
    f.replace(&f.raw);
    assert!(
        f.read().is_ok(),
        "negative reads must leave the row retryable"
    );
}

#[test]
fn stored_job_bounds_and_validates_column_metadata_before_owned_fetches() {
    for column in ["request_id", "plan_hash", "state", "created_at", "json"] {
        let f = Fixture::new();
        f.conn
            .execute(
                &format!("UPDATE jobs SET {column}=?1"),
                [b"SYNTHETIC_BLOB".as_slice()],
            )
            .unwrap();
        assert!(f.read().is_err(), "non-TEXT {column}");
        assert!(f.conn.is_autocommit());
    }
    for column in ["request_id", "plan_hash", "state", "created_at"] {
        let f = Fixture::new();
        let bound = match column {
            "request_id" => JOB_BYTES,
            "plan_hash" => 64,
            "state" => 32,
            "created_at" => 1024,
            _ => unreachable!(),
        };
        f.conn
            .execute(
                &format!("UPDATE jobs SET {column}=?1"),
                ["x".repeat(bound + 1)],
            )
            .unwrap();
        assert_eq!(
            f.read().unwrap_err().code,
            ErrorCode::ResourceLimit,
            "{column}"
        );
        assert!(f.conn.is_autocommit());
    }
    let f = Fixture::new();
    f.conn
        .execute("UPDATE jobs SET state_version='SYNTHETIC_WRONG_TYPE'", [])
        .unwrap();
    assert!(f.read().is_err());
    let f = Fixture::new();
    let mut unicode: Value = serde_json::from_str(&f.raw).unwrap();
    unicode["items"] = json!(["汉".repeat(JOB_BYTES / 3)]);
    let raw = unicode.to_string();
    assert!(raw.chars().count() < JOB_BYTES && raw.len() > JOB_BYTES);
    f.replace(&raw);
    assert_eq!(
        f.read().unwrap_err().code,
        ErrorCode::ResourceLimit,
        "byte limit, not character limit"
    );
}

#[test]
fn stored_job_streaming_tokens_are_bounded_across_fragmented_reads() {
    use std::io::{self, Cursor, Read};
    struct Fragmented<'a> {
        inner: Cursor<Vec<u8>>,
        bytes: &'a std::cell::Cell<usize>,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let limit = buffer.len().min(7);
            let count = self.inner.read(&mut buffer[..limit])?;
            self.bytes.set(self.bytes.get() + count);
            Ok(count)
        }
    }
    for (key, limit) in [
        ("jobId", 512),
        ("planHash", 512),
        ("stateVersion", 512),
        ("createdAt", 2048),
    ] {
        let raw = format!("{{\"{key}\":\"{}\"}}", "x".repeat(10_000));
        let bytes = std::cell::Cell::new(0);
        let reader = Fragmented {
            inner: Cursor::new(raw.into_bytes()),
            bytes: &bytes,
        };
        assert!(stored_header::parse(reader).is_err());
        assert!(bytes.get() <= limit, "bounded scratch for {key}");
    }
    let f = Fixture::new();
    // Fully escaped keys/IDs still fit; JSON grammar comes from serde_json,
    // while the shared reader only guards scratch and depth.
    let mut raw = f.raw.replace("\"jobId\"", "\"\\u006aobId\"");
    raw = raw.replace(
        &f.job.job_id,
        &f.job
            .job_id
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect::<String>(),
    );
    let bytes = std::cell::Cell::new(0);
    let reader = Fragmented {
        inner: Cursor::new(raw.as_bytes().to_vec()),
        bytes: &bytes,
    };
    let parsed = stored_header::parse(reader).unwrap();
    f.association.require_job(&parsed).unwrap();
    assert_eq!(bytes.get(), raw.len());
}
