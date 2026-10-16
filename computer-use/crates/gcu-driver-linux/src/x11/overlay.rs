//! Cooperative X11 screenshot barrier. No server grab can block independent
//! Stop or input release. Each UI acknowledges a fresh capture nonce only after
//! hiding its windows, and cannot remap until the disposable lease is destroyed.
use super::XError;
use std::time::{Duration, Instant};
use x11rb::{
    connection::Connection,
    errors::ReplyError,
    protocol::{
        xproto::{ConnectionExt as _, *},
        ErrorKind,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

/// Owned exclusion scope. Drop ends the lease on success, error or unwind.
pub trait CaptureScope {
    /// Root crops cannot prove compositor repaint after hiding ARGB surfaces.
    fn allows_root_capture(&self) -> bool {
        true
    }
}
/// Fake/headless scope with no native overlays.
pub struct NoOverlays;
impl CaptureScope for NoOverlays {}
struct Lease<'a> {
    conn: &'a RustConnection,
    id: u32,
    root_safe: bool,
}
impl CaptureScope for Lease<'_> {
    fn allows_root_capture(&self) -> bool {
        self.root_safe
    }
}
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        // Connection loss also destroys this InputOnly window. The UI resumes
        // on its next frame; capture never maps a retired/replaced UI window.
        let _ = self.conn.destroy_window(self.id);
        let _ = self.conn.flush();
    }
}
fn error(e: impl std::fmt::Display) -> XError {
    XError::Protocol(e.to_string())
}
fn atom(conn: &RustConnection, name: &[u8]) -> Result<u32, XError> {
    Ok(conn
        .intern_atom(false, name)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .atom)
}
fn values(conn: &RustConnection, window: u32, property: u32) -> Result<Vec<u32>, XError> {
    let reply = conn
        .get_property(false, window, property, AtomEnum::CARDINAL, 0, 4096)
        .map_err(error)?
        .reply()
        .map_err(|e| match e {
            ReplyError::X11Error(e) if e.error_kind == ErrorKind::Window => {
                XError::WindowNotFound(window)
            }
            e => error(e),
        })?;
    Ok(reply.value32().map(|v| v.collect()).unwrap_or_default())
}
pub(super) fn begin(
    conn: &RustConnection,
    root: u32,
    target: u32,
) -> Result<Box<dyn CaptureScope + '_>, XError> {
    let capture = atom(conn, b"_GCU_OVERLAY_CAPTURE")?;
    let host_marker = atom(conn, b"_GCU_OVERLAY_HOST")?;
    let ack = atom(conn, b"_GCU_OVERLAY_ACK")?;
    let marker = atom(conn, b"_GCU_OVERLAY")?;
    if !values(conn, target, marker)?.is_empty() {
        return Err(XError::Protocol(
            "overlay windows are not capture targets".into(),
        ));
    }
    let id = conn.generate_id().map_err(error)?;
    conn.create_window(
        0,
        id,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        0,
        &CreateWindowAux::default(),
    )
    .map_err(error)?
    .check()
    .map_err(error)?;
    let mut lease = Lease {
        conn,
        id,
        root_safe: true,
    };
    // A fresh nonce prevents stale acknowledgments after XID/client-ID reuse.
    let nonce: [u32; 4] = rand::random();
    conn.change_property32(PropMode::REPLACE, id, capture, AtomEnum::CARDINAL, &nonce)
        .map_err(error)?
        .check()
        .map_err(error)?;
    let mut hosts = Vec::new();
    for window in conn
        .query_tree(root)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .children
    {
        match values(conn, window, host_marker) {
            Ok(value) if !value.is_empty() => hosts.push(window),
            // Other apps may close windows between QueryTree and GetProperty.
            Ok(_) | Err(XError::WindowNotFound(_)) => (),
            Err(e) => return Err(e),
        }
    }
    lease.root_safe = hosts.is_empty();
    let proof = [id, nonce[0], nonce[1], nonce[2], nonce[3]];
    let epoch = Instant::now();
    wait_hidden(
        || {
            for host in &hosts {
                if !values(conn, *host, ack)
                    .map_err(|e| error(format!("overlay Host acknowledgment failed: {e}")))?
                    .as_chunks::<5>()
                    .0
                    .iter()
                    .any(|chunk| chunk == &proof)
                {
                    return Ok(false);
                }
            }
            Ok(true)
        },
        || epoch.elapsed(),
    )?;
    Ok(Box::new(lease))
}
// Inject time so an unresponsive UI is tested without wall-clock sleeping.
fn wait_hidden(
    mut acknowledged: impl FnMut() -> Result<bool, XError>,
    mut now: impl FnMut() -> Duration,
) -> Result<(), XError> {
    let start = now();
    loop {
        if acknowledged()? {
            return Ok(());
        }
        if now().saturating_sub(start) >= Duration::from_millis(100) {
            return Err(XError::Protocol(
                "overlay hide acknowledgment timed out; no pixels captured".into(),
            ));
        }
        std::thread::yield_now();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[test]
    fn unresponsive_ui_injected_deadline_refuses_pixels_without_delaying_stop() {
        let time = Cell::new(0);
        let stop = Cell::new(false);
        let result = wait_hidden(
            || {
                stop.set(true);
                Ok(false)
            },
            || {
                let at = time.get();
                time.set(at + 25);
                Duration::from_millis(at)
            },
        );
        assert!(result.is_err());
        assert!(stop.get());
    }
    #[test]
    fn all_hosts_acknowledged_before_deadline_permits_capture() {
        let attempts = Cell::new(0);
        assert!(wait_hidden(
            || {
                attempts.set(attempts.get() + 1);
                Ok(attempts.get() == 3)
            },
            || Duration::ZERO
        )
        .is_ok());
        assert_eq!(attempts.get(), 3);
    }
}
