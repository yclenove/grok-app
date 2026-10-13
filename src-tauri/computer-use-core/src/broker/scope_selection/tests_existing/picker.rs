//! Real shared registry and Host picker; user/agent choices are synthetic.
use super::*;
use crate::broker::{HostScopePicker, HostTaskDecision};
use crate::task_consent::HostRequestPhase;

const DOCUMENT: &str = "0123456789abcdef0123456789abcdef";

fn offer(f: &Fixture, sequence: u64, tab: &str, title: &str, document: Option<&str>) {
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
            document,
        )
        .unwrap();
}

fn picker(f: &Fixture) -> HostScopePicker {
    f.broker
        .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
        .unwrap()
        .discover()
        .unwrap()
}

fn ids(picker: &HostScopePicker) -> Vec<String> {
    picker
        .views()
        .unwrap()
        .iter()
        .map(|row| row.candidate_id.clone())
        .collect()
}

#[test]
fn existing_task_picker_shared_metadata_does_not_use_desktop_title_filters() {
    let f = Fixture::new();
    offer(&f, 2, "102", "", Some(DOCUMENT));
    offer(&f, 3, "103", "Grok", Some(DOCUMENT));
    offer(&f, 4, "104", &"x".repeat(1024), Some(DOCUMENT));
    offer(&f, 5, "105", "Legacy", None);
    offer(&f, 6, "106", "Occupied", Some(DOCUMENT));
    let host = f.broker.tabs();
    let selector = host
        .list_shared_candidates()
        .into_iter()
        .find(|(id, _, _)| id.starts_with("106@"))
        .unwrap()
        .0;
    host.grant_picker_tab("foreign", "foreign-run", &selector)
        .unwrap();
    let picker = picker(&f);
    let views = picker.views().unwrap();
    assert_eq!(views.len(), 4);
    assert!(views.iter().all(|row| row.surface == "existing-tabs"));
    assert!(views
        .iter()
        .any(|row| row.title.is_empty() && row.app_name == "chromium"));
    assert!(views.iter().any(|row| row.title == "Grok"));
    assert!(views.iter().any(|row| row.title == "x".repeat(512)));
    let wire = serde_json::to_string(&views).unwrap();
    for private in [
        &f.token,
        &f.pairing.session_key,
        &f.pairing.connection_nonce,
        &f.pairing.instance_id,
    ] {
        assert!(!wire.contains(private));
    }
    for private in [
        "tab:",
        "documentId",
        DOCUMENT,
        "https://fixture.invalid/",
        "identity",
        "profileId",
    ] {
        assert!(!wire.contains(private));
    }
    assert_eq!(f.desktop.list_calls(), 0);
    assert!(!picker.truncated());
    assert!(host.existing_target_info("101").is_none());
    assert_eq!(
        host.existing_target_info("106").unwrap().run_id,
        "foreign-run"
    );
    f.no_execution();
    host.return_borrowed("foreign-run", "106").unwrap();
}

#[test]
fn existing_task_picker_subset_commit_and_original_retry_never_rediscover_or_close_tab() {
    let f = Fixture::new();
    offer(&f, 2, "102", "Second", Some(DOCUMENT));
    let picker = picker(&f);
    let all_ids = ids(&picker);
    let chosen = vec![all_ids[0].clone()];
    let selection = picker.select(&chosen).unwrap();
    assert_eq!(selection.members().len(), 1);
    assert_eq!(selection.target_id(), "tab:101");
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, selection)
        .unwrap()
    else {
        panic!("prepare")
    };
    f.no_execution();
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    assert!(f
        .broker
        .execution_run(&f.token, "computer_observe")
        .is_err());
    f.no_execution();
    let outcome = approval.commit().unwrap();
    assert_eq!(
        picker
            .committed_outcome(&f.token, &chosen)
            .unwrap()
            .unwrap()
            .run_id,
        outcome.run_id
    );
    assert!(picker.committed_outcome(&f.token, &all_ids).is_err());
    assert!(f.broker.tabs().existing_target_info("102").is_none());
    drop(picker);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert_eq!(f.broker.continuation_count("existing-app"), 1);
    assert_eq!(f.desktop.list_calls(), 0);
    f.no_io();
    f.broker.fence_task_consent("existing-app");
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert!(f.broker.tabs().existing_target_info("101").is_none());
    assert!(!f.broker.tabs().is_closed("101"));
    assert_eq!(f.broker.tabs().list_shared_candidates().len(), 2);
    assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
    f.no_io();
}

