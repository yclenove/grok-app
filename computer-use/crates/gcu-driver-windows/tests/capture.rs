//! Portable capture and mandatory core redaction contracts.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "capture/affinity.rs"]
mod affinity;
mod common;
#[cfg(windows)]
#[path = "capture/native.rs"]
mod native;

use gcu_core::{
    driver::{Cancel, CaptureOpts, Driver, DriverError, TreeOpts, DRIVER_DEADLINE},
    model::{
        Fingerprint, Incomplete, IncompleteReason, NativeHandle, RawNode, RawTree, Rect, Role,
        States,
    },
    redact::{self, CapturedFrame, EncodeOptions, NEUTRAL_COLOR},
    tree::Pipeline,
};
use gcu_driver_windows::capture::{
    take_capture, CaptureBackend, CaptureFault, CapturePath, CaptureSource, FakeCapture, Paint,
    RawPixels, SourceWindow,
};
use std::{sync::Arc, time::Duration};

const OWN: [u8; 4] = [10, 40, 80, 255];
const COVER: [u8; 4] = [1, 2, 3, 255];
const SECRET: [u8; 4] = [255, 0, 180, 255];
const STAMP: Duration = Duration::from_secs(1);

fn sources() -> (Arc<FakeCapture>, Arc<FakeCapture>, Arc<CaptureBackend>) {
    let wgc = Arc::new(FakeCapture::new(CapturePath::Wgc));
    let print = Arc::new(FakeCapture::new(CapturePath::PrintWindow));
    for source in [&wgc, &print] {
        source.paint(
            8,
            Paint {
                own: OWN,
                cover: COVER,
                occluded: true,
                mark: None,
            },
        );
    }
    let backend = Arc::new(CaptureBackend::new(wgc.clone(), print.clone()));
    (wgc, print, backend)
}
fn source_window() -> SourceWindow {
    SourceWindow {
        hwnd: 8,
        frame: Rect {
            x: 12.,
            y: 18.,
            w: 150.,
            h: 75.,
        },
        scale: 1.5,
    }
}
fn decode(bytes: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels.truncate(info.buffer_size());
    pixels
}
fn raw_password(frame: Option<Rect>) -> RawTree {
    RawTree {
        root: Some(RawNode {
            role: Role::TextField,
            native_role: "Edit".into(),
            subrole: None,
            name: Some("Password".into()),
            value: Some(gcu_core::model::NodeValue::Text("secret".into())),
            description: None,
            placeholder: None,
            help: None,
            identifier: None,
            states: States::PASSWORD,
            actions: Default::default(),
            frame,
            text_extras: None,
            url: None,
            children: vec![],
            handle: NativeHandle::new(()),
            fingerprint: Fingerprint::default(),
        }),
        incomplete: None,
    }
}
fn processed(raw: RawTree, nodes: usize) -> gcu_core::tree::ProcessedTree {
    let mut tree = Pipeline::default().run(
        raw,
        &TreeOpts {
            max_tree_nodes: nodes,
            ..Default::default()
        },
    );
    tree.state_version = Some(42);
    tree.observed_at = Some(STAMP);
    tree
}
fn no_secret(pixels: &[u8]) {
    assert!(pixels
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p != SECRET && *p != COVER));
}

#[tokio::test]
async fn capture_occluded_150_percent_dpi_keeps_physical_geometry() {
    let (api, driver, _, win) = common::fixture().await;
    let (wgc, print, backend) = sources();
    let frame = source_window().frame;
    api.update(|s| {
        s.windows[0].frame = frame;
        s.windows[0].scale = 1.5;
    });
    let driver = driver.with_capture(backend);
    let capture = driver
        .capture(
            &win,
            &CaptureOpts {
                include_cursor: true,
                ..Default::default()
            },
            &Cancel::default(),
        )
        .await
        .unwrap();
    assert_eq!((capture.width, capture.height), (150, 75));
    assert_eq!(capture.geometry.frame, frame);
    assert_eq!(capture.geometry.scale, 1.5);
    assert_eq!(
        driver.window_geometry(&win).await.unwrap(),
        capture.geometry
    );
    assert!(decode(&capture.bytes)
        .as_chunks::<4>()
        .0
        .iter()
        .all(|p| *p == OWN));
    assert_eq!(
        driver.capture_stats().unwrap().last_path,
        Some(CapturePath::Wgc)
    );
    assert_eq!(wgc.stats().published, 1);
    assert_eq!(print.stats().published, 0);
    assert!(driver.capabilities().observe.screenshot);
    for action in common::actions(&win) {
        assert_eq!(
            driver.perform(&action, &Cancel::default()).await,
            Err(DriverError::CapabilityUnavailable)
        );
    }
}

