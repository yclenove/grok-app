//! Real ACP/Core metadata entry; Pair, Share and user choices are synthetic.
use super::*;
use grok_computer_use_core::{
    browser::SharedTabOffer,
    pairing::{ExtensionPairing, PairingConnection, PairingSession, EXTENSION_ID},
};
mod discovery;

fn prep(f: &Fixture, id: &str) -> Value {
    serde_json::to_value(
        f.client
            .computer_use_task_picker_preparation(SESSION, AGENT, &f.key, id)
            .unwrap(),
    )
    .unwrap()
}

fn begin(f: &Fixture) -> TaskPickerStarted {
    f.client
        .begin_computer_use_task_picker(SESSION, AGENT, &f.key, "existing-tabs", PROCESS)
        .unwrap()
}

fn confirm(f: &Fixture, id: &str, nonce: &str) -> Result<(), String> {
    f.client
        .confirm_computer_use_task_picker_pairing(SESSION, AGENT, &f.key, id, nonce)
}

fn pair(f: &Fixture, id: &str) -> PairingSession {
    let ch = f
        .transport
        .broker
        .tabs()
        .pending_pairing_challenge()
        .unwrap();
    assert!(confirm(f, id, "foreign-nonce").is_err());
    assert_eq!(prep(f, id)["pairingConfirmed"], false);
    confirm(f, id, &ch.nonce).unwrap();
    assert_eq!(prep(f, id)["pairingConfirmed"], true);
    confirm(f, id, &ch.nonce).unwrap();
    f.transport
        .broker
        .complete_pairing(&ExtensionPairing::proof_for(
            &ch,
            "00000000-0000-4000-8000-000000000001",
        ))
        .unwrap()
}

fn offer(f: &Fixture, session: &PairingSession, sequence: u64, id: &str, doc: Option<&str>) {
    f.transport
        .broker
        .offer_connected_tab_document(
            &PairingConnection {
                instance_id: session.instance_id.clone(),
                connection_nonce: session.connection_nonce.clone(),
                generation: session.generation,
            },
            sequence,
            SharedTabOffer {
                pairing_token: &session.session_key,
                origin: &format!("chrome-extension://{EXTENSION_ID}"),
                extension_id: Some(EXTENSION_ID),
                tab_id: id,
                title: "Synthetic shared fixture",
                url: "https://fixture.invalid/",
                browser_id: "chromium",
                profile_id: "synthetic",
                document_generation: 7,
                connection_generation: session.generation,
                focused: true,
            },
            doc,
        )
        .unwrap();
}

#[tokio::test]
async fn existing_first_use_original_app_preparation_pair_share_and_receipt_are_one_owner() {
    let f = Fixture::new_for("existing-tabs").await;
    let started = begin(&f);
    let view = prep(&f, &started.picker_id);
    assert_eq!(view["surface"], "existing-tabs");
    assert_eq!(view["ready"], false);
    assert_eq!(view["pairingConfirmed"], false);
    assert_eq!(view["pairing"]["endpoint"], "http://127.0.0.1:9");
    assert!(!view.to_string().contains(&f.transport.token));
    assert!(!view.to_string().contains("native"));
    let session = pair(&f, &started.picker_id);
    let paired = prep(&f, &started.picker_id);
    assert!(paired["pairing"].is_null());
    assert_eq!(paired["ready"], false);
    assert!(f
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .await
        .is_err());
    assert!(f.transport.picker.lock().as_ref().unwrap().call.is_some());
    offer(
        &f,
        &session,
        1,
        "101",
        Some("0123456789abcdef0123456789abcdef"),
    );
    offer(&f, &session, 2, "102", None);
    let shared = prep(&f, &started.picker_id);
    assert_eq!(shared["availableTabs"], 1);
    assert_eq!(shared["incompatibleTabs"], 1);
    assert_eq!(shared["ready"], true);
    assert!(f
        .transport
        .broker
        .tabs()
        .existing_target_info("101")
        .is_none());
    let selected = f
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .await
        .unwrap();
    assert_eq!(selected.surface, "existing-tabs");
    assert_eq!(selected.candidates.len(), 1);
    assert_eq!(selected.candidates[0].surface, "existing-tabs");
    assert!(!serde_json::to_string(&selected)
        .unwrap()
        .contains("tab:101"));
    assert_eq!(f.fake.list_calls(), 0);
    f.no_execution();
    let start = f.start(&started.picker_id, &ids(&selected)).unwrap();
    let outcome = f
        .client
        .finish_picker_approval(SESSION, AGENT, start, reserve_synthetic, approve_synthetic)
        .await
        .unwrap();
    assert_eq!(outcome.binding.target_id, "tab:101");
    assert!(f
        .client
        .computer_use_task_picker_preparation(SESSION, AGENT, &f.key, &started.picker_id)
        .is_err());
    assert_eq!(f.transport.broker.executable_grant_count(SESSION), 1);
    assert_eq!(f.fake.list_calls(), 0);
    f.client
        .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .unwrap();
    assert_eq!(f.transport.broker.executable_grant_count(SESSION), 1);
    f.finish().await;
}

