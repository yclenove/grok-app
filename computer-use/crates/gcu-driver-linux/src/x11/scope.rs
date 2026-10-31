//! Scoped X11 window proof. Pixels come from the window pixmap only.
//!
//! `_NET_WM_PID` is client-controlled. It is compared with the fixture pid and
//! start time, and a missing pid, a missing incarnation, or a failed pixmap
//! read refuses the capture. That comparison is not server-authenticated
//! client ownership. The root window is not a fallback.
//!
//! Blocking reads run on a worker thread. The control runtime only waits, and
//! that wait shares the open [`gcu_core::driver::NATIVE_TIMEOUT`] budget.

use super::hotkey::StopLatch;
use super::server::{XServer, XWindowGeometry};
use crate::identity::scope::{
    confirm_twice, process_view, ProcessClock, ScopeFault, ScopeGate, ScopeOracle, X11Snapshot,
    X11View,
};
use gcu_core::driver::{Capture, DriverError};
use gcu_core::model::{Geometry, Rect};
use std::sync::Arc;
use std::time::Duration;

/// Borrowed gate used at X11 delivery.
pub(crate) struct X11Scope<'a> {
    gate: &'a ScopeGate,
    /// In-memory proof clock. Live delivery reads the oracle instead.
    #[cfg(test)]
    clock: &'a dyn ProcessClock,
    oracle: Option<&'a ScopeOracle>,
    stop: Option<Arc<dyn StopLatch>>,
}

impl<'a> X11Scope<'a> {
    /// Proof handle for one gate and clock.
    ///
    /// This synchronous handle is for in-memory servers. Production delivery
    /// uses [`Self::with_runtime`] so the X round trip leaves the runtime.
    #[cfg(test)]
    pub(crate) fn new(gate: &'a ScopeGate, clock: &'a dyn ProcessClock) -> Self {
        Self {
            gate,
            clock,
            oracle: None,
            stop: None,
        }
    }

    /// Proof handle that reads X off the control runtime.
    pub(crate) fn with_runtime(
        gate: &'a ScopeGate,
        clock: &'a dyn ProcessClock,
        oracle: &'a ScopeOracle,
        stop: Arc<dyn StopLatch>,
    ) -> Self {
        // The sync proof is the only reader. Live admission uses `oracle`.
        #[cfg(not(test))]
        let _ = clock;
        Self {
            gate,
            #[cfg(test)]
            clock,
            oracle: Some(oracle),
            stop: Some(stop),
        }
    }

    /// Re-read the window and refuse when it is not the admitted instance.
    ///
    /// In-memory only. A live server goes through [`Self::allow_live`].
    #[cfg(test)]
    pub(crate) fn allow(&self, x: &dyn XServer, xid: u32) -> Result<(), DriverError> {
        self.gate
            .prove_x11(&observe_x11(x, xid, self.clock))
            .map(|_| ())
            .map_err(ScopeFault::deny)
    }

    /// Re-read the window off the runtime, inside the open evidence budget.
    pub(crate) async fn allow_live(
        &self,
        x: &Arc<dyn XServer>,
        xid: u32,
    ) -> Result<(), DriverError> {
        let (Some(oracle), Some(stop)) = (self.oracle, self.stop.clone()) else {
            return Err(DriverError::ScopeDenied);
        };
        load_x11_snapshot(self.gate, oracle, &stop, x, xid)
            .await
            .map(|_| ())
    }
}

/// Read the incarnation first. Pid and geometry run only when it is present.
pub(crate) fn observe_x11(x: &dyn XServer, xid: u32, clock: &dyn ProcessClock) -> X11View {
    observe_snapshot(x, xid, clock).view
}

