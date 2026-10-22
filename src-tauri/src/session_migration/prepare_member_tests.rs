use super::*;
use crate::session_migration::prepare;

#[test]
fn prepared_member_uses_exact_batch_version_candidate_and_preview() {
    let fixture = Fixture::new(2);
    let batch = start_batch(&fixture);
    let status = wait_terminal(&batch.batch_id);
    let page = prepare::items_page(&batch.batch_id, None, Some(2)).unwrap();
    let first = &page.items[0];
    let preview = first.preview_id.as_deref().unwrap();
    let full = prepare::validated_plan_inputs(&batch.batch_id).unwrap();
    let member = prepare::validated_member(
        &batch.batch_id,
        status.state_version,
        &first.candidate_id,
        preview,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&member.selection).unwrap(),
        serde_json::to_value(
            full.selection_set
                .entries
                .iter()
                .find(|entry| entry.preview_id == preview)
                .unwrap()
        )
        .unwrap()
    );
    for (version, candidate, preview) in [
        (
            status.state_version + 1,
            first.candidate_id.as_str(),
            preview,
        ),
        (
            status.state_version,
            page.items[1].candidate_id.as_str(),
            preview,
        ),
        (
            status.state_version,
            first.candidate_id.as_str(),
            page.items[1].preview_id.as_deref().unwrap(),
        ),
    ] {
        assert!(prepare::validated_member(&batch.batch_id, version, candidate, preview).is_err());
    }
    let other = start_batch(&fixture);
    let other_status = wait_terminal(&other.batch_id);
    assert!(prepare::validated_member(
        &other.batch_id,
        other_status.state_version,
        &first.candidate_id,
        preview
    )
    .is_err());
}

#[test]
fn prepared_member_and_plan_share_fail_closed_selection_checks() {
    let fixture = Fixture::new(1);
    let batch = start_batch(&fixture);
    let status = wait_terminal(&batch.batch_id);
    let item = prepare::items_page(&batch.batch_id, None, Some(1))
        .unwrap()
        .items
        .remove(0);
    let conn = prepare::open_existing(&batch.batch_id).unwrap();
    let original: String = conn
        .query_row("SELECT selection_json FROM items WHERE seq=1", [], |row| {
            row.get(0)
        })
        .unwrap();
    let mut changed: serde_json::Value = serde_json::from_str(&original).unwrap();
    changed["previewId"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    conn.execute(
        "UPDATE items SET selection_json=?1 WHERE seq=1",
        [changed.to_string()],
    )
    .unwrap();
    let before = conn.total_changes();
    assert!(prepare::validated_member(
        &batch.batch_id,
        status.state_version,
        &item.candidate_id,
        item.preview_id.as_deref().unwrap()
    )
    .is_err());
    assert!(prepare::validated_plan_inputs(&batch.batch_id).is_err());
    assert_eq!(conn.total_changes(), before);
    conn.execute("UPDATE items SET selection_json=?1 WHERE seq=1", [original])
        .unwrap();
    assert!(prepare::validated_member(
        &batch.batch_id,
        status.state_version,
        &item.candidate_id,
        item.preview_id.as_deref().unwrap()
    )
    .is_ok());
}
