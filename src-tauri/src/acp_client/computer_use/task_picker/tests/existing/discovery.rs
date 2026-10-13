//! Availability can change between readback and the original blocking discovery.
use super::*;

const DOCUMENT: &str = "0123456789abcdef0123456789abcdef";

fn unshare(f: &Fixture, session: &PairingSession, sequence: u64) {
    f.transport
        .broker
        .unoffer_connected_tab(
            &format!("chrome-extension://{EXTENSION_ID}"),
            &session.session_key,
            &PairingConnection {
                instance_id: session.instance_id.clone(),
                connection_nonce: session.connection_nonce.clone(),
                generation: session.generation,
            },
            sequence,
            "101",
            7,
        )
        .unwrap();
}

fn borrow(f: &Fixture) {
    let selector = f.transport.broker.tabs().list_shared_candidates()[0]
        .0
        .clone();
    f.transport
        .broker
        .tabs()
        .grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
}

#[tokio::test]
async fn existing_discovery_readiness_race_can_reload_the_same_empty_picker() {
    for race in ["unshare", "borrow"] {
        let f = Fixture::new_for("existing-tabs").await;
        let started = begin(&f);
        let session = pair(&f, &started.picker_id);
        offer(&f, &session, 1, "101", Some(DOCUMENT));
        assert_eq!(prep(&f, &started.picker_id)["ready"], true);
        let empty = f
            .client
            .load_picker_with(
                SESSION,
                AGENT,
                &f.key,
                &started.picker_id,
                |_, call| async {
                    // The preflight has already seen one usable Share. Retire it
                    // before the unchanged real metadata producer runs.
                    if race == "unshare" {
                        unshare(&f, &session, 2);
                    } else {
                        borrow(&f);
                    }
                    call.discover().map_err(|error| error.to_string())
                },
            )
            .await
            .unwrap();
        assert!(empty.candidates.is_empty(), "{race}");
        assert!(f.start(&started.picker_id, &[]).is_err());
        assert_eq!(prep(&f, &started.picker_id)["ready"], false);
        f.no_execution();

        // Rechecking while empty must not consume Pair/Share preparation or
        // secretly select a replacement. The same ID and full key survive.
        let waiting = f
            .client
            .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
            .await
            .unwrap();
        assert!(waiting.candidates.is_empty());
        assert_eq!(waiting.picker_id, started.picker_id);
        assert_eq!(waiting.key, started.key);
        if race == "unshare" {
            offer(&f, &session, 3, "101", Some(DOCUMENT));
        } else {
            f.transport
                .broker
                .tabs()
                .return_borrowed("foreign-run", "101")
                .unwrap();
        }
        assert_eq!(prep(&f, &started.picker_id)["ready"], true);
        let fresh = f
            .client
            .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
            .await
            .unwrap();
        assert_eq!(
            fresh.candidates.len(),
            1,
            "{race}: empty receipt froze discovery"
        );
        assert_eq!(fresh.picker_id, started.picker_id);
        assert_eq!(fresh.key, started.key);
        assert!(f
            .transport
            .broker
            .tabs()
            .pending_pairing_challenge()
            .is_none());
        assert!(f
            .transport
            .broker
            .tabs()
            .existing_target_info("101")
            .is_none());
        f.no_execution();
        f.finish().await;
    }
}

async fn empty_after_unshare() -> (Fixture, TaskPickerStarted, PairingSession) {
    let f = Fixture::new_for("existing-tabs").await;
    let started = begin(&f);
    let session = pair(&f, &started.picker_id);
    offer(&f, &session, 1, "101", Some(DOCUMENT));
    let empty = f
        .client
        .load_picker_with(
            SESSION,
            AGENT,
            &f.key,
            &started.picker_id,
            |_, call| async {
                unshare(&f, &session, 2);
                call.discover().map_err(|error| error.to_string())
            },
        )
        .await
        .unwrap();
    assert!(empty.candidates.is_empty());
    offer(&f, &session, 3, "101", Some(DOCUMENT));
    (f, started, session)
}

#[tokio::test]
async fn existing_empty_retry_retains_receipt_when_an_original_borrow_is_still_live() {
    let (f, started, _) = empty_after_unshare().await;
    let borrowed = f
        .transport
        .picker
        .lock()
        .as_ref()
        .unwrap()
        .ready
        .clone()
        .unwrap();
    assert!(f
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .await
        .is_err());
    assert!(borrowed.is_current());
    assert!(borrowed.views().unwrap().is_empty());
    assert!(f.transport.picker.lock().as_ref().unwrap().ready.is_some());
    assert_eq!(prep(&f, &started.picker_id)["ready"], true);
    drop(borrowed);
    let retry = f
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .await
        .unwrap();
    assert_eq!(retry.picker_id, started.picker_id);
    assert_eq!(retry.candidates.len(), 1);
    f.no_execution();
    f.finish().await;
}

#[tokio::test]
async fn existing_empty_retry_cancel_stop_or_replacement_fences_original_discovery() {
    for fence in ["cancel", "stop", "replace"] {
        let (f, started, _) = empty_after_unshare().await;
        let mut replacement = None;
        let result = f
            .client
            .load_picker_with(
                SESSION,
                AGENT,
                &f.key,
                &started.picker_id,
                |_, call| async {
                    assert!(call.is_current());
                    match fence {
                        "cancel" => {
                            f.client
                                .cancel_computer_use_task_picker(
                                    SESSION,
                                    AGENT,
                                    &f.key,
                                    &started.picker_id,
                                )
                                .unwrap();
                        }
                        "stop" => {
                            f.client
                                .decide_computer_use_task(
                                    SESSION,
                                    AGENT,
                                    &f.key,
                                    grok_computer_use_core::broker::HostTaskDecision::Stop,
                                )
                                .unwrap();
                        }
                        _ => replacement = Some(begin(&f)),
                    }
                    assert!(!call.is_current());
                    call.discover().map_err(|error| error.to_string())
                },
            )
            .await;
        assert!(result.is_err(), "{fence}");
        assert!(f
            .client
            .computer_use_task_picker_preparation(SESSION, AGENT, &f.key, &started.picker_id,)
            .is_err());
        if let Some(fresh) = replacement {
            assert_eq!(prep(&f, &fresh.picker_id)["ready"], true);
            assert!(!f
                .client
                .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id,)
                .unwrap());
        }
        f.no_execution();
        f.finish().await;
    }
}