#[test]
fn existing_task_picker_original_proof_rejects_reshare_document_unshare_and_expiry() {
    for mode in ["reshare", "document", "unshare", "expiry", "connection"] {
        let mut f = Fixture::new();
        let picker = picker(&f);
        let selected = picker.select(&ids(&picker)).unwrap();
        let host = f.broker.tabs();
        match mode {
            "reshare" => reoffer(&f, 2, Some(DOCUMENT)).unwrap(),
            "document" => reoffer(&f, 2, Some("fedcba9876543210fedcba9876543210")).unwrap(),
            "unshare" => {
                host.unoffer_connected_tab(
                    &f.origin,
                    &f.pairing.session_key,
                    &connection(&f.pairing),
                    2,
                    "101",
                    7,
                )
                .unwrap();
            }
            _ => {
                if mode == "expiry" {
                    host.expire_pairing_connection_for_test();
                    assert!(host.expire_pairing_connection());
                }
                let challenge = host.begin_pairing_challenge();
                host.confirm_pairing_app_for(&challenge.nonce).unwrap();
                f.pairing = host
                    .complete_pairing_request(&ExtensionPairing::proof_for(
                        &challenge,
                        "00000000-0000-4000-8000-000000000002",
                    ))
                    .unwrap();
                reoffer(&f, 1, Some(DOCUMENT)).unwrap();
            }
        }
        assert!(
            f.broker
                .reserve_selected_access_approval(&f.token, selected)
                .is_err(),
            "{mode}"
        );
        assert!(host.existing_target_info("101").is_none());
        f.no_execution();
    }
}

#[test]
fn existing_task_picker_new_receipt_and_late_old_drop_do_not_share_candidate_authority() {
    let f = Fixture::new();
    let old = picker(&f);
    let old_ids = ids(&old);
    let fresh = picker(&f);
    assert!(old.views().is_err());
    assert!(old.select(&old_ids).is_err());
    assert!(fresh.select(&old_ids).is_err());
    old.cancel_handle().fence();
    drop(old);
    assert!(fresh.select(&ids(&fresh)).is_ok());
    f.no_execution();
}

#[test]
fn existing_task_picker_empty_duplicate_foreign_and_oversize_choices_have_no_effects() {
    let f = Fixture::new();
    let picker = picker(&f);
    let ids = ids(&picker);
    for selection in [
        vec![],
        vec![ids[0].clone(); 2],
        vec!["tab:101".into()],
        vec!["foreign".into()],
        vec![ids[0].clone(); 9],
    ] {
        assert!(picker.select(&selection).is_err());
    }
    assert!(picker.select(&ids).is_ok());
    f.no_execution();
}

#[test]
fn existing_task_picker_cancel_drop_and_prepared_drop_preserve_user_share() {
    for mode in ["call", "discovered", "selected", "prepared"] {
        let f = Fixture::new();
        let call = f
            .broker
            .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
            .unwrap();
        assert_eq!(
            f.broker
                .host_task_view(&f.token)
                .unwrap()
                .request
                .unwrap()
                .phase,
            HostRequestPhase::Preparing
        );
        if mode == "call" {
            call.cancel_handle().fence();
            assert!(call.discover().is_err());
        } else {
            let picker = call.discover().unwrap();
            if mode == "discovered" {
                drop(picker);
            } else {
                let selected = picker.select(&ids(&picker)).unwrap();
                if mode == "selected" {
                    drop(picker);
                    assert!(f
                        .broker
                        .reserve_selected_access_approval(&f.token, selected)
                        .is_err());
                } else {
                    let AccessApprovalReady::Prepare(mut approval) = f
                        .broker
                        .reserve_selected_access_approval(&f.token, selected)
                        .unwrap()
                    else {
                        panic!("prepare")
                    };
                    approval.publish_feature().unwrap();
                    approval.prepare_execution().unwrap();
                    assert!(
                        f.broker
                            .tabs()
                            .existing_target_info("101")
                            .unwrap()
                            .borrowed
                    );
                    drop(picker);
                    assert!(approval.commit().is_err());
                    for ticket in f.broker.take_cleanup_tickets() {
                        f.broker.finish_stop_cleanup(&ticket).unwrap();
                    }
                    assert!(!f.broker.tabs().is_closed("101"));
                }
            }
        }
        assert_eq!(
            f.broker
                .host_task_view(&f.token)
                .unwrap()
                .request
                .unwrap()
                .phase,
            HostRequestPhase::Pending
        );
        assert!(!f.broker.feature_enabled());
        assert!(f.broker.tabs().existing_target_info("101").is_none());
        assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
        f.no_execution();
    }
}

