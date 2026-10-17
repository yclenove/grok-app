//! Session gallery admits only redacted images and refuses cross-session reads.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use gcu_host_protocol::events::{Fault, Gallery, Role};
use gcu_host_protocol::messages::{Event, Image};

struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("gcu-shots-{}-{nanos}-{n}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn gallery(&self) -> Gallery {
        Gallery::open(self.path.clone())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn png(payload: &[u8]) -> Image {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(payload);
    Image {
        mime: "image/png".into(),
        width: 2,
        height: 2,
        bytes,
    }
}

fn event(session: &str, image: Image) -> Event {
    serde_json::from_value(serde_json::json!({
        "session_id": session,
        "tool": "click",
        "target": "Save",
        "outcome": "done",
        "duration_ms": 12,
        "thumbnail": {
            "mime": image.mime,
            "width": image.width,
            "height": image.height,
            "bytes": image.bytes,
        }
    }))
    .unwrap()
}

#[test]
fn admit_png_same_session_returns_private_file() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let image = png(b"pixels");
    let path = gallery.admit("sess-a", "i_1", &image, Role::Full).unwrap();
    let root = temp.path.canonicalize().unwrap();
    assert!(path.canonicalize().unwrap().starts_with(&root));
    assert_eq!(path.file_name().unwrap(), "i_1.full.png");
    assert_eq!(std::fs::read(&path).unwrap(), image.bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let owned = gallery.paths("sess-a", "i_1").unwrap();
    assert_eq!(owned.full.as_deref(), Some(path.as_path()));
    assert!(owned.thumb.is_none());
}

#[test]
fn resolve_other_session_is_foreign() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let path = gallery
        .admit("sess-a", "i_1", &png(b"a"), Role::Thumb)
        .unwrap();
    assert_eq!(gallery.paths("sess-b", "i_1"), Err(Fault::Foreign));
    assert!(path.is_file());
    assert_eq!(gallery.paths("sess-a", "missing"), Err(Fault::Missing));
}

#[test]
fn second_session_cannot_overwrite_owner() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let first = png(b"owner");
    let path = gallery.admit("sess-a", "i_1", &first, Role::Full).unwrap();
    assert_eq!(
        gallery.admit("sess-b", "i_1", &png(b"intruder"), Role::Full),
        Err(Fault::Foreign)
    );
    assert_eq!(std::fs::read(&path).unwrap(), first.bytes);
    assert_eq!(
        gallery.paths("sess-a", "i_1").unwrap().full.as_deref(),
        Some(path.as_path())
    );
}

#[test]
fn drop_session_deletes_files_and_resolve_is_missing() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let path = gallery
        .admit("sess-a", "i_1", &png(b"gone"), Role::Thumb)
        .unwrap();
    gallery.note(&event("sess-a", png(b"thumb"))).unwrap();
    assert!(path.is_file());
    gallery.drop_session("sess-a");
    assert!(!path.exists());
    assert_eq!(gallery.paths("sess-a", "i_1"), Err(Fault::Missing));
    assert!(!session_dir(&temp.path, "sess-a").exists());
    gallery.drop_session("sess-a");
}

#[test]
fn admit_text_bytes_is_rejected() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let text = Image {
        mime: "image/png".into(),
        width: 2,
        height: 2,
        bytes: b"not-an-image".to_vec(),
    };
    assert_eq!(
        gallery.admit("sess-a", "i_1", &text, Role::Full),
        Err(Fault::Rejected)
    );
    let jpeg_mime = Image {
        mime: "image/jpeg".into(),
        width: 2,
        height: 2,
        bytes: b"\x89PNG\r\n\x1a\n".to_vec(),
    };
    assert_eq!(
        gallery.admit("sess-a", "i_1", &jpeg_mime, Role::Full),
        Err(Fault::Rejected)
    );
    assert!(!session_dir(&temp.path, "sess-a").exists());
}

#[test]
fn admit_zero_size_is_rejected() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let mut image = png(b"x");
    image.width = 0;
    assert_eq!(
        gallery.admit("sess-a", "i_1", &image, Role::Thumb),
        Err(Fault::Rejected)
    );
    image.width = 2;
    image.bytes.clear();
    assert_eq!(
        gallery.admit("sess-a", "i_1", &image, Role::Thumb),
        Err(Fault::Rejected)
    );
}

#[test]
fn admit_id_with_parent_segments_is_rejected() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let image = png(b"x");
    for id in ["../x", "a/b", "..", "", ".hidden"] {
        assert_eq!(
            gallery.admit("sess-a", id, &image, Role::Full),
            Err(Fault::Rejected),
            "{id}"
        );
    }
    assert_eq!(
        gallery.admit("../sess", "i_1", &image, Role::Full),
        Err(Fault::Rejected)
    );
    assert!(!session_dir(&temp.path, "sess-a").exists());
}

#[test]
fn admit_oversize_bytes_is_rejected() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    let mut bytes = vec![0_u8; 8 * 1024 * 1024 + 1];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    let image = Image {
        mime: "image/png".into(),
        width: 1,
        height: 1,
        bytes,
    };
    assert_eq!(
        gallery.admit("sess-a", "i_big", &image, Role::Full),
        Err(Fault::Rejected)
    );
    assert!(!session_dir(&temp.path, "sess-a").exists());
}

#[test]
fn note_thumbnail_then_drop_session_removes_file() {
    let temp = Temp::new();
    let gallery = temp.gallery();
    gallery.note(&event("sess-a", png(b"event"))).unwrap();
    let dir = session_dir(&temp.path, "sess-a");
    let files = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(files, 1);
    gallery.drop_session("sess-a");
    assert!(!dir.exists());
}

fn session_dir(root: &Path, session: &str) -> PathBuf {
    root.join(session)
}

struct Inner;

impl gcu_host_protocol::server::HostCallbacks for Inner {
    fn notification(
        &self,
        _request: &gcu_host_protocol::messages::Request,
    ) -> Option<Result<serde_json::Value, gcu_host_protocol::messages::RpcError>> {
        Some(Ok(serde_json::json!({ "seen": true })))
    }

    fn handle(
        &self,
        _request: gcu_host_protocol::messages::Request,
    ) -> gcu_host_protocol::server::CallbackFuture<'_> {
        Box::pin(async { Ok(serde_json::json!({})) })
    }

    fn stop_state(&self) -> Option<bool> {
        Some(true)
    }

    fn disconnected(&self) {}
}

#[test]
fn observe_forwards_stop_state_and_notifications_to_the_inner_callbacks() {
    use gcu_host_protocol::server::HostCallbacks;
    let temp = Temp::new();
    let wrapped = gcu_host_protocol::events::observe(std::sync::Arc::new(Inner), temp.gallery());
    // A Host latch that the wrapper hid would let a new engine start unstopped.
    assert_eq!(wrapped.stop_state(), Some(true));
    let request: gcu_host_protocol::messages::Request = serde_json::from_value(
        serde_json::json!({ "jsonrpc": "2.0", "method": "host.overlay.cursor", "params": {} }),
    )
    .unwrap();
    assert!(wrapped.notification(&request).is_some());
}
