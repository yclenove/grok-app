//! Real import worker and native journal/meta writers, with isolated usage roots.
use super::*;
use crate::session_migration::engine;
use crate::store;
use std::io::Write;
use std::sync::Barrier;
use std::time::{Duration, Instant};

fn message(turn: usize, role: &str, text: &str) -> store::ChatMessageStored {
    store::ChatMessageStored {
        id: format!("{role}-{turn}"),
        role: role.into(),
        content: text.into(),
        thought: None,
        created_at: chrono::Utc::now(),
        is_error: false,
        attachments: None,
        marker: None,
    }
}

#[test]
fn import_worker_and_native_journal_meta_usage_writers_make_overlapping_progress() {
    let home = Home::new();
    let viewed = store::create_session(None, Some("Viewed native".into()), false).unwrap();
    let mut native = store::create_session(None, Some("Background native".into()), false).unwrap();
    let original_viewed = serde_json::to_value(&viewed).unwrap();
    let old_message = message(0, "user", "已打开的旧消息");
    store::append_message(&viewed.id, old_message.clone()).unwrap();
    let usage_root = home.path.join("synthetic-agent-sessions");
    let usage_session = usage_root.join("synthetic-project").join(&native.id);
    std::fs::create_dir_all(&usage_session).unwrap();
    let mut usage_file = std::fs::File::create(usage_session.join("updates.jsonl")).unwrap();
    let count = 64;
    let inputs = (0..count)
        .map(|i| {
            let mut item = input_named(&format!("foreign-{i}"), "Imported transcript only");
            item.adapter.inventory.source_tokens = 1_000_000;
            crate::session_migration::jobs::prepare_test_commit_item(item)
        })
        .collect();
    let queued = enqueue(job(), inputs).unwrap();
    let gate = Barrier::new(2);
    let turns = 16;
    let mut observed = Vec::new();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            gate.wait();
            run_pending()
        });
        gate.wait();
        let mut previous = 0;
        for turn in 0..turns {
            let deadline = Instant::now() + Duration::from_secs(30);
            // Observe actual committed import progress between native writes.
            // A barrier or a fixed sleep alone would not prove this overlap.
            let done = loop {
                let status = engine::job_get(&queued.job_id).unwrap();
                let done = status.progress.unwrap().done;
                if done > previous {
                    break done;
                }
                assert!(
                    !worker.is_finished(),
                    "worker stopped before overlapping progress"
                );
                assert!(
                    Instant::now() < deadline,
                    "import progress stalled during native writes"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            assert!(
                done < count,
                "worker finished before native overlap was exercised"
            );
            previous = done;
            observed.push(done);

            let user = message(turn, "user", &format!("问题 {turn}"));
            let mut answer = message(turn, "assistant", "正在输出");
            store::append_message(&native.id, user.clone()).unwrap();
            store::append_message(&native.id, answer.clone()).unwrap();
            answer.content = format!("回答 {turn}");
            answer.created_at = chrono::Utc::now();
            // Journal reconciliation runs through the production merge/lock path.
            store::save_messages(&native.id, &[user, answer]).unwrap();
            native.title = format!("Background turn {turn}");
            native.updated_at = chrono::Utc::now();
            store::update_runtime_session_meta(&native).unwrap();

            writeln!(
                usage_file,
                "{}",
                serde_json::json!({
                    "params": { "update": { "sessionUpdate": "turn_completed",
                        "usage": { "inputTokens": 100, "outputTokens": 25, "totalTokens": 125 } } }
                })
            )
            .unwrap();
            usage_file.flush().unwrap();
            store::write_json(&usage_session.join("signals.json"), &serde_json::json!({
                "turnCount": turn + 1, "contextTokensUsed": 7, "primaryModelId": "synthetic-native"
            })).unwrap();
            let (heatmap, logs) =
                crate::account::local_usage_from_roots(7, 100, std::slice::from_ref(&usage_root));
            assert_eq!(
                heatmap.iter().map(|day| day.tokens).sum::<u64>(),
                (turn as u64 + 1) * 125
            );
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].id, native.id);
            assert_eq!(logs[0].turns, turn as u64 + 1);
            assert_eq!(logs[0].context_tokens, 7);
            assert_eq!(store::load_messages(&native.id).len(), (turn + 1) * 2);
        }
        assert_eq!(worker.join().unwrap().unwrap(), 1);
    });
    assert_eq!(observed.len(), turns);
    assert!(observed.windows(2).all(|pair| pair[1] > pair[0]));

    let conn = open_catalog().unwrap();
    let imported = get_job(&conn, &queued.job_id).unwrap().unwrap();
    assert_eq!(imported.state, JobState::Completed);
    assert_eq!(list_entries(&conn).unwrap().len(), count as usize);
    for item in &imported.items {
        assert_eq!(item.phase, "committed");
        let inventory = revision_inventory(
            &conn,
            item.import_id.as_deref().unwrap(),
            item.revision_id.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(inventory.source_tokens, 1_000_000);
    }
    let rows = store::load_sessions_index();
    let ids: std::collections::BTreeSet<_> = rows.iter().map(|row| row.id.clone()).collect();
    assert_eq!(
        ids,
        [viewed.id.clone(), native.id.clone()].into_iter().collect()
    );
    let viewed_after = rows.iter().find(|row| row.id == viewed.id).unwrap();
    assert_eq!(serde_json::to_value(viewed_after).unwrap(), original_viewed);
    assert_eq!(
        serde_json::to_value(store::load_messages(&viewed.id)).unwrap(),
        serde_json::to_value([old_message]).unwrap()
    );
    assert_eq!(
        rows.iter().find(|row| row.id == native.id).unwrap().title,
        "Background turn 15"
    );
    let messages = store::load_messages(&native.id);
    assert_eq!(messages.len(), turns * 2);
    for turn in 0..turns {
        assert_eq!(messages[turn * 2].id, format!("user-{turn}"));
        assert_eq!(messages[turn * 2].content, format!("问题 {turn}"));
        assert_eq!(messages[turn * 2 + 1].id, format!("assistant-{turn}"));
        assert_eq!(messages[turn * 2 + 1].content, format!("回答 {turn}"));
    }
    let (heatmap, logs) = crate::account::local_usage_from_roots(7, 100, &[usage_root]);
    assert_eq!(heatmap.iter().map(|day| day.tokens).sum::<u64>(), 2000);
    assert_eq!(heatmap.iter().map(|day| day.requests).sum::<u64>(), 1);
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].id, native.id);
    assert_eq!(logs[0].usage_tokens, 2000);
    println!("NATIVE_IMPORT_COEXISTENCE imports={count} nativeTurns={turns} nativeTokens=2000 committedAtNativeWrites={observed:?}");
}
