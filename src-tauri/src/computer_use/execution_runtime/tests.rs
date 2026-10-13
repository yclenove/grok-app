//! Synthetic factory effects exercise the real Broker registry, without native
//! permissions, a product singleton, browser process, or human approval.
use super::*;
use grok_computer_use_core::{broker::BrokerOptions, fake::FakeAdapter};
use std::sync::atomic::{AtomicUsize, Ordering};

fn broker() -> ComputerUseBroker {
    ComputerUseBroker::request_endpoint(BrokerOptions {
        feature_enabled: true,
        lease_path: std::env::temp_dir().join(format!("cu-runtime-{}.lease", uuid::Uuid::new_v4())),
        ..BrokerOptions::default()
    })
}

#[test]
fn scope_metadata_preparation_only_registers_selected_descriptor_without_starting_desktop() {
    for selected in [
        SurfaceKind::Desktop,
        SurfaceKind::ExistingTab,
        SurfaceKind::WebView,
    ] {
        let broker = broker();
        let fake = Arc::new(FakeAdapter::new());
        let calls = AtomicUsize::new(0);
        prepare_scope_selection_with(
            &broker,
            selected,
            || {
                assert_eq!(selected, SurfaceKind::Desktop);
                calls.fetch_add(1, Ordering::SeqCst);
                fake.clone()
            },
            || {
                assert_eq!(selected, SurfaceKind::WebView);
                calls.fetch_add(1, Ordering::SeqCst);
                fake.clone()
            },
        )
        .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(selected != SurfaceKind::ExistingTab)
        );
        assert_eq!(fake.list_calls(), 0);
        assert_eq!(fake.observe_calls(), 0);
        assert!(fake.executions().is_empty());
        for surface in [
            SurfaceKind::Desktop,
            SurfaceKind::ExistingTab,
            SurfaceKind::ManagedBrowser,
            SurfaceKind::WebView,
        ] {
            assert_eq!(
                broker
                    .register_surface_adapter(surface, Arc::new(FakeAdapter::new()))
                    .is_err(),
                surface == selected
            );
        }
    }
    for selected in [SurfaceKind::ManagedBrowser] {
        let broker = broker();
        assert!(prepare_scope_selection_with(
            &broker,
            selected,
            || panic!("unsupported metadata started Desktop"),
            || panic!("unsupported metadata started WebView"),
        )
        .is_err());
        for surface in [
            SurfaceKind::Desktop,
            SurfaceKind::ExistingTab,
            SurfaceKind::ManagedBrowser,
            SurfaceKind::WebView,
        ] {
            broker
                .register_surface_adapter(surface, Arc::new(FakeAdapter::new()))
                .unwrap();
        }
    }
}

#[test]
fn preparation_installs_only_the_selected_executor_and_never_discovers_targets() {
    for selected in [
        SurfaceKind::Desktop,
        SurfaceKind::ManagedBrowser,
        SurfaceKind::ExistingTab,
        SurfaceKind::WebView,
    ] {
        let broker = broker();
        let fake = Arc::new(FakeAdapter::new());
        let calls = AtomicUsize::new(0);
        prepare_with(
            &broker,
            selected,
            || {
                assert_eq!(selected, SurfaceKind::Desktop);
                calls.fetch_add(1, Ordering::SeqCst);
                fake.clone()
            },
            || {
                assert_eq!(selected, SurfaceKind::WebView);
                calls.fetch_add(1, Ordering::SeqCst);
                fake.clone()
            },
            |broker| {
                assert_eq!(selected, SurfaceKind::ManagedBrowser);
                calls.fetch_add(1, Ordering::SeqCst);
                broker
                    .register_managed_browser_adapter()
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
        )
        .unwrap();
        assert_eq!(
            calls.load(Ordering::SeqCst),
            usize::from(selected != SurfaceKind::ExistingTab)
        );
        assert_eq!(fake.list_calls(), 0);
        assert_eq!(fake.observe_calls(), 0);
        assert!(fake.executions().is_empty());
        // A different executor conflicts only with the selected registry slot.
        // Unrelated surfaces were never installed as a side effect.
        for surface in [
            SurfaceKind::Desktop,
            SurfaceKind::ManagedBrowser,
            SurfaceKind::ExistingTab,
            SurfaceKind::WebView,
        ] {
            let replacement = Arc::new(FakeAdapter::new());
            assert_eq!(
                broker
                    .register_surface_adapter(surface, replacement)
                    .is_err(),
                surface == selected,
                "selected={selected:?} other={surface:?}"
            );
        }
    }
}

#[test]
fn failed_managed_startup_is_returned_without_registering_a_fallback() {
    let broker = broker();
    let result = prepare_with(
        &broker,
        SurfaceKind::ManagedBrowser,
        || panic!("managed failure started Desktop"),
        || panic!("managed failure started WebView"),
        |_| Err("synthetic private worker startup failure".into()),
    );
    assert_eq!(
        result.err().as_deref(),
        Some("synthetic private worker startup failure")
    );
    for surface in [
        SurfaceKind::Desktop,
        SurfaceKind::ManagedBrowser,
        SurfaceKind::ExistingTab,
        SurfaceKind::WebView,
    ] {
        broker
            .register_surface_adapter(surface, Arc::new(FakeAdapter::new()))
            .unwrap();
    }
}

#[test]
fn conflicting_selected_registration_aborts_instead_of_silently_succeeding() {
    let original = Arc::new(FakeAdapter::new());
    let broker = ComputerUseBroker::new(original.clone(), BrokerOptions::default());
    let replacement = Arc::new(FakeAdapter::new());
    let result = prepare_with(
        &broker,
        SurfaceKind::Desktop,
        || replacement.clone(),
        || panic!("Desktop failure started WebView"),
        |_| panic!("Desktop failure started managed worker"),
    );
    assert!(result.is_err());
    assert!(Arc::ptr_eq(
        &broker.adapter(),
        &(original.clone() as Arc<dyn ComputerUseAdapter>)
    ));
    assert_eq!(original.list_calls(), 0);
    assert_eq!(replacement.list_calls(), 0);
}