#[test]
fn existing_task_picker_stop_at_original_call_discovery_and_preparation_never_publishes_late_grant()
{
    for mode in ["call", "discovered", "prepared"] {
        let f = Fixture::new();
        let call = f
            .broker
            .begin_task_scope_picker(&f.token, &f.key, SurfaceKind::ExistingTab)
            .unwrap();
        if mode == "call" {
            f.broker
                .decide_host_task(&f.token, &f.key, HostTaskDecision::Stop)
                .unwrap();
            assert!(call.discover().is_err());
        } else {
            let picker = call.discover().unwrap();
            if mode == "discovered" {
                let ids = ids(&picker);
                f.broker
                    .decide_host_task(&f.token, &f.key, HostTaskDecision::Stop)
                    .unwrap();
                assert!(picker.select(&ids).is_err());
            } else {
                let AccessApprovalReady::Prepare(mut approval) = f
                    .broker
                    .reserve_selected_access_approval(
                        &f.token,
                        picker.select(&ids(&picker)).unwrap(),
                    )
                    .unwrap()
                else {
                    panic!("prepare")
                };
                approval.publish_feature().unwrap();
                approval.prepare_execution().unwrap();
                f.broker
                    .decide_host_task(&f.token, &f.key, HostTaskDecision::Stop)
                    .unwrap();
                assert!(approval.commit().is_err());
                for ticket in f.broker.take_cleanup_tickets() {
                    f.broker.finish_stop_cleanup(&ticket).unwrap();
                }
                assert!(!f.broker.tabs().is_closed("101"));
            }
        }
        assert_eq!(f.broker.executable_grant_count("existing-app"), 0);
        assert!(f
            .broker
            .execution_run(&f.token, "computer_observe")
            .is_err());
        assert!(f.broker.tabs().existing_target_info("101").is_none());
        assert_eq!(f.broker.tabs().list_shared_candidates().len(), 1);
        f.no_io();
    }
}

#[test]
fn existing_task_picker_foreign_borrow_after_discovery_is_not_adopted_or_released() {
    let f = Fixture::new();
    let picker = picker(&f);
    let host = f.broker.tabs();
    host.grant_picker_tab(
        "foreign",
        "foreign-run",
        &host.list_shared_candidates()[0].0,
    )
    .unwrap();
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, picker.select(&ids(&picker)).unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    assert!(approval.prepare_execution().is_err());
    approval.abort();
    for ticket in f.broker.take_cleanup_tickets() {
        f.broker.finish_stop_cleanup(&ticket).unwrap();
    }
    assert_eq!(
        host.existing_target_info("101").unwrap().run_id,
        "foreign-run"
    );
    assert!(!host.is_closed("101"));
    host.return_borrowed("foreign-run", "101").unwrap();
    f.no_execution();
}

#[test]
fn existing_task_picker_requires_full_original_key_requested_surface_and_live_transport() {
    let f = Fixture::new();
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
            revision: 2,
            ..f.key.clone()
        },
    ] {
        assert!(f
            .broker
            .begin_task_scope_picker(&f.token, &key, SurfaceKind::ExistingTab)
            .is_err());
    }
    for surface in [
        SurfaceKind::Desktop,
        SurfaceKind::ManagedBrowser,
        SurfaceKind::WebView,
    ] {
        assert!(f
            .broker
            .begin_task_scope_picker(&f.token, &f.key, surface)
            .is_err());
    }
    let picker = picker(&f);
    let ids = ids(&picker);
    f.broker.retire_transport_connection(&f.token);
    assert!(picker.views().is_err());
    assert!(picker.select(&ids).is_err());
    assert_eq!(f.desktop.list_calls(), 0);
    f.no_execution();
}

#[test]
fn existing_task_picker_blank_title_can_prepare_without_desktop_metadata_or_content() {
    let f = Fixture::new();
    offer(&f, 2, "101", "", Some(DOCUMENT));
    let picker = picker(&f);
    let views = picker.views().unwrap();
    assert_eq!(views.len(), 1);
    assert!(views[0].title.is_empty());
    assert_eq!(views[0].app_name, "chromium");
    let AccessApprovalReady::Prepare(mut approval) = f
        .broker
        .reserve_selected_access_approval(&f.token, picker.select(&ids(&picker)).unwrap())
        .unwrap()
    else {
        panic!("prepare")
    };
    approval.publish_feature().unwrap();
    approval.prepare_execution().unwrap();
    f.no_execution();
    approval.commit().unwrap();
    assert_eq!(f.broker.executable_grant_count("existing-app"), 1);
    assert_eq!(f.desktop.list_calls(), 0);
    f.no_io();
}
