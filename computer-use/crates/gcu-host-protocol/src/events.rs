//! Session-owned redacted screenshots for the chat tool card.
//!
//! The only input is a Host [`Image`]: encoded bytes that already left the
//! redaction pipeline. This module never accepts a filesystem path, and it
//! never reads a path the model printed. A derivative file is created only
//! after the bytes pass the media-type check. `paths` refuses an id owned by
//! another session. `drop_session` deletes that session's directory.

use crate::auth::ProcessInstance;
use crate::messages::{Empty, Event, Image, Request, RpcError};
use crate::server::{CallbackFuture, HostCallbacks, Peer};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

mod gallery_cleanup;

/// Why a screenshot was not stored or not returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// This id is stored for a different session.
    Foreign,
    /// This session has no such image.
    Missing,
    /// The bytes, id, or path were not a redacted image we can keep.
    Rejected,
}

/// Which derivative of one delivered image to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Small preview from `host.event.emit`.
    Thumb,
    /// Image returned by `engine.screenshot.get`.
    Full,
}

/// Paths this session is allowed to preview. Both are absent only when the
/// row exists with no file, which `paths` does not return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedPaths {
    /// Redacted thumbnail, when one was admitted.
    pub thumb: Option<PathBuf>,
    /// Redacted full image, when one was admitted.
    pub full: Option<PathBuf>,
}

struct Stored {
    path: PathBuf,
    len: usize,
}

struct Row {
    session: String,
    thumb: Option<Stored>,
    full: Option<Stored>,
}

struct State {
    root: PathBuf,
    root_canon: Option<PathBuf>,
    rows: HashMap<String, Row>,
    order: VecDeque<String>,
    closed: HashSet<String>,
}

/// In-memory index plus the private directory of derivative files.
///
/// Clone shares the index. Blocking file work happens inside the methods;
/// callers on an async runtime should move those calls to `spawn_blocking`.
/// Timeout: none. Cancellation: none. Thread safety: interior mutex.
#[derive(Clone)]
pub struct Gallery {
    state: Arc<Mutex<State>>,
}

const MAX_IMAGE: usize = 8 * 1024 * 1024;
const MAX_SESSION: usize = 32 * 1024 * 1024;

impl Gallery {
    /// Remember `root` but do not create it until the first admitted image.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    #[must_use]
    pub fn open(root: PathBuf) -> Self {
        Self::sweep_orphans(&root);
        Self {
            state: Arc::new(Mutex::new(State {
                root,
                root_canon: None,
                rows: HashMap::new(),
                order: VecDeque::new(),
                closed: HashSet::new(),
            })),
        }
    }

    /// Retry closed, failed, and temporary derivative directories under `root`.
    ///
    /// Unmarked session directories are left in place. Blocking: a directory
    /// scan. Timeout: none. Cancellation: none.
    pub fn sweep_orphans(root: &Path) {
        gallery_cleanup::sweep(root);
    }

    /// Write one redacted image for `session` and `id`.
    ///
    /// An id already owned by another session is [`Fault::Foreign`] and is not
    /// overwritten. Blocking: creates directories and writes one file.
    /// Timeout: none. Cancellation: none.
    pub fn admit(
        &self,
        session: &str,
        id: &str,
        image: &Image,
        role: Role,
    ) -> Result<PathBuf, Fault> {
        let ext = checked_image(image)?;
        if !safe_key(session) || !safe_key(id) {
            return Err(Fault::Rejected);
        }
        let mut state = lock(&self.state);
        if state.closed.contains(session) {
            return Err(Fault::Missing);
        }
        if let Some(row) = state.rows.get(id) {
            if row.session != session {
                return Err(Fault::Foreign);
            }
        }
        let root = ensure_root(&mut state)?;
        let dir = root.join(session);
        if gallery_cleanup::ensure_private_dir(&dir).is_err() {
            return Err(Fault::Rejected);
        }
        if !contained(&root, &dir) {
            return Err(Fault::Rejected);
        }
        let path = dir.join(file_name(id, role, ext));
        if is_symlink(&path) {
            return Err(Fault::Rejected);
        }
        let replaced = replaced_len(&state, id, role);
        let projected = session_bytes(&state, session)
            .saturating_sub(replaced)
            .saturating_add(image.bytes.len());
        if projected > MAX_SESSION {
            trim_session(&mut state, session, id, image.bytes.len());
        }
        let replaced = replaced_len(&state, id, role);
        let projected = session_bytes(&state, session)
            .saturating_sub(replaced)
            .saturating_add(image.bytes.len());
        if projected > MAX_SESSION {
            return Err(Fault::Rejected);
        }
        if gallery_cleanup::write_private(&path, &image.bytes).is_err() {
            return Err(Fault::Rejected);
        }
        if !contained(&root, &path) {
            let _ = std::fs::remove_file(&path);
            return Err(Fault::Rejected);
        }
        let stored = Stored {
            path: path.clone(),
            len: image.bytes.len(),
        };
        let row = state.rows.entry(id.to_owned()).or_insert_with(|| Row {
            session: session.to_owned(),
            thumb: None,
            full: None,
        });
        match role {
            Role::Thumb => row.thumb = Some(stored),
            Role::Full => row.full = Some(stored),
        }
        if !state.order.iter().any(|item| item == id) {
            state.order.push_back(id.to_owned());
        }
        Ok(path)
    }

