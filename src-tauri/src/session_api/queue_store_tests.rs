use super::{
    queue_regression_tests::{item, Fixture},
    queue_store as store, TurnResult, TurnStatus,
};
use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
};
#[test]
fn legacy_valid_queue_is_read_without_rewriting_then_claimed_durably() {
    let f = Fixture::new();
    let q = item("q");
    let raw = serde_json::to_vec(&serde_json::json!({"items":[q]})).unwrap();
    fs::write(f.path(), &raw).unwrap();
    assert_eq!(store::list(&f.path()).unwrap(), vec![q.clone()]);
    assert_eq!(fs::read(f.path()).unwrap(), raw);
    assert!(store::claim(&f.path(), &q).unwrap().is_some());
    let value: serde_json::Value = serde_json::from_slice(&fs::read(f.path()).unwrap()).unwrap();
    assert_eq!(value["version"], 4);
    assert_eq!(value["outcomes"], serde_json::json!([]));
    assert_eq!(value["items"][0]["delivery"]["state"], "pending");
}
#[test]
fn malformed_and_future_queues_never_become_empty_successes() {
    let f = Fixture::new();
    let valid = serde_json::to_value(item("q")).unwrap();
    let mut invalids = vec![
        serde_json::json!({}),
        serde_json::json!({"version":99,"items":[]}),
        serde_json::json!({"version":1,"items":[]}),
        serde_json::json!({"items":[],"future":1}),
        serde_json::json!({"items":[valid.clone(),valid.clone()]}),
    ];
    for (field, value) in [
        ("sessionId", serde_json::json!("")),
        ("itemId", serde_json::json!(" ")),
        ("prompt", serde_json::json!(" ")),
        ("future", serde_json::json!(true)),
        (
            "sessionId",
            serde_json::json!("imp:83761701-94e0-4ae4-904a-4917ca98ddcb"),
        ),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        invalids.push(serde_json::json!({"items":[bad]}));
    }
    let mut bad = valid;
    bad["delivery"] =
        serde_json::json!({"nonce":"not-a-uuid","state":"pending","detail":"pending"});
    invalids.push(serde_json::json!({"version":2,"items":[bad]}));
    for value in invalids {
        let before = serde_json::to_vec(&value).unwrap();
        fs::write(f.path(), &before).unwrap();
        assert!(store::list(&f.path()).is_err(), "{value}");
        assert!(store::insert(&f.path(), item("new")).is_err(), "{value}");
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}
#[test]
fn invalid_request_does_not_poison_a_valid_queue() {
    let f = Fixture::new();
    store::insert(&f.path(), item("q")).unwrap();
    let before = fs::read(f.path()).unwrap();
    for bad in [
        super::queue_store::Item::new("", "bad", "p", 1),
        super::queue_store::Item::new("s", "", "p", 1),
        super::queue_store::Item::new("s", "bad", " ", 1),
    ] {
        assert!(store::insert(&f.path(), bad.clone()).is_err());
        assert!(store::claim(&f.path(), &bad).is_err());
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}
#[test]
fn same_id_cannot_overwrite_another_owner_or_prompt() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let before = fs::read(f.path()).unwrap();
    store::insert(&f.path(), q.clone()).unwrap();
    assert_eq!(fs::read(f.path()).unwrap(), before);
    for changed in [
        super::queue_store::Item::new("other", "q", "prompt", 1),
        super::queue_store::Item::new("s", "q", "other", 1),
    ] {
        assert!(store::insert(&f.path(), changed).is_err());
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}
#[test]
fn fifo_and_session_isolation_hold_across_claims() {
    let f = Fixture::new();
    let a = item("a");
    let b = item("b");
    let c = super::queue_store::Item::new("other", "c", "p", 2);
    for q in [&a, &b, &c] {
        store::insert(&f.path(), q.clone()).unwrap();
    }
    assert!(store::claim(&f.path(), &b).unwrap().is_none());
    let nonce = store::claim(&f.path(), &a).unwrap().unwrap();
    assert!(store::claim(&f.path(), &b).unwrap().is_none());
    assert!(store::claim(&f.path(), &c).unwrap().is_some());
    assert!(store::finish(&f.path(), &a, &nonce, &TurnResult::started("s")).unwrap());
    assert!(store::claim(&f.path(), &b).unwrap().is_some());
}
#[test]
fn stale_nonce_or_changed_payload_cannot_acknowledge() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
    let before = fs::read(f.path()).unwrap();
    assert!(store::finish(&f.path(), &q, "stale", &TurnResult::started("s")).is_err());
    let mut changed = q.clone();
    changed.prompt = "other".into();
    assert!(store::finish(&f.path(), &changed, &nonce, &TurnResult::started("s")).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), before);
}
#[test]
fn wrong_session_or_invalid_success_is_held_not_deleted() {
    for result in [
        TurnResult::started("other"),
        TurnResult {
            ok: false,
            ..TurnResult::started("s")
        },
    ] {
        let f = Fixture::new();
        let q = item("q");
        store::insert(&f.path(), q.clone()).unwrap();
        let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
        assert!(!store::finish(&f.path(), &q, &nonce, &result).unwrap());
        assert_eq!(store::list(&f.path()).unwrap().len(), 1);
        assert!(store::claim(&f.path(), &q).unwrap().is_none());
    }
}
#[test]
fn failed_or_uncertain_delivery_retains_exact_payload() {
    for status in [
        TurnStatus::Unknown,
        TurnStatus::Error,
        TurnStatus::NotFound,
        TurnStatus::RetryLater,
        TurnStatus::Busy,
    ] {
        let f = Fixture::new();
        let q = item("q");
        store::insert(&f.path(), q.clone()).unwrap();
        let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
        assert!(!store::finish(
            &f.path(),
            &q,
            &nonce,
            &TurnResult::fail(status, "s", "not confirmed")
        )
        .unwrap());
        let kept = store::list(&f.path()).unwrap().remove(0);
        assert_eq!(kept.prompt, q.prompt);
        assert_eq!(kept.item_id, q.item_id);
        assert!(store::claim(&f.path(), &q).unwrap().is_none());
    }
}
#[test]
fn queued_ack_requires_matching_persisted_replacement() {
    for persisted in [false, true] {
        let f = Fixture::new();
        let q = item("q");
        store::insert(&f.path(), q.clone()).unwrap();
        let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
        if persisted {
            store::insert(&f.path(), item("replacement")).unwrap();
        }
        let result = TurnResult {
            status: TurnStatus::Queued,
            queue_item_id: Some("replacement".into()),
            queued_prompt: Some("prompt".into()),
            ..TurnResult::started("s")
        };
        assert_eq!(
            store::finish(&f.path(), &q, &nonce, &result).unwrap(),
            persisted
        );
        let items = store::list(&f.path()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].item_id,
            if persisted { "replacement" } else { "q" }
        );
    }
}
#[test]
fn concurrent_producers_do_not_lose_accepted_items() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(24));
    let threads: Vec<_> = (0..24)
        .map(|i| {
            let path = f.path();
            let b = barrier.clone();
            thread::spawn(move || {
                b.wait();
                store::insert(&path, item(&format!("q-{i}"))).unwrap();
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(store::list(&f.path()).unwrap().len(), 24);
}
#[test]
fn concurrent_claimers_get_exactly_one_nonce() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let threads: Vec<_> = (0..24)
        .map(|_| {
            let path = f.path();
            let q = q.clone();
            thread::spawn(move || store::claim(&path, &q).unwrap())
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .filter_map(|t| t.join().unwrap())
            .count(),
        1
    );
    assert_eq!(store::list(&f.path()).unwrap().len(), 1);
}
#[test]
fn successful_ack_is_not_reclaimable_from_a_stale_snapshot() {
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
    assert!(store::finish(&f.path(), &q, &nonce, &TurnResult::started("s")).unwrap());
    assert!(store::list(&f.path()).unwrap().is_empty());
    assert!(store::claim(&f.path(), &q).unwrap().is_none());
}

#[cfg(windows)]
#[test]
fn readonly_queue_write_failure_preserves_claim_and_original_bytes() {
    struct Restore(std::path::PathBuf, fs::Permissions);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, self.1.clone());
        }
    }
    let f = Fixture::new();
    let q = item("q");
    store::insert(&f.path(), q.clone()).unwrap();
    let nonce = store::claim(&f.path(), &q).unwrap().unwrap();
    let before = fs::read(f.path()).unwrap();
    let original = fs::metadata(f.path()).unwrap().permissions();
    let restore = Restore(f.path(), original.clone());
    let mut readonly = original;
    readonly.set_readonly(true);
    fs::set_permissions(f.path(), readonly).unwrap();
    assert!(store::insert(&f.path(), item("new")).is_err());
    assert!(store::finish(&f.path(), &q, &nonce, &TurnResult::started("s")).is_err());
    assert_eq!(fs::read(f.path()).unwrap(), before);
    assert_eq!(store::list(&f.path()).unwrap().len(), 1);
    assert!(store::claim(&f.path(), &q).unwrap().is_none());
    drop(restore);
}
#[test]
fn stale_temp_file_is_not_overwritten_or_promoted() {
    let f = Fixture::new();
    let stale = f.path().with_extension("tmp");
    fs::write(&stale, b"unowned stale bytes").unwrap();
    store::insert(&f.path(), item("q")).unwrap();
    assert_eq!(fs::read(stale).unwrap(), b"unowned stale bytes");
    assert_eq!(store::list(&f.path()).unwrap().len(), 1);
}
#[test]
fn directory_instead_of_queue_fails_closed_without_removing_contents() {
    let f = Fixture::new();
    fs::create_dir(f.path()).unwrap();
    let sentinel = f.path().join("preserve");
    fs::write(&sentinel, b"opaque").unwrap();
    assert!(store::list(&f.path()).is_err());
    assert!(store::insert(&f.path(), item("q")).is_err());
    assert!(store::claim(&f.path(), &item("q")).is_err());
    assert_eq!(fs::read(sentinel).unwrap(), b"opaque");
}

