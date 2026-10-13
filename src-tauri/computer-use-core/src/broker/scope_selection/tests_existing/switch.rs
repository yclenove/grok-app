//! Original share/task receipts; choices and extension peers are synthetic.
use super::*;
use crate::task_consent::CommitOutcome;

mod action;
mod gate;
mod invalidation;
mod io;
mod observations;
mod races;
mod recovery;
mod recovery_fences;

const DOCUMENT: &str = "0123456789abcdef0123456789abcdef";

fn share(f: &Fixture, sequence: u64, tab: &str, title: &str) {
    share_document(f, sequence, tab, title, DOCUMENT);
}

fn share_document(f: &Fixture, sequence: u64, tab: &str, title: &str, document: &str) {
    f.broker
        .tabs()
        .offer_connected_tab_document(
            &connection(&f.pairing),
            sequence,
            SharedTabOffer {
                pairing_token: &f.pairing.session_key,
                origin: &f.origin,
                extension_id: Some(EXTENSION_ID),
                tab_id: tab,
                title,
                url: "https://fixture.invalid/",
                browser_id: "chromium",
                profile_id: "fixture",
                document_generation: 7,
                connection_generation: f.pairing.generation,
                focused: true,
            },
            Some(document),
        )
        .unwrap();
}

fn approve_group(f: &Fixture) -> CommitOutcome {
    share(f, 2, "102", "Second");
    share(f, 3, "103", "Unselected");
    let picker = f
        .broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap()
        .discover()
        .unwrap();
    let choices = picker
        .views()
        .unwrap()
        .into_iter()
        .filter(|row| row.title != "Unselected")
        .map(|row| row.candidate_id)
        .collect::<Vec<_>>();
    assert_eq!(choices.len(), 2);
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, picker.select(&choices).unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    let outcome = approval.commit().unwrap();
    assert_eq!(outcome.binding.target_id, "tab:101");
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert!(f.broker.tabs().existing_target_info("103").is_none());
    outcome
}

fn open(f: &Fixture, run: &str, tab: &str) -> Result<serde_json::Value, BrokerError> {
    f.broker.open_approved_task_target(
        &f.token,
        &json!({"runId":run,"targetId":format!("tab:{tab}")}),
    )
}

#[test]
fn existing_task_group_lists_inactive_approved_share_without_borrow_or_content() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let rows = f
        .broker
        .approved_task_targets(&f.token, &json!({"runId":outcome.run_id}))
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["targetId"], "tab:101");
    assert_eq!(rows[0]["active"], true);
    assert_eq!(rows[1]["targetId"], "tab:102");
    assert_eq!(rows[1]["active"], false);
    assert!(rows[1]["targetGeneration"].is_null());
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    assert_eq!(f.desktop.list_calls(), 0);
    f.no_io();
}

#[test]
fn existing_task_group_switches_original_a_b_a_and_preserves_all_user_shares() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let unselected = f.broker.tabs().existing_scope_identity("tab:103").unwrap();
    let old = f
        .broker
        .begin_model_call(&f.token, "computer_observe")
        .unwrap();
    let mut generation = f.broker.authorized_target(&outcome.run_id).unwrap().1;
    for (active, inactive) in [("102", "101"), ("101", "102"), ("102", "101")] {
        let result = open(&f, &outcome.run_id, active).unwrap();
        assert_eq!(result["targetId"], format!("tab:{active}"));
        assert!(result["targetGeneration"].as_u64().unwrap() > generation);
        generation = result["targetGeneration"].as_u64().unwrap();
        assert_eq!(
            f.broker.tabs().existing_target_info(active).unwrap().run_id,
            outcome.run_id
        );
        assert!(f.broker.tabs().existing_target_info(inactive).is_none());
        assert!(f.broker.tabs().existing_target_info("103").is_none());
        assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
        assert_eq!(f.broker.continuation_count("existing-app"), 1);
        assert!(f.broker.lease.holder().is_none());
        f.no_io();
    }
    assert!(f.broker.observe_for_permit(&old, false).is_err());
    f.broker.fence_task_consent("existing-app");
    for ticket in f.broker.take_cleanup_tickets() {
        assert_eq!(
            f.broker.finish_stop_cleanup(&ticket).unwrap(),
            StopState::Stopped
        );
    }
    for tab in ["101", "102", "103"] {
        assert!(f.broker.tabs().existing_target_info(tab).is_none());
    }
    for tab in ["101", "102"] {
        assert!(!f.broker.tabs().is_closed(tab));
    }
    assert_eq!(
        f.broker.tabs().existing_scope_identity("tab:103").unwrap(),
        unselected
    );
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 3);
    f.no_io();
}

#[test]
fn existing_task_group_unselected_and_foreign_borrow_never_retire_active_member() {
    let f = Fixture::new();
    let outcome = approve_group(&f);
    let before = f.broker.authorized_target(&outcome.run_id).unwrap();
    assert!(open(&f, &outcome.run_id, "103").is_err());
    let host = f.broker.tabs();
    let selector = host
        .list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with("102@"))
        .unwrap()
        .0;
    host.grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
    assert!(open(&f, &outcome.run_id, "102").is_err());
    assert_eq!(f.broker.authorized_target(&outcome.run_id).unwrap(), before);
    assert_eq!(
        host.existing_target_info("102").unwrap().run_id,
        "foreign-run"
    );
    assert_eq!(
        host.existing_target_info("101").unwrap().run_id,
        outcome.run_id
    );
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert!(!host.is_closed("102"));
    host.return_borrowed("foreign-run", "102").unwrap();
    f.no_io();
}