#[tokio::test]
async fn capture_minimized_refuses_without_restoring() {
    let (api, driver, _, win) = common::fixture().await;
    let (_, _, backend) = sources();
    let driver = driver.with_capture(backend);
    api.update(|s| s.windows[0].minimized = true);
    let error = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert_eq!(error, DriverError::WindowMinimized);
    assert_eq!(driver.capture_stats().unwrap().wgc_attempts, 0);
    assert!(api.state().unwrap().restores.is_empty());
}

#[tokio::test]
async fn capture_failed_callbacks_return_no_pixels() {
    let (wgc, print, backend) = sources();
    wgc.set_fault(CaptureFault::CallbackFailed);
    print.set_fault(CaptureFault::CallbackFailed);
    let error = take_capture(
        &backend,
        &source_window(),
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DriverError::Native {
            maybe_executed: false,
            ..
        }
    ));
    assert!(!format!("{error:?}").contains("secret"));
    assert_eq!(backend.stats().published, 0);
    assert_eq!(backend.stats().discarded, 1);
    assert_eq!(backend.stats().fallbacks, 1);
}

#[tokio::test(start_paused = true)]
async fn capture_hung_callback_times_out_at_driver_deadline() {
    let (wgc, _, backend) = sources();
    wgc.set_fault(CaptureFault::Hang);
    let start = tokio::time::Instant::now();
    let error = take_capture(
        &backend,
        &source_window(),
        Duration::from_secs(20),
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert_eq!(start.elapsed(), DRIVER_DEADLINE);
    assert_eq!(backend.stats().deadline, Some(DRIVER_DEADLINE));
    assert_eq!(backend.stats().print_window_attempts, 0);
    assert_eq!(backend.stats().published, 0);
}

#[tokio::test(start_paused = true)]
async fn capture_cancelled_in_flight_drops_pixels_without_fallback() {
    let (wgc, _, backend) = sources();
    wgc.set_fault(CaptureFault::Hang);
    let cancel = Cancel::default();
    let later = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        later.cancel();
    });
    let error = take_capture(&backend, &source_window(), DRIVER_DEADLINE, &cancel)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert_eq!(backend.stats().published, 0);
    assert_eq!(backend.stats().discarded, 1);
    assert_eq!(backend.stats().print_window_attempts, 0);
    assert_eq!(wgc.stats().published, 0);
}

#[tokio::test]
async fn capture_missing_permission_or_protection_never_falls_back() {
    for fault in [
        CaptureFault::PermissionMissing,
        CaptureFault::Protected,
        CaptureFault::RevokeBeforePublish,
    ] {
        let (wgc, _, backend) = sources();
        wgc.set_fault(fault);
        let error = take_capture(
            &backend,
            &source_window(),
            DRIVER_DEADLINE,
            &Cancel::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, DriverError::PermissionMissing { .. }));
        assert!(!error.maybe_executed());
        assert_eq!(backend.stats().published, 0);
        assert_eq!(backend.stats().print_window_attempts, 0);
    }
}

#[tokio::test]
async fn capture_wgc_unavailable_or_failed_uses_print_window() {
    for fault in [
        CaptureFault::Unavailable,
        CaptureFault::CallbackFailed,
        CaptureFault::Black,
    ] {
        let (wgc, print, backend) = sources();
        wgc.set_fault(fault);
        let pixels = take_capture(
            &backend,
            &source_window(),
            DRIVER_DEADLINE,
            &Cancel::default(),
        )
        .await
        .unwrap();
        assert!(pixels.rgba.as_chunks::<4>().0.iter().all(|p| *p == OWN));
        assert_eq!(backend.stats().fallbacks, 1);
        assert_eq!(backend.stats().last_path, Some(CapturePath::PrintWindow));
        assert_eq!(print.stats().published, 1);
    }
}