#[test]
fn busy_handoff_keeps_the_original_fifo_position() {
    let f = Fixture::new();
    let first = item("first");
    let next = item("next");
    let replacement = item("replacement");
    for q in [&first, &next] {
        store::insert(&f.path(), q.clone()).unwrap();
    }
    let nonce = store::claim(&f.path(), &first).unwrap().unwrap();
    store::insert(&f.path(), replacement.clone()).unwrap();
    let queued = TurnResult {
        status: TurnStatus::Queued,
        queue_item_id: Some("replacement".into()),
        queued_prompt: Some("prompt".into()),
        ..TurnResult::started("s")
    };
    assert!(store::finish(&f.path(), &first, &nonce, &queued).unwrap());
    let items = store::list(&f.path()).unwrap();
    assert_eq!(
        items.iter().map(|q| q.item_id.as_str()).collect::<Vec<_>>(),
        vec!["replacement", "next"]
    );
    assert!(store::claim(&f.path(), &next).unwrap().is_none());
    assert!(store::claim(&f.path(), &replacement).unwrap().is_some());
}

#[test]
fn busy_handoff_preserves_other_sessions_and_younger_prompt_order() {
    let f = Fixture::new();
    let before = super::queue_store::Item::new("other", "other-head", "other", 1);
    let first = item("first");
    let other_next = super::queue_store::Item::new("other", "other-next", "other", 2);
    let next = item("next");
    let replacement = item("replacement");
    for q in [&before, &first, &other_next, &next] {
        store::insert(&f.path(), q.clone()).unwrap();
    }
    let nonce = store::claim(&f.path(), &first).unwrap().unwrap();
    store::insert(&f.path(), replacement.clone()).unwrap();
    let queued = TurnResult {
        status: TurnStatus::Queued,
        queue_item_id: Some("replacement".into()),
        queued_prompt: Some("prompt".into()),
        ..TurnResult::started("s")
    };
    assert!(store::finish(&f.path(), &first, &nonce, &queued).unwrap());
    let items = store::list(&f.path()).unwrap();
    assert_eq!(
        items.iter().map(|q| q.item_id.as_str()).collect::<Vec<_>>(),
        vec!["other-head", "replacement", "other-next", "next"]
    );
    assert!(store::claim(&f.path(), &next).unwrap().is_none());
    assert!(store::claim(&f.path(), &before).unwrap().is_some());
    assert!(store::claim(&f.path(), &replacement).unwrap().is_some());
}