fn observe_snapshot(x: &dyn XServer, xid: u32, clock: &dyn ProcessClock) -> X11Snapshot {
    let incarnation = match x.window_incarnation(xid) {
        Ok(value) => value.filter(|value| *value > 0),
        Err(_) => None,
    };
    if incarnation.is_none() {
        return X11Snapshot {
            view: X11View {
                window_id: xid,
                net_wm_pid: None,
                incarnation: None,
                owner: None,
            },
            geometry: None,
        };
    }
    let net_wm_pid = x.window_pid(xid).ok().flatten();
    let owner = net_wm_pid.and_then(|pid| process_view(clock, pid));
    let geometry = x.window_geometry(xid).ok().map(|geom| {
        let scale = x.root_geometry().map(|root| root.scale).unwrap_or(1.0);
        Geometry {
            frame: Rect {
                x: geom.x as f64,
                y: geom.y as f64,
                w: geom.width as f64,
                h: geom.height as f64,
            },
            scale,
        }
    });
    X11Snapshot {
        view: X11View {
            window_id: xid,
            net_wm_pid,
            incarnation,
            owner,
        },
        geometry,
    }
}

/// Run `work` on a named thread and wait at most `budget`.
///
/// `None` means the budget ran out, Stop was latched, or the thread did not
/// start. Dropping the receiver leaves the worker running. The worker must
/// not own input release. A zero budget or an already latched Stop does not
/// start a thread.
pub(crate) async fn off_runtime<T, F>(
    budget: Duration,
    stop: &Arc<dyn StopLatch>,
    work: F,
) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if budget.is_zero() || stop.is_stopped() {
        return None;
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let Ok(worker) = std::thread::Builder::new()
        .name("gcu-linux-evidence".to_string())
        .spawn(move || {
            let _ = tx.send(work());
        })
    else {
        return None;
    };
    let stop = Arc::clone(stop);
    let stop_wait = async move {
        loop {
            if stop.is_stopped() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let outcome = tokio::select! {
        biased;
        _ = stop_wait => None,
        result = tokio::time::timeout(budget, rx) => {
            match result {
                Ok(Ok(value)) => Some(value),
                _ => None,
            }
        }
    };
    drop(worker);
    outcome
}

/// Observe `xid` off the runtime and store it only when the gate accepts it.
///
/// An exhausted budget, a missing incarnation, or a failed proof clears `xid`.
pub(crate) async fn load_x11_snapshot(
    gate: &ScopeGate,
    oracle: &ScopeOracle,
    stop: &Arc<dyn StopLatch>,
    x: &Arc<dyn XServer>,
    xid: u32,
) -> Result<X11Snapshot, DriverError> {
    if !oracle.budget_is_live() {
        oracle.clear_x11(xid);
        return Err(DriverError::ScopeDenied);
    }
    let budget = oracle.evidence_remaining();
    let clock = oracle.clock_arc();
    let server = Arc::clone(x);
    let observed = off_runtime(budget, stop, move || {
        observe_snapshot(server.as_ref(), xid, clock.as_ref())
    })
    .await;
    let Some(snapshot) = observed else {
        oracle.clear_x11(xid);
        return Err(DriverError::ScopeDenied);
    };
    if gate.prove_x11(&snapshot.view).is_err() {
        oracle.clear_x11(xid);
        return Err(DriverError::ScopeDenied);
    }
    oracle.store_x11(xid, snapshot.clone());
    Ok(snapshot)
}

/// Capture on a worker, then stop waiting when the budget or Stop wins.
///
/// The bytes stay on the worker when this returns [`DriverError::ScopeDenied`].
pub(crate) async fn capture_off_runtime(
    x: Arc<dyn XServer>,
    xid: u32,
    gate: ScopeGate,
    clock: Arc<dyn ProcessClock>,
    stop: &Arc<dyn StopLatch>,
    budget: Duration,
) -> Result<Capture, DriverError> {
    let observed = off_runtime(budget, stop, move || {
        capture_in_scope(x.as_ref(), xid, &gate, clock.as_ref())
    })
    .await;
    match observed {
        Some(result) => result,
        None => Err(DriverError::ScopeDenied),
    }
}

/// Capture `xid` only when it still proves as the admitted window.
///
/// The proof runs before the pixmap read and again before the bytes are
/// returned. Composite is required. A root-window crop is a refusal.
pub(crate) fn capture_in_scope(
    x: &dyn XServer,
    xid: u32,
    gate: &ScopeGate,
    clock: &dyn ProcessClock,
) -> Result<Capture, DriverError> {
    confirm_twice(
        || {
            gate.prove_x11(&observe_x11(x, xid, clock))
                .map(|_| ())
                .map_err(ScopeFault::deny)
        },
        || read_pixmap(x, xid),
    )
}

fn read_pixmap(x: &dyn XServer, xid: u32) -> Result<Capture, DriverError> {
    if !x.is_composite_available().unwrap_or(false) {
        return Err(DriverError::ScopeDenied);
    }
    let geom = x
        .window_geometry(xid)
        .map_err(|_| DriverError::ScopeDenied)?;
    if geom.width == 0
        || geom.height == 0
        || geom.width > u32::from(u16::MAX)
        || geom.height > u32::from(u16::MAX)
    {
        return Err(DriverError::ScopeDenied);
    }
    let pixmap = x
        .composite_name_window_pixmap(xid)
        .map_err(|_| DriverError::ScopeDenied)?;
    let image = x.get_image(pixmap, 0, 0, geom.width as u16, geom.height as u16);
    let _ = x.free_pixmap(pixmap);
    let image = image.map_err(|_| DriverError::ScopeDenied)?;
    let root = x.root_geometry().map_err(|_| DriverError::ScopeDenied)?;
    encode_png(&image.data, geom, root.scale)
}

fn encode_png(pixels: &[u8], geom: XWindowGeometry, scale: f64) -> Result<Capture, DriverError> {
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, geom.width, geom.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|_| DriverError::ScopeDenied)?;
    writer
        .write_image_data(pixels)
        .map_err(|_| DriverError::ScopeDenied)?;
    writer.finish().map_err(|_| DriverError::ScopeDenied)?;
    Ok(Capture {
        bytes: png_bytes,
        width: geom.width,
        height: geom.height,
        mime: "image/png".into(),
        geometry: Geometry {
            frame: Rect {
                x: geom.x as f64,
                y: geom.y as f64,
                w: geom.width as f64,
                h: geom.height as f64,
            },
            scale,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::scope::{MapClock, ScopeGate, ScopeInstance, ScopeWindow};
    use crate::x11::{FakeWindow, FakeX, RawImage, XRootGeometry, XWindowGeometry};
    use std::sync::Arc;

    fn root() -> XRootGeometry {
        XRootGeometry {
            root_id: 1,
            width: 64,
            height: 64,
            scale: 1.0,
        }
    }

    fn server(composite: bool) -> FakeX {
        let x = FakeX::new(root(), composite);
        x.add_window(FakeWindow {
            id: 7,
            name: Some("fixture".into()),
            pid: Some(42),
            geometry: XWindowGeometry {
                x: 1,
                y: 2,
                width: 2,
                height: 2,
                border_width: 0,
                depth: 24,
            },
            is_mapped: true,
            image: Some(RawImage {
                width: 2,
                height: 2,
                data: vec![9, 8, 7, 255, 9, 8, 7, 255, 9, 8, 7, 255, 9, 8, 7, 255],
            }),
        });
        x.set_window_incarnation(7, 5);
        x.set_root_image(RawImage {
            width: 64,
            height: 64,
            data: [1u8, 2, 3, 255].repeat(64 * 64),
        });
        x
    }

    fn gate() -> ScopeGate {
        let owner = ScopeInstance::try_new(42, 1000, "boot-a").unwrap();
        let window = ScopeWindow::try_new("7", 5, owner, "app.fixture").unwrap();
        ScopeGate::try_new(vec![window]).unwrap()
    }

    fn clock() -> MapClock {
        let clock = MapClock::new("boot-a");
        clock.set(42, 1000);
        clock
    }

    fn pixel(bytes: &[u8]) -> [u8; 4] {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap_or(4096)];
        let info = reader.next_frame(&mut buf).unwrap();
        let data = &buf[..info.buffer_size()];
        [data[0], data[1], data[2], data[3]]
    }

    #[test]
    fn capture_returns_the_window_pixmap_only_while_the_identity_holds() {
        let x = server(true);
        let shot = capture_in_scope(&x, 7, &gate(), &clock()).unwrap();
        assert_eq!(shot.width, 2);
        assert_eq!(pixel(&shot.bytes), [9, 8, 7, 255]);
        assert_eq!(x.get_events().len(), 0);
    }

    #[test]
    fn missing_composite_does_not_crop_the_root() {
        let x = server(false);
        let err = capture_in_scope(&x, 7, &gate(), &clock()).unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(!err.to_string().contains("fixture"));
    }

    #[test]
    fn pid_reuse_and_window_reuse_refuse_before_delivery() {
        let x = server(true);
        let clock = clock();
        clock.set(42, 2000);
        assert_eq!(
            capture_in_scope(&x, 7, &gate(), &clock).unwrap_err(),
            DriverError::ScopeDenied
        );
        clock.set(42, 1000);
        x.set_window_incarnation(7, 6);
        assert_eq!(
            X11Scope::new(&gate(), &clock).allow(&x, 7).unwrap_err(),
            DriverError::ScopeDenied
        );
        assert!(x.get_events().is_empty());
    }

    #[test]
    fn incarnation_change_during_the_read_drops_the_pixels() {
        let x = server(true);
        x.set_bump_incarnation_on_image(true);
        let err = capture_in_scope(&x, 7, &gate(), &clock()).unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(!format!("{err:?}").contains("9, 8, 7"));
    }

    #[tokio::test]
    async fn scoped_perform_does_not_emit_input_when_the_owner_changed() {
        use crate::driver::LinuxDriver;
        use crate::identity::scope::ScopeBinding;
        use gcu_core::driver::{
            Cancel, CaptureOpts, Driver, DriverAction, PointerRoute, WindowRef,
        };
        use gcu_core::model::WindowPoint;
        use gcu_protocol::tools::MouseButton;
        use gcu_protocol::AppInfo;

        let x = Arc::new(server(true));
        x.add_window(FakeWindow {
            id: 8,
            name: Some("other".into()),
            pid: Some(99),
            geometry: XWindowGeometry {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
                border_width: 0,
                depth: 24,
            },
            is_mapped: true,
            image: None,
        });
        let mut driver = LinuxDriver::new_fake(
            crate::atspi::FakeSource::default(),
            crate::atspi::FakeNode::default(),
            vec![AppInfo {
                id: "app.fixture".into(),
                name: "Fixture".into(),
                pid: Some(99),
                identity: None,
            }],
            Vec::new(),
        );
        driver = driver.with_x_server(Arc::clone(&x) as Arc<dyn XServer>);
        let binding = Arc::new(ScopeBinding::new(
            gate(),
            Arc::new(clock()) as Arc<dyn ProcessClock>,
        ));
        driver.bind_scope_binding(Arc::clone(&binding));
        let win = WindowRef {
            app: gcu_core::driver::AppRef {
                info: AppInfo {
                    id: "app.fixture".into(),
                    name: "Fixture".into(),
                    pid: Some(99),
                    identity: None,
                },
            },
            id: "8".into(),
        };
        let geom = driver.window_geometry(&win).await.unwrap_err();
        assert_eq!(geom, DriverError::ScopeDenied);
        let shot = driver
            .capture(&win, &CaptureOpts::default(), &Cancel::default())
            .await
            .unwrap_err();
        assert_eq!(shot, DriverError::ScopeDenied);
        let action = DriverAction::PointerClick {
            win,
            at: WindowPoint { x: 1.0, y: 1.0 },
            button: MouseButton::Left,
            count: 1,
            route: PointerRoute::AllowForeground,
        };
        let err = driver
            .perform(&action, &Cancel::default())
            .await
            .unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(x.get_events().is_empty());
        assert!(!err.to_string().contains("other"));
    }
}
