//! Explicit native fixture contract. Custom harness keeps AppKit on main.
//! Default: not_run. Opt in with GCU_OVERLAY_NATIVE=1 on an interactive runner.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_host_api::{CursorKind, OverlayProvider, ScreenPoint, SessionCtx, StatusText};
use gcu_overlay::{Catalog, Config, Controls, CopyKey, DesktopOverlay, Error, MonotonicClock};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

struct TestCopy;
impl Catalog for TestCopy {
    fn text(&self, key: CopyKey) -> String {
        // Test-only labels; product Hosts must supply the complete i18n table.
        format!("Fixture {key:?}")
    }
}
#[derive(Default)]
struct TestControls(AtomicBool);
impl Controls for TestControls {
    fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn resume(&self) -> Result<(), Error> {
        self.0.store(false, Ordering::Release);
        Ok(())
    }
    fn stopped(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    // This probe observes a fixture only and never enables an input route.
    fn healthy(&self) -> bool {
        true
    }
    fn open_approval(&self, _: &str) -> Result<(), Error> {
        Ok(())
    }
}
enum Message {
    Frame(mpsc::SyncSender<()>),
    Finished(Result<(), String>),
}
fn main() {
    if std::env::var("GCU_OVERLAY_NATIVE").as_deref() != Ok("1") {
        println!("not_run: native overlay requires an interactive fixture runner and GCU_OVERLAY_NATIVE=1");
        return;
    }
    let mut ui = DesktopOverlay::open(
        Arc::new(MonotonicClock::default()),
        Arc::new(TestCopy),
        Arc::new(TestControls::default()),
        Config::default(),
    )
    .unwrap();
    let overlay = ui.provider();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            native_contract(overlay, &send)
        }));
        let result = result.unwrap_or_else(|_| Err("native screenshot contract panicked".into()));
        let _ = send.send(Message::Finished(result));
    });
    loop {
        match receive.recv_timeout(gcu_overlay::animation::FRAME_INTERVAL) {
            Ok(Message::Frame(ack)) => {
                ui.tick().unwrap();
                ack.send(()).unwrap();
            }
            Ok(Message::Finished(result)) => {
                result.unwrap();
                println!("PASS: native overlay screenshot pixels unchanged");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => ui.tick().unwrap(),
            Err(e) => panic!("native fixture worker disconnected: {e}"),
        }
    }
}
fn barrier(send: &mpsc::Sender<Message>) {
    let (ack, ready) = mpsc::sync_channel(1);
    send.send(Message::Frame(ack)).unwrap();
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
}
fn pixel_hash(bytes: &[u8]) -> u64 {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let mut hash = DefaultHasher::new();
    pixels[..info.buffer_size()].hash(&mut hash);
    hash.finish()
}
#[cfg(any(windows, target_os = "macos"))]
fn native_contract(
    overlay: gcu_overlay::Overlay,
    send: &mpsc::Sender<Message>,
) -> Result<(), String> {
    use gcu_core::driver::{AppQuery, CaptureOpts, Driver, WindowRef};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        #[cfg(windows)]
        let driver = gcu_driver_windows::WindowsDriver::new_in_process()
            .await
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        let driver = gcu_driver_macos::MacosDriver::new()
            .await
            .map_err(|e| e.to_string())?;
        let app = driver
            .resolve_app(&AppQuery("gcu-fixture".into()))
            .await
            .map_err(|e| e.to_string())?;
        let window = driver
            .list_windows(Some(&app))
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or("start the static gcu-fixture window first")?;
        let target = WindowRef {
            app,
            id: window.id.clone(),
        };
        let cancel = gcu_core::driver::Cancel::default();
        let before = driver
            .capture(&target, &CaptureOpts::default(), &cancel)
            .await
            .map_err(|e| e.to_string())?;
        let geometry = driver
            .window_geometry(&target)
            .await
            .map_err(|e| e.to_string())?;
        let ctx = SessionCtx {
            session_id: "native-fixture".into(),
        };
        overlay.status(
            &ctx,
            &StatusText {
                message: String::new(),
                foreground: true,
            },
        );
        overlay.show_target(
            &ctx,
            &gcu_host_api::WindowInfo {
                id: window.id,
                title: window.title,
                frame: geometry.frame,
                scale: geometry.scale,
            },
            ScreenPoint {
                x: geometry.frame.x + 80.,
                y: geometry.frame.y + 80.,
            },
            CursorKind::Pointer,
        );
        barrier(send);
        assert!(overlay.exclude_from_capture_supported());
        let after = driver
            .capture(&target, &CaptureOpts::default(), &cancel)
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(pixel_hash(&before.bytes), pixel_hash(&after.bytes));
        overlay.hide(&ctx);
        barrier(send);
        Ok(())
    })
}
#[cfg(target_os = "linux")]
fn native_contract(
    overlay: gcu_overlay::Overlay,
    send: &mpsc::Sender<Message>,
) -> Result<(), String> {
    use gcu_driver_linux::x11::{RealX, XServer};
    let x = RealX::connect().map_err(|e| e.to_string())?;
    let fixture = x
        .client_list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|id| {
            x.window_name(*id)
                .ok()
                .flatten()
                .is_some_and(|n| n.contains("GCU Fixture"))
        })
        .ok_or("start the static gcu-fixture window first")?;
    let capture = |id| gcu_driver_linux::x11::capture_window(&x, id).map_err(|e| e.to_string());
    let before = capture(fixture)?;
    let ctx = SessionCtx {
        session_id: "native-fixture".into(),
    };
    let g = x.window_geometry(fixture).map_err(|e| e.to_string())?;
    overlay.status(
        &ctx,
        &StatusText {
            message: String::new(),
            foreground: true,
        },
    );
    overlay.show_target(
        &ctx,
        &gcu_host_api::WindowInfo {
            id: fixture.to_string(),
            title: String::new(),
            scale: 1.,
            frame: gcu_overlay::Rect {
                x: g.x.into(),
                y: g.y.into(),
                w: g.width.into(),
                h: g.height.into(),
            },
        },
        ScreenPoint {
            x: f64::from(g.x) + 80.,
            y: f64::from(g.y) + 80.,
        },
        CursorKind::Pointer,
    );
    barrier(send);
    assert!(!overlay.exclude_from_capture_supported()); // X11 uses the hide lease instead.
    let after = capture(fixture)?;
    assert_eq!(pixel_hash(&before.bytes), pixel_hash(&after.bytes));
    overlay.hide(&ctx);
    barrier(send);
    Ok(())
}
