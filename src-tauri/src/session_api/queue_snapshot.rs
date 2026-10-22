//! Read-only, bounded queue projection. Never dispatches or releases a claim.
use super::{queue_dispatch, queue_store::Item};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueSnapshot {
    version: u8,
    items: Vec<QueueSnapshotItem>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QueueSnapshotItem {
    session_id: String,
    item_id: String,
    prompt: String,
    created_at: i64,
    state: &'static str,
}
impl From<Item> for QueueSnapshotItem {
    fn from(item: Item) -> Self {
        let state = item.display_state();
        Self {
            session_id: item.session_id,
            item_id: item.item_id,
            prompt: item.prompt,
            created_at: item.created_at,
            state,
        }
    }
}
async fn snapshot_at(root: &Path) -> Result<QueueSnapshot, String> {
    let items = queue_dispatch::list_at(root)
        .await
        .map_err(|_| "SESSION_QUEUE_UNAVAILABLE".to_string())?;
    if items.iter().any(|item| item.session_id == "__draft__") {
        return Err("SESSION_QUEUE_UNAVAILABLE".into());
    }
    Ok(QueueSnapshot {
        version: 1,
        items: items.into_iter().map(Into::into).collect(),
    })
}
/// Desktop IPC and authenticated mirror share this exact read-only projection.
#[tauri::command]
pub(crate) async fn session_api_queue_snapshot() -> Result<QueueSnapshot, String> {
    snapshot_at(&crate::paths::app_data_root()).await
}
#[cfg(test)]
mod tests {
    use super::super::{
        queue_regression_tests::{item, Fixture},
        queue_store, TurnResult, TurnStatus,
    };
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn missing_queue_is_empty_without_creating_queue_data() {
        let f = Fixture::new();
        let snapshot = snapshot_at(&f.0).await.unwrap();
        assert_eq!(snapshot.version, 1);
        assert!(snapshot.items.is_empty());
        assert!(!f.path().exists());
    }
    #[tokio::test]
    async fn restores_all_200_items_in_durable_order_without_rewriting() {
        let f = Fixture::new();
        let items = (0..200)
            .map(|i| {
                Item::new(
                    if i % 2 == 0 { "a" } else { "b" },
                    &format!("q-{i}"),
                    &format!("prompt {i}"),
                    200 - i,
                )
            })
            .collect::<Vec<_>>();
        let before = serde_json::to_vec(&serde_json::json!({"version":2,"items":items})).unwrap();
        fs::write(f.path(), &before).unwrap();
        let snapshot = snapshot_at(&f.0).await.unwrap();
        assert_eq!(snapshot.items.len(), 200);
        for (i, row) in snapshot.items.iter().enumerate() {
            assert_eq!(row.item_id, format!("q-{i}"));
            assert_eq!(row.state, "queued");
            assert_eq!(row.prompt, format!("prompt {i}"));
        }
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
    #[tokio::test]
    async fn pending_and_held_are_visible_without_exposing_claim_secrets() {
        let f = Fixture::new();
        let q = item("q");
        queue_store::insert(&f.path(), q.clone()).unwrap();
        let nonce = queue_store::claim(&f.path(), &q).unwrap().unwrap();
        let before = fs::read(f.path()).unwrap();
        let pending = snapshot_at(&f.0).await.unwrap();
        assert_eq!(pending.items[0].state, "pending");
        let json = serde_json::to_string(&pending).unwrap();
        assert!(!json.contains(&nonce));
        assert!(!json.contains("nonce"));
        assert!(!json.contains("detail"));
        assert_eq!(fs::read(f.path()).unwrap(), before);
        let result = TurnResult::fail(TurnStatus::Unknown, "s", "unconfirmed");
        assert!(!queue_store::finish(&f.path(), &q, &nonce, &result).unwrap());
        assert_eq!(snapshot_at(&f.0).await.unwrap().items[0].state, "held");
        assert!(queue_store::claim(&f.path(), &q).unwrap().is_none());
    }
    #[tokio::test]
    async fn corrupt_future_or_unrepresentable_store_is_not_a_fake_empty_snapshot() {
        let f = Fixture::new();
        for bytes in [
            b"{broken".to_vec(),
            br#"{"version":99,"items":[]}"#.to_vec(),
            serde_json::to_vec(
                &serde_json::json!({"version":2,"items":[Item::new("__draft__","q","p",1)]}),
            )
            .unwrap(),
        ] {
            fs::write(f.path(), &bytes).unwrap();
            assert_eq!(
                snapshot_at(&f.0).await.unwrap_err(),
                "SESSION_QUEUE_UNAVAILABLE"
            );
            assert_eq!(fs::read(f.path()).unwrap(), bytes);
        }
    }
    #[tokio::test]
    async fn confirmed_take_and_handoff_are_projected_from_disk_not_event_history() {
        let f = Fixture::new();
        let first = item("first");
        let tail = item("tail");
        queue_store::insert(&f.path(), first.clone()).unwrap();
        queue_store::insert(&f.path(), tail).unwrap();
        let nonce = queue_store::claim(&f.path(), &first).unwrap().unwrap();
        queue_store::insert(&f.path(), item("replacement")).unwrap();
        let handoff = TurnResult {
            ok: true,
            status: TurnStatus::Queued,
            session_id: "s".into(),
            idempotency_key: None,
            message: None,
            queue_item_id: Some("replacement".into()),
            queued_prompt: Some("prompt".into()),
        };
        assert!(queue_store::finish(&f.path(), &first, &nonce, &handoff).unwrap());
        let rows = snapshot_at(&f.0).await.unwrap().items;
        assert_eq!(
            rows.iter().map(|r| r.item_id.as_str()).collect::<Vec<_>>(),
            vec!["replacement", "tail"]
        );
        let replacement = queue_store::list(&f.path()).unwrap().remove(0);
        let nonce = queue_store::claim(&f.path(), &replacement)
            .unwrap()
            .unwrap();
        let started = TurnResult {
            ok: true,
            status: TurnStatus::TurnStarted,
            session_id: "s".into(),
            idempotency_key: None,
            message: None,
            queue_item_id: None,
            queued_prompt: None,
        };
        assert!(queue_store::finish(&f.path(), &replacement, &nonce, &started).unwrap());
        assert_eq!(snapshot_at(&f.0).await.unwrap().items[0].item_id, "tail");
    }
}
