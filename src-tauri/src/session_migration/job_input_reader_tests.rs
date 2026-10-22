//! Stored-row/header/retention probes, not accepted association grants.
use super::*;
use crate::session_migration::errors::ErrorCode;
use std::{cell::Cell, io::Cursor};

fn database() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE job_items(job_id TEXT,candidate_id TEXT,result_json TEXT,
         PRIMARY KEY(job_id,candidate_id));
         INSERT INTO job_items VALUES('j','c','{}');",
    )
    .unwrap();
    conn
}

#[test]
fn stored_size_lookup_does_not_load_the_result_column_in_sqlite() {
    let conn = database();
    let opcodes = |sql: &str| -> Vec<(String, i64)> {
        conn.prepare(&format!("EXPLAIN {sql}"))
            .unwrap()
            // EXPLAIN's P2 operand is column index 3, not P3 at index 4.
            .query_map(params!["j", "c"], |row| Ok((row.get(1)?, row.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let codes = opcodes(ROW_ID_SQL);
    assert!(
        !codes
            .iter()
            .any(|(opcode, column)| opcode == "Column" && *column == 2),
        "size classification may not fetch result_json into SQLite memory"
    );
    assert!(
        !codes.iter().any(|(opcode, _)| opcode == "Cast"),
        "byte length comes from BLOB metadata, not a SQL value CAST"
    );
    assert!(
        opcodes("SELECT result_json FROM job_items WHERE job_id=?1 AND candidate_id=?2")
            .iter()
            .any(|(opcode, column)| opcode == "Column" && *column == 2)
    );
}

#[test]
fn oversized_stored_header_cannot_downgrade_or_hide_duplicate_fields() {
    let conn = database();
    let padding = "x".repeat(input_wire::ASSOCIATION_BYTES);
    for (header, expected) in [
        (
            "\"version\":3,\"identityAssociation\":null",
            ErrorCode::ResourceLimit,
        ),
        (
            "\"version\":3,\"identityAssociation\":{}",
            ErrorCode::ResourceLimit,
        ),
        ("\"version\":3", ErrorCode::SchemaUnsupported),
        (
            "\"version\":2,\"identityAssociation\":null",
            ErrorCode::SchemaUnsupported,
        ),
        (
            "\"version\":4,\"identityAssociation\":null",
            ErrorCode::SchemaUnsupported,
        ),
        (
            "\"version\":2,\"version\":3,\"identityAssociation\":null",
            ErrorCode::SchemaUnsupported,
        ),
        (
            "\"version\":3,\"version\":2,\"identityAssociation\":null",
            ErrorCode::SchemaUnsupported,
        ),
        (
            "\"version\":3,\"identityAssociation\":null,\"identityAssociation\":{}",
            ErrorCode::SchemaUnsupported,
        ),
        (
            "\"\\u0076ersion\":3,\"identity\\u0041ssociation\":null",
            ErrorCode::ResourceLimit,
        ),
    ] {
        let json = format!("{{{header},\"adapter\":{{\"syntheticPadding\":\"{padding}\"}}}}");
        // A BLOB rejects a full String fetch, so the classified error must come
        // from the bounded streaming header before that fetch is attempted.
        conn.execute("UPDATE job_items SET result_json=?1", [json.as_bytes()])
            .unwrap();
        assert_eq!(
            load(&conn, "j", "c").unwrap_err().code,
            expected,
            "{header}"
        );
        assert!(conn.is_autocommit(), "the read snapshot must be released");
    }
}

struct Fragmented<'a> {
    inner: Cursor<Vec<u8>>,
    bytes: &'a Cell<usize>,
}
impl Read for Fragmented<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let limit = buffer.len().min(7);
        let count = self.inner.read(&mut buffer[..limit])?;
        self.bytes.set(self.bytes.get() + count);
        Ok(count)
    }
}

#[test]
fn streaming_header_scratch_is_bounded_without_capping_body_strings() {
    for json in [
        format!("{{\"{}\":2}}", "x".repeat(10_000)),
        format!("{{\"\\u0076ersion\":\"{}\"}}", "x".repeat(10_000)),
    ] {
        let bytes = Cell::new(0);
        let reader = Fragmented {
            inner: Cursor::new(json.into_bytes()),
            bytes: &bytes,
        };
        assert_eq!(
            input_wire::inspect_reader(reader, 20_000).unwrap_err().code,
            ErrorCode::SchemaUnsupported
        );
        assert!(
            bytes.get() <= 512,
            "do not grow parser scratch to fit a corrupt header token"
        );
    }
    let json = format!(
        "{{\"version\":2,\"options\":{{\"padding\":\"{}\"}}}}",
        "汉\\\"{}[]".repeat(10_000)
    );
    let bytes = Cell::new(0);
    let reader = Fragmented {
        inner: Cursor::new(json.as_bytes().to_vec()),
        bytes: &bytes,
    };
    input_wire::inspect_reader(reader, json.len()).unwrap();
    input_wire::inspect(&json).unwrap();
    assert_eq!(bytes.get(), json.len());
    let deep = format!(
        "{{\"version\":2,\"options\":{}0{}}}",
        "[".repeat(129),
        "]".repeat(129)
    );
    assert!(input_wire::inspect_reader(deep.as_bytes(), deep.len()).is_err());
}

#[test]
fn retention_projection_reads_only_the_path_and_rejects_ambiguous_paths() {
    let conn = database();
    let path = std::env::temp_dir().join("synthetic-retention-reference");
    let raw_path = serde_json::to_string(&path).unwrap();
    let padding = "x".repeat(input_wire::ASSOCIATION_BYTES);
    let raw = format!("{{\"version\":3,\"identityAssociation\":null,\"previewPath\":{raw_path},\"adapter\":{{\"padding\":\"{padding}\"}}}}");
    conn.execute("UPDATE job_items SET result_json=?1", [raw.as_bytes()])
        .unwrap();
    assert_eq!(retained_preview_path(&conn, "j", "c").unwrap(), path);
    assert_eq!(
        load(&conn, "j", "c").unwrap_err().code,
        ErrorCode::ResourceLimit
    );
    for changed in [
        format!("{{\"previewPath\":{raw_path},\"previewPath\":{raw_path}}}"),
        "{\"previewPath\":\"relative\"}".into(),
        "{}".into(),
        "{\"previewPath\":null}".into(),
        format!("{{\"previewPath\":\"{}\"}}", "x".repeat(128 * 1024 * 6 + 1)),
    ] {
        conn.execute("UPDATE job_items SET result_json=?1", [changed.as_bytes()])
            .unwrap();
        assert_eq!(
            retained_preview_path(&conn, "j", "c").unwrap_err().code,
            ErrorCode::SnapshotUnavailable
        );
        assert!(conn.is_autocommit());
    }
}
