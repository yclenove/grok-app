mod portal_consent {
    use super::super::{
        computer_use_picker_targets_with_mode, uses_system_picker, validate_authorization_selection,
    };
    use grok_computer_use_core::{
        adapter::SurfaceKind,
        broker::{BrokerOptions, ComputerUseBroker},
    };

    #[test]
    fn app_portal_selection_requires_host_mode_and_exact_attempt_identity() {
        for surface in [
            SurfaceKind::Desktop,
            SurfaceKind::ManagedBrowser,
            SurfaceKind::ExistingTab,
            SurfaceKind::WebView,
        ] {
            assert!(!uses_system_picker(surface, "targets"));
            assert!(!uses_system_picker(surface, "unknown"));
            assert_eq!(
                uses_system_picker(surface, "portal"),
                surface == SurfaceKind::Desktop
            );
        }
        assert!(validate_authorization_selection("ui:1", 1, "", true).is_ok());
        assert!(validate_authorization_selection("ui:1", 1, "wayland:forged", true).is_err());
        assert!(validate_authorization_selection("", 1, "", true).is_err());
        assert!(validate_authorization_selection("ui:1", 0, "", true).is_err());
        assert!(validate_authorization_selection("ui:1", 1, "", false).is_err());
        assert!(validate_authorization_selection("ui:1", 1, "real-window", false).is_ok());
    }

    #[test]
    fn app_portal_discovery_is_empty_before_consent_but_keeps_exact_run_checks() {
        use crate::computer_use::{sessions, test_support::CountingAdapter};
        let adapter = std::sync::Arc::new(CountingAdapter::default());
        let broker = ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                ..BrokerOptions::default()
            },
        );
        let chat = format!("portal-ui-{}", uuid::Uuid::new_v4());
        assert!(computer_use_picker_targets_with_mode(
            &broker,
            Some(&chat),
            None,
            Some("desktop"),
            "portal"
        )
        .unwrap()
        .is_empty());
        assert!(adapter.listed_runs().is_empty());
        assert!(computer_use_picker_targets_with_mode(
            &broker,
            None,
            None,
            Some("desktop"),
            "portal"
        )
        .is_err());
        assert!(computer_use_picker_targets_with_mode(
            &broker,
            Some(&chat),
            None,
            Some("unknown"),
            "portal"
        )
        .is_err());
        let ticket = sessions::begin(&broker, &chat, None, "portal-discovery", 1).unwrap();
        assert!(computer_use_picker_targets_with_mode(
            &broker,
            Some("other-chat"),
            Some(&ticket.run_id),
            Some("desktop"),
            "portal"
        )
        .is_err());
        let rows = computer_use_picker_targets_with_mode(
            &broker,
            Some(&chat),
            Some(&ticket.run_id),
            Some("desktop"),
            "portal",
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            adapter.listed_runs().as_slice(),
            std::slice::from_ref(&ticket.run_id)
        );
        assert!(broker.authorized_target(&ticket.run_id).is_err());
        sessions::forget_session(&chat);
    }
}

