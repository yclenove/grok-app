//! Route B contracts run without Tauri or a desktop.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_protocol::{
    messages::{CursorKind, OverlayShow},
    overlay::*,
};
use gcu_protocol::Rect;

fn monitor(x: f64, scale: f64) -> Monitor {
    Monitor {
        frame: Rect {
            x,
            y: -200.,
            w: 1920.,
            h: 1080.,
        },
        scale,
    }
}
fn target() -> OverlayShow {
    OverlayShow {
        session_id: "s".into(),
        window_id: "w".into(),
        title: "fixture".into(),
        frame: Rect {
            x: -1800.,
            y: -100.,
            w: 800.,
            h: 600.,
        },
        scale: 2.,
        x: -1600.,
        y: 100.,
        kind: CursorKind::Pointer,
    }
}

#[test]
fn mixed_dpi_negative_origin_uses_monitor_scale_and_local_css_coordinates() {
    let p = placement(&[monitor(0., 1.), monitor(-1920., 2.)], Some(&target())).unwrap();
    assert_eq!(p.monitor.frame.x, -1920.);
    assert_eq!((p.cursor.unwrap().x, p.cursor.unwrap().y), (160., 150.));
    assert_eq!(p.capsule.w, 640. * 2.);
    assert!(p.capsule.x >= -1920.);
}

#[test]
fn macos_mixed_backing_factors_select_display_in_global_points() {
    let displays = [
        Monitor {
            frame: Rect {
                x: 0.,
                y: 0.,
                w: 3840.,
                h: 2160.,
            },
            scale: 2.,
        },
        Monitor {
            frame: Rect {
                x: 1920.,
                y: 0.,
                w: 1920.,
                h: 1080.,
            },
            scale: 1.,
        },
    ];
    let mut show = target();
    show.scale = 1.;
    show.frame = Rect {
        x: 1920.,
        y: 0.,
        w: 800.,
        h: 600.,
    };
    show.x = 2020.;
    show.y = 100.;
    let p = backing_placement(&displays, Some(&show)).unwrap();
    assert_eq!(p.monitor.scale, 1.);
    assert_eq!((p.cursor.unwrap().x, p.cursor.unwrap().y), (100., 100.));
    assert_eq!(p.capsule.w, 640.);

    // The driver uses the target window's backing factor for the global
    // origin, even when the pointer crosses onto a screen with another DPI.
    show.scale = 2.;
    show.x *= 2.;
    show.y *= 2.;
    show.frame.x *= 2.;
    show.frame.w *= 2.;
    show.frame.h *= 2.;
    let p = backing_placement(&displays, Some(&show)).unwrap();
    assert_eq!(p.monitor.scale, 1.);
    assert_eq!((p.cursor.unwrap().x, p.cursor.unwrap().y), (100., 100.));
}

#[test]
fn macos_negative_display_origin_restores_its_own_backing_factor() {
    let displays = [monitor(0., 1.), monitor(-3840., 2.)];
    let mut show = target();
    show.x = -1600.;
    show.y = 100.;
    show.scale = 1.;
    let p = backing_placement(&displays, Some(&show)).unwrap();
    assert_eq!(p.monitor.frame.x, -3840.);
    assert_eq!((p.cursor.unwrap().x, p.cursor.unwrap().y), (320., 200.));
    assert_eq!(p.capsule.w, 1280.);
}

#[test]
fn unplugged_monitor_moves_capsule_onscreen_and_suppresses_offscreen_cursor() {
    let p = placement(&[monitor(0., 1.25)], Some(&target())).unwrap();
    assert!(p.capsule.x >= 0.);
    assert!(p.cursor.is_none());
    assert!(placement(&[monitor(0., f64::NAN)], None).is_none());
}

#[test]
fn hidden_or_engine_replaced_does_not_clear_stop_or_show_old_cursor() {
    let mut desk = Desk::default();
    desk.show(target()).unwrap();
    desk.set_stopped(true);
    desk.hide("s");
    assert_eq!(desk.snapshot().phase, Phase::Stopped);
    assert!(desk.snapshot().target.is_none());
    desk.disconnect();
    assert_eq!(desk.snapshot().phase, Phase::Stopped);
    desk.set_stopped(false);
    assert_eq!(desk.snapshot().phase, Phase::Hidden);
}