#[tokio::test]
async fn capture_both_black_frames_refuses_success() {
    let (wgc, print, backend) = sources();
    wgc.set_fault(CaptureFault::Black);
    print.set_fault(CaptureFault::Black);
    let error = take_capture(
        &backend,
        &source_window(),
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        DriverError::Native {
            maybe_executed: false,
            ..
        }
    ));
    assert_eq!(backend.stats().published, 0);
}

#[tokio::test]
async fn capture_ready_frame_cancelled_before_publication_never_escapes() {
    let (wgc, _, backend) = sources();
    wgc.set_fault(CaptureFault::CancelBeforePublish);
    let error = take_capture(
        &backend,
        &source_window(),
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Cancelled {
            maybe_executed: false
        }
    );
    assert_eq!(backend.stats().published, 0);
    assert_eq!(backend.stats().discarded, 1);
    assert_eq!(backend.stats().print_window_attempts, 0);
}

#[tokio::test]
async fn capture_unattached_or_unsupported_sources_leave_capability_unavailable() {
    let (_, driver, _, win) = common::fixture().await;
    assert!(!driver.capabilities().observe.screenshot);
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
    let (wgc, print, backend) = sources();
    wgc.set_fault(CaptureFault::Unavailable);
    print.set_fault(CaptureFault::Unavailable);
    let driver = driver.with_capture(backend);
    assert!(!driver.capabilities().observe.screenshot);
    assert_eq!(driver.capabilities().limits.max_image_edge, 0);
    assert_eq!(
        driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err(),
        DriverError::CapabilityUnavailable
    );
}

struct DelayedSource(Arc<FakeCapture>);
#[async_trait::async_trait]
impl CaptureSource for DelayedSource {
    fn available(&self) -> bool {
        true
    }
    fn path(&self) -> CapturePath {
        CapturePath::Wgc
    }
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> gcu_core::driver::Result<RawPixels> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        self.0.capture(window, deadline, cancel).await
    }
}
#[tokio::test(start_paused = true)]
async fn capture_fallback_wait_uses_remaining_total_deadline() {
    let (wgc, print, _) = sources();
    wgc.set_fault(CaptureFault::CallbackFailed);
    print.set_fault(CaptureFault::Hang);
    let backend = CaptureBackend::new(Arc::new(DelayedSource(wgc)), print.clone());
    let start = tokio::time::Instant::now();
    let error = take_capture(
        &backend,
        &source_window(),
        DRIVER_DEADLINE,
        &Cancel::default(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        DriverError::Timeout {
            maybe_executed: false
        }
    );
    assert_eq!(start.elapsed(), DRIVER_DEADLINE);
    assert_eq!(
        print.stats().deadline,
        Some(DRIVER_DEADLINE - Duration::from_secs(1))
    );
    assert_eq!(backend.stats().fallbacks, 1);
    assert_eq!(backend.stats().published, 0);
}

struct GeometryChange(
    Arc<FakeCapture>,
    Arc<gcu_driver_windows::testing::FakeWinApi>,
);
#[async_trait::async_trait]
impl CaptureSource for GeometryChange {
    fn available(&self) -> bool {
        true
    }
    fn path(&self) -> CapturePath {
        CapturePath::Wgc
    }
    async fn capture(
        &self,
        window: &SourceWindow,
        deadline: Duration,
        cancel: &Cancel,
    ) -> gcu_core::driver::Result<RawPixels> {
        let pixels = self.0.capture(window, deadline, cancel).await?;
        self.1.update(|s| s.windows[0].frame.x += 1.);
        Ok(pixels)
    }
}
#[tokio::test]
async fn capture_window_moved_after_callback_discards_image() {
    let (api, driver, _, win) = common::fixture().await;
    let (wgc, print, _) = sources();
    let backend = Arc::new(CaptureBackend::new(
        Arc::new(GeometryChange(wgc, api)),
        print,
    ));
    let driver = driver.with_capture(backend);
    let error = driver
        .capture(&win, &CaptureOpts::default(), &Cancel::default())
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        DriverError::Native {
            maybe_executed: false,
            ..
        }
    ));
}