    /// Return derivative paths only when `session` owns `id`.
    ///
    /// Blocking: none. Timeout: none. Cancellation: none.
    pub fn paths(&self, session: &str, id: &str) -> Result<OwnedPaths, Fault> {
        if !safe_key(session) || !safe_key(id) {
            return Err(Fault::Rejected);
        }
        let state = lock(&self.state);
        if state.closed.contains(session) {
            return Err(Fault::Missing);
        }
        let Some(row) = state.rows.get(id) else {
            return Err(Fault::Missing);
        };
        if row.session != session {
            return Err(Fault::Foreign);
        }
        Ok(OwnedPaths {
            thumb: row.thumb.as_ref().map(|item| item.path.clone()),
            full: row.full.as_ref().map(|item| item.path.clone()),
        })
    }

    /// Delete every derivative stored for `session`.
    ///
    /// Unknown sessions are a no-op. A symlinked directory is not removed.
    /// Blocking: deletes one directory. Timeout: none. Cancellation: none.
    pub fn drop_session(&self, session: &str) {
        if !safe_key(session) {
            return;
        }
        let mut state = lock(&self.state);
        state.closed.insert(session.to_owned());
        purge_session(&mut state, session);
    }

    /// Immediately deny late writes/reads without filesystem IO. No waits.
    pub fn block_session(&self, session: &str) {
        lock(&self.state).closed.insert(session.to_owned());
    }

    /// Delete a closed generation on a blocking worker. If already reopened,
    /// the obsolete cleanup is inert. No timeout or cancellation.
    pub fn purge_closed(&self, session: &str) {
        let mut state = lock(&self.state);
        if safe_key(session) && state.closed.contains(session) {
            purge_session(&mut state, session);
        }
    }

    /// Clear old derivatives before admitting a fresh namespace. Blocking file
    /// deletion; call off the async/UI thread. No timeout or cancellation.
    pub fn open_session(&self, session: &str) {
        if !safe_key(session) {
            return;
        }
        let mut state = lock(&self.state);
        purge_session(&mut state, session);
        state.closed.remove(session);
    }

    /// Store the event thumbnail, if the engine sent one.
    ///
    /// The wire event has no screenshot id, so the file is keyed by tool,
    /// duration, and a hash of the target label. Blocking: same as [`admit`].
    /// Timeout: none. Cancellation: none.
    pub fn note(&self, event: &Event) -> Result<(), Fault> {
        let Some(image) = &event.thumbnail else {
            return Ok(());
        };
        let id = event_id(event);
        self.admit(&event.session_id, &id, image, Role::Thumb)
            .map(|_| ())
    }
}

fn purge_session(state: &mut State, session: &str) {
    if let Some(root) = state.root_canon.clone() {
        let dir = root.join(session);
        if removable_session_dir(&root, &dir) {
            gallery_cleanup::purge_dir(&dir);
        }
    }
    let State { rows, order, .. } = state;
    rows.retain(|_, row| row.session != session);
    order.retain(|id| rows.contains_key(id));
}

/// Forward every method, and keep `host.event.emit` thumbnails in `gallery`.
///
/// The notification still resolves to an empty object. A rejected thumbnail
/// is dropped; it does not fail the engine. Blocking: the thumbnail write
/// runs on the callback task. Timeout: the server's five-second cap.
/// Cancellation: disconnect drops the callback.
#[must_use]
pub fn observe(inner: Arc<dyn HostCallbacks>, gallery: Gallery) -> Arc<dyn HostCallbacks> {
    Arc::new(Observing {
        inner,
        gallery,
        panel: None,
    })
}

/// [`observe`] plus the side-panel timeline.
///
/// The panel clock is wall time. A rejected thumbnail is still recorded as a
/// step when the event parsed. Blocking: the thumbnail write runs on the
/// callback task. Timeout: the server's five-second cap. Cancellation:
/// disconnect drops the callback.
#[must_use]
pub fn observe_panel(
    inner: Arc<dyn HostCallbacks>,
    gallery: Gallery,
    panel: crate::panel::Panel,
) -> Arc<dyn HostCallbacks> {
    Arc::new(Observing {
        inner,
        gallery,
        panel: Some(panel),
    })
}

struct Observing {
    inner: Arc<dyn HostCallbacks>,
    gallery: Gallery,
    panel: Option<crate::panel::Panel>,
}

