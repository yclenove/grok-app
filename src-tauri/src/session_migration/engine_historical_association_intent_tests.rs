//! Negative mutations over genuine accepted routing snapshots; never grants.
use super::*;
use rusqlite::{params, Connection};

fn raw_meta(conn: &Connection) -> Vec<(i64, String, String)> {
    conn.prepare("SELECT rowid,key,hex(CAST(value AS BLOB)) FROM meta ORDER BY rowid")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
#[ignore = "requires explicit hash-pinned V243 synthetic seed and registered source fixture"]
fn historical_seed_association_acceptance_intent_snapshot_refusals_predecode() {
    let home = seed_home();
    let _runtime = RuntimeReset;
    let conn = open_catalog().unwrap();
    let plan = confirmed_plan();
    let queued = acceptance(&plan, "synthetic-original-intent").unwrap();
    let intent_key = format!("job_entry_intent:{}", queued.job_id);
    let scope_key = format!("job_entry_scope:{}", queued.job_id);
    let witness_key = format!("job_entry_scope_accepted:{}", queued.job_id);
    let snapshot_key = format!("job_entry_plan_snapshot:{}", queued.job_id);
    let capture = |key: &str| {
        conn.query_row("SELECT rowid,value FROM meta WHERE key=?1", [key], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
    };
    let intent = capture(&intent_key);
    let scope = capture(&scope_key);
    let witness = capture(&witness_key);
    let snapshot = capture(&snapshot_key);
    let before_meta = raw_meta(&conn);
    let original_job: String = conn
        .query_row(
            "SELECT json FROM jobs WHERE job_id=?1",
            [&queued.job_id],
            |row| row.get(0),
        )
        .unwrap();
    let (_, original_plan) = load_plan_json(&conn, &plan.plan_id).unwrap();
    let old_history = text(IMPORT, REVISION);
    let archive = jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap();

    for (fault, expected) in [
        ("missing", ErrorCode::HistoryAccessUnavailable),
        ("blob", ErrorCode::HistoryAccessUnavailable),
        ("utf8", ErrorCode::HistoryAccessUnavailable),
        ("checksum", ErrorCode::HistoryAccessUnavailable),
        ("oversized", ErrorCode::ResourceLimit),
        ("version", ErrorCode::HistoryAccessUnavailable),
        ("context", ErrorCode::PlanStale),
        ("plan", ErrorCode::PlanStale),
        ("omitted-association", ErrorCode::HistoryAccessUnavailable),
        (
            "coordinated-empty-labels",
            ErrorCode::HistoryAccessUnavailable,
        ),
        ("coordinated-plan-downgrade", ErrorCode::PlanStale),
        ("unknown-ordinary-flag", ErrorCode::HistoryAccessUnavailable),
    ] {
        match fault {
            "missing" => {
                conn.execute("DELETE FROM meta WHERE key=?1", [&intent_key])
                    .unwrap();
            }
            "blob" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(value AS BLOB) WHERE key=?1",
                    [&intent_key],
                )
                .unwrap();
            }
            "utf8" => {
                conn.execute(
                    "UPDATE meta SET value=CAST(x'ff' AS TEXT) WHERE key=?1",
                    [&intent_key],
                )
                .unwrap();
            }
            "checksum" => {
                conn.execute(
                    "UPDATE meta SET value=value || ' ' WHERE key=?1",
                    [&intent_key],
                )
                .unwrap();
            }
            "oversized" => {
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![" ".repeat(16 * 1024 * 1024 + 1), intent_key],
                )
                .unwrap();
            }
            _ => {
                let mut changed: serde_json::Value = serde_json::from_str(&intent.1).unwrap();
                match fault {
                    "version" => changed["version"] = json!(2),
                    "context" => changed["contextDigest"] = json!("0".repeat(64)),
                    "plan" => changed["planId"] = json!(Uuid::new_v4().to_string()),
                    "omitted-association"
                    | "coordinated-empty-labels"
                    | "coordinated-plan-downgrade" => changed["associations"] = json!([]),
                    "unknown-ordinary-flag" => changed["ordinary"] = json!(true),
                    _ => unreachable!(),
                }
                let raw = changed.to_string();
                let mut changed_scope: serde_json::Value = serde_json::from_str(&scope.1).unwrap();
                if matches!(
                    fault,
                    "coordinated-empty-labels" | "coordinated-plan-downgrade"
                ) {
                    changed_scope["references"] = json!([]);
                }
                if fault == "coordinated-plan-downgrade" {
                    let mut changed_plan: serde_json::Value =
                        serde_json::from_str(&snapshot.1).unwrap();
                    changed_plan["version"] = json!(1);
                    changed_plan
                        .as_object_mut()
                        .unwrap()
                        .remove("identityAssociations");
                    let raw_plan = changed_plan.to_string();
                    changed_scope["planJsonDigest"] =
                        json!(hash::sha256_bytes(raw_plan.as_bytes()));
                    conn.execute(
                        "UPDATE meta SET value=?1 WHERE key=?2",
                        params![raw_plan, snapshot_key],
                    )
                    .unwrap();
                }
                changed_scope["intentDigest"] = json!(hash::sha256_bytes(raw.as_bytes()));
                let raw_scope = changed_scope.to_string();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![raw, intent_key],
                )
                .unwrap();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![raw_scope, scope_key],
                )
                .unwrap();
                conn.execute(
                    "UPDATE meta SET value=?1 WHERE key=?2",
                    params![hash::sha256_bytes(raw_scope.as_bytes()), witness_key],
                )
                .unwrap();
            }
        }
        let damaged_meta = raw_meta(&conn);
        for reader in ["get", "page", "retry"] {
            jobs::take_job_item_deserializes();
            let result = match reader {
                "get" => job_get(&queued.job_id),
                "page" => job_items_page(&queued.job_id, None, Some(1)),
                "retry" => commit(&plan.plan_id, &plan.plan_hash, &queued.request_id),
                _ => unreachable!(),
            };
            assert_eq!(result.unwrap_err().code, expected, "{fault}/{reader}");
            assert_eq!(jobs::take_job_item_deserializes(), 0, "{fault}/{reader}");
        }
        assert_eq!(raw_meta(&conn), damaged_meta, "no read-time repair");
        for (key, original) in [
            (&intent_key, &intent),
            (&scope_key, &scope),
            (&witness_key, &witness),
            (&snapshot_key, &snapshot),
        ] {
            conn.execute(
                "INSERT OR REPLACE INTO meta(rowid,key,value) VALUES(?1,?2,?3)",
                params![original.0, key, original.1],
            )
            .unwrap();
        }
        assert_eq!(raw_meta(&conn), before_meta);
        assert_eq!(
            conn.query_row::<String, _, _>(
                "SELECT json FROM jobs WHERE job_id=?1",
                [&queued.job_id],
                |row| row.get(0)
            )
            .unwrap(),
            original_job
        );
        assert_eq!(
            load_plan_json(&conn, &plan.plan_id).unwrap().1,
            original_plan
        );
        assert_eq!(text(IMPORT, REVISION), old_history);
        assert_eq!(
            jobs::published_hashes(&home.path.join(ARCHIVE)).unwrap(),
            archive
        );
        assert_eq!(job_get(&queued.job_id).unwrap().job_id, queued.job_id);
    }
}