#[tokio::test]
async fn capture_both_paths_redact_password_before_derivatives_even_after_pruning() {
    for fallback in [false, true] {
        let (api, driver, _, win) = common::fixture().await;
        let (wgc, print, backend) = sources();
        let frame = source_window().frame;
        api.update(|s| {
            s.windows[0].frame = frame;
            s.windows[0].scale = 1.5;
        });
        let secret = Rect {
            x: 20.,
            y: 16.,
            w: 30.,
            h: 12.,
        };
        for source in [&wgc, &print] {
            source.paint(
                8,
                Paint {
                    own: OWN,
                    cover: COVER,
                    occluded: true,
                    mark: Some((SECRET, secret)),
                },
            );
        }
        if fallback {
            wgc.set_fault(CaptureFault::CallbackFailed);
        }
        let driver = driver.with_capture(backend);
        for nodes in [1200, 0] {
            let capture = driver
                .capture(&win, &CaptureOpts::default(), &Cancel::default())
                .await
                .unwrap();
            assert!(decode(&capture.bytes).as_chunks::<4>().0.contains(&SECRET));
            let tree = processed(raw_password(Some(secret)), nodes);
            assert_eq!(tree.sensitive_regions.len(), 1);
            let image = redact::apply(&tree, CapturedFrame::new(capture, 42, STAMP)).unwrap();
            assert_eq!(image.scale_to_window(), 1.);
            no_secret(image.pixels());
            assert_eq!(
                &image.pixels()[(16 * 150 + 20) * 4..(16 * 150 + 20) * 4 + 4],
                &NEUTRAL_COLOR
            );
            no_secret(image.thumbnail(32).unwrap().pixels());
            no_secret(image.approval_crop(secret).unwrap().pixels());
            no_secret(&decode(
                image.encode(EncodeOptions::default()).unwrap().bytes(),
            ));
        }
        let stats = driver.capture_stats().unwrap();
        assert_eq!(
            stats.last_path,
            Some(if fallback {
                CapturePath::PrintWindow
            } else {
                CapturePath::Wgc
            })
        );
        assert_eq!(stats.fallbacks, if fallback { 2 } else { 0 });
    }
}

#[tokio::test]
async fn capture_incomplete_stale_or_unlocatable_login_delivers_no_image_on_both_paths() {
    for fallback in [false, true] {
        let (api, driver, _, win) = common::fixture().await;
        let (wgc, _, backend) = sources();
        api.update(|s| s.windows[0].frame = source_window().frame);
        if fallback {
            wgc.set_fault(CaptureFault::Unavailable);
        }
        let driver = driver.with_capture(backend);
        let capture = driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap();
        let box_ = Some(Rect {
            x: 20.,
            y: 16.,
            w: 30.,
            h: 12.,
        });
        let tree = processed(raw_password(box_), 1200);
        for (version, stamp) in [(999, STAMP), (42, STAMP + Duration::from_millis(150))] {
            assert_eq!(
                redact::apply(&tree, CapturedFrame::new(capture.clone(), version, stamp))
                    .unwrap_err(),
                redact::RedactionError::Stale
            );
        }
        let tree = processed(raw_password(None), 1200);
        assert_eq!(
            redact::apply(&tree, CapturedFrame::new(capture.clone(), 42, STAMP)).unwrap_err(),
            redact::RedactionError::NeedsHandoff
        );
        for reason in [
            IncompleteReason::NodeCap,
            IncompleteReason::DepthCap,
            IncompleteReason::PartialError,
            IncompleteReason::Deadline,
        ] {
            let mut raw = raw_password(box_);
            raw.incomplete = Some(Incomplete {
                reason,
                read_nodes: 1,
                unread_nodes: None,
            });
            let tree = processed(raw, 1200);
            assert_eq!(
                redact::apply(&tree, CapturedFrame::new(capture.clone(), 42, STAMP)).unwrap_err(),
                redact::RedactionError::NeedsHandoff
            );
        }
    }
}