mod target_readback {
    use grok_computer_use_core::{
        adapter::{
            AdapterActResult, Capabilities, ComputerUseAdapter, DispatchRequest, SurfaceKind,
            TargetInfo,
        },
        broker::{BrokerOptions, ComputerUseBroker},
        protocol::Observation,
    };
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    struct OwnedTargets {
        surface: SurfaceKind,
        ambient_reads: AtomicUsize,
        scoped_reads: parking_lot::Mutex<Vec<String>>,
        missing: AtomicBool,
        unavailable: AtomicBool,
        alive: AtomicBool,
        duplicated: AtomicBool,
        on_read: parking_lot::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl OwnedTargets {
        fn new(surface: SurfaceKind) -> Self {
            Self {
                surface,
                ambient_reads: AtomicUsize::new(0),
                scoped_reads: Default::default(),
                missing: AtomicBool::new(false),
                unavailable: AtomicBool::new(false),
                alive: AtomicBool::new(true),
                duplicated: AtomicBool::new(false),
                on_read: Default::default(),
            }
        }

        fn row(&self, run: &str) -> TargetInfo {
            let (prefix, kind) = match self.surface {
                SurfaceKind::Desktop => ("wayland:", "monitor"),
                SurfaceKind::WebView => ("wv:", "webview"),
                _ => unreachable!("this App fixture covers native and WebView owned targets"),
            };
            TargetInfo {
                target_id: format!("{prefix}{run}"),
                title: format!("Owned {run}"),
                app_name: format!("Owner {run}"),
                kind: kind.into(),
                pid: None,
                backend: self.backend_id().into(),
                execution_mode: "foreground-session".into(),
                replay_policy: "never".into(),
                lifecycle_stamp: 1,
                display_id: format!("display-{run}"),
                coordinate_space: "image-pixels".into(),
                scope_label: format!("Private {run}"),
            }
        }

        fn before_reply(&self) {
            let callback = self.on_read.lock().take();
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    impl ComputerUseAdapter for OwnedTargets {
        fn backend_id(&self) -> &'static str {
            "app-owned-target-test"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::for_surface(self.surface, self.backend_id())
        }
        fn list_targets(&self) -> Result<Vec<TargetInfo>, String> {
            self.ambient_reads.fetch_add(1, Ordering::SeqCst);
            self.before_reply();
            Err("ambient enumeration is not a run-owned grant".into())
        }
        fn list_targets_for_run(&self, run: &str) -> Result<Vec<TargetInfo>, String> {
            self.scoped_reads.lock().push(run.into());
            self.before_reply();
            if self.unavailable.load(Ordering::SeqCst) {
                return Err("exact owned enumeration failed".into());
            }
            Ok(if self.missing.load(Ordering::SeqCst) {
                vec![]
            } else if self.duplicated.load(Ordering::SeqCst) {
                vec![self.row(run), self.row(run)]
            } else {
                vec![self.row(run)]
            })
        }
        fn target_alive(&self, _: &str) -> bool {
            self.alive.load(Ordering::SeqCst)
        }
        fn observe(&self, _: &str) -> Result<Observation, String> {
            Err("not a capture test".into())
        }
        fn act(&self, _: &DispatchRequest) -> Result<AdapterActResult, String> {
            Err("not an input test".into())
        }
        fn abort(&self, _: &str, _: u64) -> Result<(), String> {
            Ok(())
        }
        fn is_idle(&self, _: &str) -> bool {
            true
        }
        fn start_periodic_preview(&self, _: &str) {}
        fn stop_periodic_preview(&self) {}
        fn periodic_preview_active(&self) -> bool {
            false
        }
    }

    fn fixture(surface: SurfaceKind) -> (Arc<ComputerUseBroker>, Arc<OwnedTargets>) {
        let adapter = Arc::new(OwnedTargets::new(surface));
        let broker = Arc::new(ComputerUseBroker::new(
            adapter.clone(),
            BrokerOptions {
                feature_enabled: true,
                ..BrokerOptions::default()
            },
        ));
        if surface == SurfaceKind::WebView {
            broker
                .register_surface_adapter(surface, adapter.clone())
                .unwrap();
        }
        for run in ["left", "right"] {
            broker.open_run(&format!("chat-{run}"), run).unwrap();
            broker
                .authorize_on_surface(
                    &format!("chat-{run}"),
                    run,
                    surface,
                    &adapter.row(run).target_id,
                )
                .unwrap();
        }
        adapter.scoped_reads.lock().clear();
        (broker, adapter)
    }

    #[test]
    fn authorized_metadata_keeps_native_monitor_and_webview_run_ownership() {
        for surface in [SurfaceKind::Desktop, SurfaceKind::WebView] {
            let (broker, adapter) = fixture(surface);
            for run in ["left", "right", "left"] {
                let row = super::super::authorized_target_dto(&broker, run, surface).unwrap();
                let expected = adapter.row(run);
                assert_eq!(row.target_id, expected.target_id);
                assert_eq!(row.title, expected.title);
                assert_eq!(row.app_name, expected.app_name);
                assert_eq!(row.kind, expected.kind);
                assert_eq!(row.surface, surface.as_wire());
            }
            assert_eq!(adapter.ambient_reads.load(Ordering::SeqCst), 0);
            assert_eq!(*adapter.scoped_reads.lock(), ["left", "right", "left"]);
        }
    }

    #[test]
    fn missing_or_failed_owned_metadata_is_not_fabricated_as_a_window() {
        for surface in [SurfaceKind::Desktop, SurfaceKind::WebView] {
            let (broker, adapter) = fixture(surface);
            adapter.missing.store(true, Ordering::SeqCst);
            assert!(super::super::authorized_target_dto(&broker, "left", surface).is_err());
            adapter.missing.store(false, Ordering::SeqCst);
            adapter.unavailable.store(true, Ordering::SeqCst);
            let error = super::super::authorized_target_dto(&broker, "left", surface)
                .err()
                .unwrap();
            assert!(error.contains("exact owned enumeration failed"), "{error}");
            assert_eq!(adapter.ambient_reads.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn stop_during_owned_metadata_read_rejects_the_late_reply() {
        let (broker, adapter) = fixture(SurfaceKind::Desktop);
        let stop = broker.clone();
        *adapter.on_read.lock() = Some(Box::new(move || {
            stop.fence_stop("left").unwrap();
        }));
        assert!(
            super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop).is_err()
        );
        assert!(broker.authorized_target("right").is_ok());
    }

    #[test]
    fn same_target_reauthorization_during_read_rejects_the_old_generation() {
        let (broker, adapter) = fixture(SurfaceKind::Desktop);
        let change = broker.clone();
        let target = adapter.row("left").target_id;
        *adapter.on_read.lock() = Some(Box::new(move || {
            change
                .authorize_on_surface("chat-left", "left", SurfaceKind::Desktop, &target)
                .unwrap();
        }));
        let error = super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop)
            .err()
            .expect("stale target generation was returned");
        assert!(error.contains("identity mismatch"), "{error}");
    }

    #[test]
    fn surface_mismatch_is_rejected_before_another_backend_is_enumerated() {
        let (broker, adapter) = fixture(SurfaceKind::WebView);
        assert!(
            super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop).is_err()
        );
        assert!(adapter.scoped_reads.lock().is_empty());
        assert_eq!(adapter.ambient_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn feature_off_during_metadata_read_cannot_publish_authorization() {
        let (broker, adapter) = fixture(SurfaceKind::Desktop);
        let disable = broker.clone();
        *adapter.on_read.lock() = Some(Box::new(move || {
            disable.fence_feature_enabled(false);
        }));
        let error = super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop)
            .err()
            .unwrap();
        assert!(error.contains("disabled"), "{error}");
    }

    #[test]
    fn missing_stopped_and_disabled_runs_do_not_enumerate_the_adapter() {
        let (broker, adapter) = fixture(SurfaceKind::Desktop);
        assert!(
            super::super::authorized_target_dto(&broker, "missing", SurfaceKind::Desktop).is_err()
        );
        broker.fence_stop("left").unwrap();
        assert!(
            super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop).is_err()
        );
        broker.fence_feature_enabled(false);
        assert!(
            super::super::authorized_target_dto(&broker, "right", SurfaceKind::Desktop).is_err()
        );
        assert!(adapter.scoped_reads.lock().is_empty());
        assert_eq!(adapter.ambient_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn dead_owned_target_is_not_returned_from_stale_metadata() {
        let (broker, adapter) = fixture(SurfaceKind::Desktop);
        adapter.alive.store(false, Ordering::SeqCst);
        let error = super::super::authorized_target_dto(&broker, "left", SurfaceKind::Desktop)
            .err()
            .unwrap();
        assert!(error.contains("target is gone"), "{error}");
    }

    #[test]
    fn duplicate_owned_target_metadata_is_ambiguous_not_first_match_wins() {
        let (broker, adapter) = fixture(SurfaceKind::WebView);
        adapter.duplicated.store(true, Ordering::SeqCst);
        let error = super::super::authorized_target_dto(&broker, "left", SurfaceKind::WebView)
            .err()
            .unwrap();
        assert!(error.contains("ambiguousTargetMetadata"), "{error}");
    }
}
