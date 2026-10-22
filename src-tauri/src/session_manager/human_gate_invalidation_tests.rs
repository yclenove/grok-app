//! Reset/Stop/exit regressions use the real producer and prepared publisher.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn assert_not_published(update: HumanGateUpdate) {
    let published = AtomicBool::new(false);
    update.publish(|| published.store(true, Ordering::SeqCst));
    assert!(
        !published.load(Ordering::SeqCst),
        "an invalidated prepared plan was persisted and published"
    );
}

#[test]
fn stop_and_steer_revoke_delayed_plan_events_in_both_pools() {
    for background in [false, true] {
        for live_rpc in [false, true] {
            let fixture = Fixture::new();
            prepare_busy(&fixture);
            if background {
                fixture.move_focus(false);
            }
            let update = if live_rpc {
                prepare_plan_body(&fixture, "停止前尚未落盘的审批")
            } else {
                progress_plan(&fixture, "停止前尚未落盘的进度")
            };
            fixture.manager.with_session_mut(
                &fixture.target_meta.id,
                SessionManager::take_human_gates_for_cancel,
            );
            assert_not_published(update);
        }
    }
}

#[test]
fn failed_turn_reset_revokes_its_delayed_plan_event() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let update = prepare_plan_body(&fixture, "失败前的审批");
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                SessionManager::release_failed_turn_markers(session, None);
            });
        assert_not_published(update);
    }
}

#[test]
fn draining_runtime_slots_revokes_their_delayed_plan_events() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let update = prepare_plan_body(&fixture, "回收前的审批");
        let _drained = fixture.manager.drain_all_agent_slots();
        assert_not_published(update);
    }
}

#[test]
fn taking_dead_process_gates_revokes_delayed_plan_persistence() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let update = prepare_plan_body(&fixture, "退出前的审批");
        let row = fixture.manager.with_session_mut(
            &fixture.target_meta.id,
            SessionManager::take_pending_gate_invalidation,
        );
        assert!(row.flatten().is_some());
        assert_not_published(update);
    }
}

#[test]
fn stopped_progress_keeps_unwritten_content_and_can_close_with_its_receipt() {
    for background in [false, true] {
        let fixture = Fixture::new();
        prepare_busy(&fixture);
        if background {
            fixture.move_focus(false);
        }
        let old = progress_plan(&fixture, "停止时尚未写盘的中文草稿");
        let (_, _, invalidation) = fixture
            .manager
            .with_session_mut(
                &fixture.target_meta.id,
                SessionManager::take_human_gates_for_cancel,
            )
            .unwrap();
        let invalidation = invalidation.unwrap();
        invalidation.publish(|_| {});
        assert_not_published(old);
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert_eq!(chrome.body, "停止时尚未写盘的中文草稿");
        assert!(chrome.gate_stale && !chrome.waiting && chrome.rpc_id.is_none());
        let receipt = invalidation.row["planChromeRevision"]
            .as_str()
            .unwrap()
            .to_string();
        dismiss_chrome(&fixture.target_meta.id, Some(receipt)).unwrap();
        assert!(
            crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
                .unwrap()
                .user_closed
        );
    }
}

#[test]
fn a_delayed_reset_cannot_persist_or_publish_over_a_new_same_rpc_gate() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        prepare_plan_body(&fixture, "重置前的原审批").persist();
        let old = fixture
            .manager
            .with_session_mut(
                &fixture.target_meta.id,
                SessionManager::take_pending_gate_invalidation,
            )
            .flatten()
            .unwrap();
        prepare_plan_body(&fixture, "复用编号的新审批").persist();
        let published = AtomicBool::new(false);
        old.publish(|_| published.store(true, Ordering::SeqCst));
        assert!(!published.load(Ordering::SeqCst));
        let chrome = crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id).unwrap();
        assert_eq!(chrome.body, "复用编号的新审批");
        assert_eq!(chrome.rpc_id, Some(70));
        assert!(!chrome.gate_stale);
        let view = crate::plan_chrome::load_plan_chrome_view(&fixture.target_meta.id).unwrap();
        assert!(dismiss_chrome(&fixture.target_meta.id, view.chrome_revision).is_err());
    }
}

#[test]
fn failed_turn_reset_retires_live_rpc_and_retains_a_closable_draft() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "失败后的只读计划").persist();
    let previous = crate::plan_chrome::load_plan_chrome_view(&fixture.target_meta.id)
        .unwrap()
        .chrome_revision;
    let pending = fixture
        .manager
        .with_session_mut(&fixture.target_meta.id, |session| {
            SessionManager::release_failed_turn_markers(session, None)
        })
        .flatten()
        .unwrap();
    pending.publish(|_| {});
    assert!(dismiss_chrome(&fixture.target_meta.id, previous).is_err());
    let view = crate::plan_chrome::load_plan_chrome_view(&fixture.target_meta.id).unwrap();
    assert_eq!(view.chrome.body, "失败后的只读计划");
    assert!(view.chrome.rpc_id.is_none() && view.chrome.gate_stale);
    dismiss_chrome(&fixture.target_meta.id, view.chrome_revision).unwrap();
}