#[derive(Default)]
struct Fake {
    calls: Vec<&'static str>,
    fail: bool,
}
impl Surface for Fake {
    type Error = ();
    fn nonactivating(&mut self) -> Result<(), ()> {
        self.calls.push("noactivate");
        Ok(())
    }
    fn passthrough(&mut self, cursor: bool) -> Result<(), ()> {
        self.calls.push(if cursor {
            "passthrough"
        } else {
            "capsule-input"
        });
        Ok(())
    }
    fn exclude_capture(&mut self) -> Result<(), ()> {
        self.calls.push("exclude");
        if self.fail {
            Err(())
        } else {
            Ok(())
        }
    }
    fn hide(&mut self) {
        self.calls.push("hide");
    }
}
#[test]
fn native_safety_failure_hides_before_any_surface_can_be_shown() {
    let mut fake = Fake {
        fail: true,
        ..Default::default()
    };
    assert!(prepare_surface(&mut fake, true).is_err());
    assert_eq!(fake.calls, ["noactivate", "passthrough", "exclude", "hide"]);
}

#[test]
fn capsule_keeps_pointer_input_but_uses_the_same_nonactivation_and_capture_guards() {
    let mut fake = Fake::default();
    prepare_surface(&mut fake, false).unwrap();
    assert_eq!(fake.calls, ["noactivate", "capsule-input", "exclude"]);
}

#[test]
fn target_invalid_dpi_or_nan_is_rejected_without_changing_scene() {
    let mut desk = Desk::default();
    let mut show = target();
    show.scale = 0.;
    assert!(desk.show(show).is_err());
    assert_eq!(desk.snapshot().phase, Phase::Hidden);
}

#[test]
fn idle_cursor_injected_monotonic_clock_expires_without_hiding_capsule() {
    let mut desk = Desk::default();
    desk.show_at(target(), 123).unwrap();
    assert!(desk.snapshot_at(30_122).cursor_visible);
    let snapshot = desk.snapshot_at(30_123);
    assert!(!snapshot.cursor_visible);
    assert_eq!(snapshot.phase, Phase::Running);
    assert_eq!(RESPONSE_MS, 350);
}

#[test]
fn capture_lease_hides_before_nonce_ack_and_never_remaps_until_lease_is_gone() {
    use gcu_host_protocol::overlay::capture::*;
    #[derive(Default)]
    struct FakeCapture {
        calls: Vec<&'static str>,
        fail_hide: bool,
    }
    impl CaptureSurface for FakeCapture {
        type Error = ();
        fn hide_all(&mut self) -> Result<(), ()> {
            self.calls.push("hide+flush");
            if self.fail_hide {
                Err(())
            } else {
                Ok(())
            }
        }
        fn acknowledge(&mut self, proofs: &[[u32; 5]]) -> Result<(), ()> {
            self.calls.push(if proofs.is_empty() {
                "clear-ack"
            } else {
                "nonce-ack"
            });
            Ok(())
        }
        fn show_all(&mut self) -> Result<(), ()> {
            self.calls.push("show");
            Ok(())
        }
    }
    let mut fake = FakeCapture::default();
    let proof = [[42, 1, 2, 3, 4]];
    capture_barrier(&mut fake, &proof).unwrap();
    capture_barrier(&mut fake, &proof).unwrap();
    assert_eq!(
        fake.calls,
        ["hide+flush", "nonce-ack", "hide+flush", "nonce-ack"]
    );
    capture_barrier(&mut fake, &[]).unwrap();
    assert_eq!(&fake.calls[4..], ["clear-ack", "show"]);
    fake.calls.clear();
    fake.fail_hide = true;
    assert!(capture_barrier(&mut fake, &proof).is_err());
    assert_eq!(fake.calls, ["hide+flush"]);
}