#[tokio::test]
async fn existing_first_use_foreign_key_picker_agent_and_session_cannot_confirm_or_read_code() {
    let f = Fixture::new_for("existing-tabs").await;
    let started = begin(&f);
    let ch = f
        .transport
        .broker
        .tabs()
        .pending_pairing_challenge()
        .unwrap();
    for key in [
        HostRequestKey {
            connection_id: "foreign".into(),
            ..f.key.clone()
        },
        HostRequestKey {
            task_id: "foreign".into(),
            ..f.key.clone()
        },
        HostRequestKey {
            request_id: "foreign".into(),
            ..f.key.clone()
        },
        HostRequestKey {
            revision: f.key.revision + 1,
            ..f.key.clone()
        },
    ] {
        assert!(f
            .client
            .computer_use_task_picker_preparation(SESSION, AGENT, &key, &started.picker_id)
            .is_err());
        assert!(f
            .client
            .confirm_computer_use_task_picker_pairing(
                SESSION,
                AGENT,
                &key,
                &started.picker_id,
                &ch.nonce
            )
            .is_err());
    }
    for (session, agent, id) in [
        ("foreign", AGENT, started.picker_id.as_str()),
        (SESSION, "foreign", started.picker_id.as_str()),
        (SESSION, AGENT, "foreign"),
    ] {
        assert!(f
            .client
            .computer_use_task_picker_preparation(session, agent, &f.key, id)
            .is_err());
        assert!(f
            .client
            .confirm_computer_use_task_picker_pairing(session, agent, &f.key, id, &ch.nonce)
            .is_err());
    }
    assert_eq!(prep(&f, &started.picker_id)["pairingConfirmed"], false);
    f.no_execution();
    f.finish().await;
}

#[tokio::test]
async fn existing_first_use_old_picker_drop_and_stop_cannot_restore_or_confirm_preparation() {
    for fence in ["cancel", "stop", "replace", "kill", "agent"] {
        let f = Fixture::new_for("existing-tabs").await;
        let old = begin(&f);
        let ch = f
            .transport
            .broker
            .tabs()
            .pending_pairing_challenge()
            .unwrap();
        let mut replacement = None;
        match fence {
            "cancel" => {
                f.client
                    .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &old.picker_id)
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
            "replace" => {
                replacement = Some(begin(&f));
            }
            "kill" => {
                f.client.kill().await;
            }
            _ => {
                f.transport
                    .broker
                    .bind_agent_session(SESSION, "replacement-agent")
                    .unwrap();
            }
        }
        assert!(
            f.client
                .computer_use_task_picker_preparation(SESSION, AGENT, &f.key, &old.picker_id)
                .is_err(),
            "{fence}"
        );
        assert!(confirm(&f, &old.picker_id, &ch.nonce).is_err(), "{fence}");
        if let Some(fresh) = replacement {
            assert!(!f
                .client
                .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &old.picker_id)
                .unwrap());
            assert_eq!(prep(&f, &fresh.picker_id)["pairingConfirmed"], false);
            pair(&f, &fresh.picker_id);
        }
        f.no_execution();
        f.finish().await;
    }
}

#[tokio::test]
async fn existing_first_use_borrowed_and_legacy_shares_wait_without_consuming_original_call() {
    let f = Fixture::new_for("existing-tabs").await;
    let started = begin(&f);
    let session = pair(&f, &started.picker_id);
    offer(
        &f,
        &session,
        1,
        "101",
        Some("0123456789abcdef0123456789abcdef"),
    );
    offer(&f, &session, 2, "102", None);
    let selector = f
        .transport
        .broker
        .tabs()
        .list_shared_candidates()
        .into_iter()
        .find(|(selector, _, _)| selector.starts_with("101@"))
        .unwrap()
        .0;
    f.transport
        .broker
        .tabs()
        .grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
    let waiting = prep(&f, &started.picker_id);
    assert_eq!(waiting["availableTabs"], 0);
    assert_eq!(waiting["borrowedTabs"], 1);
    assert_eq!(waiting["incompatibleTabs"], 1);
    assert!(f
        .client
        .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
        .await
        .is_err());
    assert!(f.transport.picker.lock().as_ref().unwrap().call.is_some());
    assert_eq!(
        f.transport
            .broker
            .tabs()
            .existing_target_info("101")
            .unwrap()
            .run_id,
        "foreign-run"
    );
    f.transport
        .broker
        .tabs()
        .return_borrowed("foreign-run", "101")
        .unwrap();
    assert_eq!(prep(&f, &started.picker_id)["ready"], true);
    assert_eq!(
        f.client
            .load_computer_use_task_picker(SESSION, AGENT, &f.key, &started.picker_id)
            .await
            .unwrap()
            .candidates
            .len(),
        1
    );
    f.no_execution();
    f.finish().await;
}

#[tokio::test]
async fn existing_first_use_reuses_completed_pair_without_global_revoke_or_new_challenge() {
    let f = Fixture::new_for("existing-tabs").await;
    let old = begin(&f);
    let session = pair(&f, &old.picker_id);
    offer(
        &f,
        &session,
        1,
        "101",
        Some("0123456789abcdef0123456789abcdef"),
    );
    let fresh = begin(&f);
    let view = prep(&f, &fresh.picker_id);
    assert!(view["pairing"].is_null());
    assert_eq!(view["availableTabs"], 1);
    assert!(f
        .transport
        .broker
        .tabs()
        .pending_pairing_challenge()
        .is_none());
    assert!(!f
        .client
        .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &old.picker_id)
        .unwrap());
    assert_eq!(prep(&f, &fresh.picker_id)["ready"], true);
    f.client
        .cancel_computer_use_task_picker(SESSION, AGENT, &f.key, &fresh.picker_id)
        .unwrap();
    assert_eq!(f.transport.broker.tabs().list_shared_candidates().len(), 1);
    f.no_execution();
    f.finish().await;
}