#[test]
fn real_shared_process_exit_preparation_cleans_live_and_background_gates() {
    for background in [false, true] {
        let fixture = Fixture::new();
        if background {
            fixture.move_focus(false);
        }
        let old = prepare_plan_body(&fixture, "退出进程未落盘的计划");
        let update = fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                SessionManager::prepare_process_exit_locked(session, Some(1))
            })
            .unwrap();
        assert_eq!(update.invalidations.len(), 1);
        for invalidation in update.invalidations {
            invalidation.publish(|_| {});
        }
        assert_not_published(old);
        assert!(fixture
            .manager
            .pending_plan(Some(fixture.target_meta.id.clone()))
            .is_none());
        fixture
            .manager
            .with_session_mut(&fixture.target_meta.id, |session| {
                assert!(session.acp.is_none());
                assert!(!SessionManager::live_session_is_busy(session));
                assert_eq!(session.fsm.state(), SessionState::Disconnected);
            })
            .unwrap();
        let view = crate::plan_chrome::load_plan_chrome_view(&fixture.target_meta.id).unwrap();
        assert_eq!(view.chrome.body, "退出进程未落盘的计划");
        dismiss_chrome(&fixture.target_meta.id, view.chrome_revision).unwrap();
    }
}

#[test]
fn invalidation_reservation_never_waits_for_a_plan_disk_transaction() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "正在写盘的审批").persist();
    let lock = crate::store_lock::lock_exclusive(
        &crate::paths::session_dir(&fixture.target_meta.id).join("plan_chrome.json"),
    )
    .unwrap();
    let manager = fixture.manager.clone();
    let id = fixture.target_meta.id.clone();
    let (finished, worker) = action(move || {
        manager.with_session_mut(&id, SessionManager::take_pending_gate_invalidation)
    });
    let result = finished.recv_timeout(Duration::from_secs(1));
    drop(lock);
    let pending = worker.join().unwrap().flatten().unwrap();
    assert!(
        result.is_ok(),
        "invalidation held the runtime while waiting for disk"
    );
    pending.publish(|_| {});
}

#[test]
fn invalidation_captures_both_displayed_and_unpublished_progress_revisions() {
    let fixture = Fixture::new();
    prepare_busy(&fixture);
    let displayed = progress_plan(&fixture, "已经显示的进度");
    let HumanGatePayload::Plan(displayed_plan) = &displayed.payload else {
        panic!("plan expected")
    };
    let displayed_revision = displayed_plan.chrome_revision.clone().unwrap();
    displayed.persist();
    let delayed = progress_plan(&fixture, "预约但未显示的进度");
    let HumanGatePayload::Plan(delayed_plan) = &delayed.payload else {
        panic!("plan expected")
    };
    let delayed_revision = delayed_plan.chrome_revision.clone().unwrap();
    let (_, _, pending) = fixture
        .manager
        .with_session_mut(
            &fixture.target_meta.id,
            SessionManager::take_human_gates_for_cancel,
        )
        .unwrap();
    let pending = pending.unwrap();
    assert_eq!(
        pending.row["publishedPlanChromeRevision"],
        displayed_revision
    );
    assert_eq!(pending.row["previousPlanChromeRevision"], delayed_revision);
    pending.publish(|_| {});
    assert_not_published(delayed);
    assert_eq!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .body,
        "预约但未显示的进度"
    );
}

#[test]
fn a_new_plan_does_not_hide_the_invalidation_of_an_independent_questionnaire() {
    let fixture = Fixture::new();
    prepare_plan_body(&fixture, "原计划").persist();
    receipt(&fixture, true).unwrap();
    let old = fixture
        .manager
        .with_session_mut(
            &fixture.target_meta.id,
            SessionManager::take_pending_gate_invalidation,
        )
        .flatten()
        .unwrap();
    prepare_plan_body(&fixture, "新计划").persist();
    let mut emitted = None;
    old.publish(|row| emitted = Some(row.clone()));
    let row = emitted.unwrap();
    assert_eq!(row["askUserRpcId"], 71);
    assert!(row["askUserRequestGeneration"].is_string());
    assert!(row["planRpcId"].is_null() && row["planChromeRevision"].is_null());
    assert_eq!(
        crate::plan_chrome::load_plan_chrome(&fixture.target_meta.id)
            .unwrap()
            .body,
        "新计划"
    );
}