impl HostCallbacks for Observing {
    fn handle(&self, request: Request) -> CallbackFuture<'_> {
        if request.method == "host.event.emit" {
            let gallery = self.gallery.clone();
            let panel = self.panel.clone();
            return Box::pin(async move {
                if let Ok(event) = serde_json::from_value::<Event>(request.params) {
                    let _ = gallery.note(&event);
                    if let Some(panel) = panel {
                        let _ = panel.push(&event, wall_ms());
                    }
                }
                empty_value()
            });
        }
        self.inner.handle(request)
    }

    fn notification(&self, request: &Request) -> Option<Result<Value, RpcError>> {
        self.inner.notification(request)
    }

    fn attached(&self, engine: ProcessInstance, peer: Peer) {
        self.inner.attached(engine, peer);
    }

    fn stop_state(&self) -> Option<bool> {
        self.inner.stop_state()
    }

    fn control_attached(&self, peer: Peer) {
        self.inner.control_attached(peer);
    }

    fn disconnected(&self) {
        self.inner.disconnected();
    }
}

fn empty_value() -> Result<Value, RpcError> {
    serde_json::to_value(Empty {}).map_err(|_| RpcError::new(-32603, "event response"))
}

fn lock(mutex: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    mutex.lock().unwrap_or_else(|err| err.into_inner())
}

fn checked_image(image: &Image) -> Result<&'static str, Fault> {
    if image.bytes.is_empty()
        || image.bytes.len() > MAX_IMAGE
        || image.width == 0
        || image.height == 0
    {
        return Err(Fault::Rejected);
    }
    match image.mime.as_str() {
        "image/png" if image.bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Ok("png"),
        "image/jpeg" if image.bytes.starts_with(&[0xFF, 0xD8, 0xFF]) => Ok("jpg"),
        _ => Err(Fault::Rejected),
    }
}

pub(crate) fn safe_key(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() || value.len() > 128 {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn file_name(id: &str, role: Role, ext: &str) -> String {
    let kind = match role {
        Role::Thumb => "thumb",
        Role::Full => "full",
    };
    format!("{id}.{kind}.{ext}")
}

fn event_id(event: &Event) -> String {
    thumbnail_id(event)
}

/// Stable screenshot id for one event thumbnail.
///
/// The wire event has no screenshot id. The key is the tool, the duration, and
/// a hash of the target label. It does not include the session id, so the same
/// triple in another session is [`Fault::Foreign`]. Blocking: none.
/// Timeout: none. Cancellation: none.
#[must_use]
pub fn thumbnail_id(event: &Event) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in event.target.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("e-{}-{}-{hash:x}", event.tool.as_str(), event.duration_ms)
}

fn wall_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn ensure_root(state: &mut State) -> Result<PathBuf, Fault> {
    if let Some(root) = &state.root_canon {
        return Ok(root.clone());
    }
    if gallery_cleanup::ensure_private_dir(&state.root).is_err() {
        return Err(Fault::Rejected);
    }
    match state.root.canonicalize() {
        Ok(root) => {
            state.root_canon = Some(root.clone());
            Ok(root)
        }
        Err(_) => Err(Fault::Rejected),
    }
}

fn contained(root: &Path, path: &Path) -> bool {
    let Ok(canon) = path.canonicalize() else {
        return false;
    };
    canon.starts_with(root) && canon != root
}

fn removable_session_dir(root: &Path, dir: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return false;
    }
    let Ok(canon) = dir.canonicalize() else {
        return false;
    };
    canon.starts_with(root) && canon != root
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

fn replaced_len(state: &State, id: &str, role: Role) -> usize {
    let Some(row) = state.rows.get(id) else {
        return 0;
    };
    let stored = match role {
        Role::Thumb => row.thumb.as_ref(),
        Role::Full => row.full.as_ref(),
    };
    stored.map_or(0, |item| item.len)
}

fn session_bytes(state: &State, session: &str) -> usize {
    state
        .rows
        .values()
        .filter(|row| row.session == session)
        .map(|row| {
            row.thumb.as_ref().map_or(0, |item| item.len)
                + row.full.as_ref().map_or(0, |item| item.len)
        })
        .sum()
}

fn trim_session(state: &mut State, session: &str, keep: &str, incoming: usize) {
    while session_bytes(state, session).saturating_add(incoming) > MAX_SESSION {
        let Some(old) = state
            .order
            .iter()
            .find(|id| {
                *id != keep
                    && state
                        .rows
                        .get(*id)
                        .is_some_and(|row| row.session == session)
            })
            .cloned()
        else {
            break;
        };
        remove_id(state, &old);
    }
}

fn remove_id(state: &mut State, id: &str) {
    if let Some(row) = state.rows.remove(id) {
        if let Some(item) = row.thumb.as_ref() {
            gallery_cleanup::forget_file(&item.path);
        }
        if let Some(item) = row.full.as_ref() {
            gallery_cleanup::forget_file(&item.path);
        }
    }
    state.order.retain(|item| item != id);
}
