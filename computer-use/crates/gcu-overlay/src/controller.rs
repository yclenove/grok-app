//! Thread-safe notifications; bounded memory and no GUI waits.
use crate::{animation::Spring, lock, Catalog, Rect, Scene};
use gcu_host_api::{CursorKind, OverlayProvider, ScreenPoint, SessionCtx, StatusText, WindowInfo};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// Injectable monotonic time. Reads are nonblocking and never use wall time.
pub trait Clock: Send + Sync {
    /// Time elapsed since an arbitrary process-local epoch.
    fn now(&self) -> Duration;
}
/// Production time source. Tests supply a manually advanced clock.
pub struct MonotonicClock(Instant);
impl Default for MonotonicClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}
/// Visual options; disabling them never disables emergency Stop monitoring.
#[derive(Clone, Debug)]
pub struct Config {
    /// Mirrors `GCU_OVERLAY=0`, read by the Host rather than model arguments.
    pub visible: bool,
    /// Spring response in seconds. Default 0.35.
    pub response: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            visible: true,
            response: 0.35,
        }
    }
}
#[derive(Default)]
pub(crate) struct Model {
    pub sessions: BTreeMap<String, Session>,
    pub revision: u64,
}
pub(crate) struct Session {
    pub revision: u64,
    pub message: String,
    pub foreground: bool,
    pub approval: Option<String>,
    pub cursor: Option<(Spring, CursorKind, Duration)>,
    pub window: Option<Rect>,
    pub scale: f64,
}
/// Cloneable `OverlayProvider` for one standalone Host; no native handles cross threads.
#[derive(Clone)]
pub struct Overlay {
    pub(crate) capture_proven: Arc<AtomicBool>,
    pub(crate) model: Arc<Mutex<Model>>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) catalog: Arc<dyn Catalog>,
    pub(crate) config: Config,
}
impl Overlay {
    /// Construct the portable layer. No IO, waits, timeout or cancellation.
    pub fn new(clock: Arc<dyn Clock>, catalog: Arc<dyn Catalog>, mut config: Config) -> Self {
        config.response = if config.response.is_finite() {
            config.response.clamp(0.05, 2.)
        } else {
            0.35
        };
        Self {
            capture_proven: Arc::new(AtomicBool::new(false)),
            model: Arc::new(Mutex::new(Model::default())),
            clock,
            catalog,
            config,
        }
    }
    fn update(&self, session: &SessionCtx, change: impl FnOnce(&mut Session)) {
        // The bound covers both engine and Host identifiers; never keep unbounded payloads.
        if session.session_id.len() > 256 {
            return;
        }
        let mut model = lock(&self.model);
        if !model.sessions.contains_key(&session.session_id) && model.sessions.len() >= 64 {
            let oldest = model
                .sessions
                .iter()
                .min_by_key(|(_, s)| s.revision)
                .map(|(k, _)| k.clone());
            if let Some(key) = oldest {
                model.sessions.remove(&key);
            }
        }
        model.revision = model.revision.wrapping_add(1);
        let revision = model.revision;
        let entry = model
            .sessions
            .entry(session.session_id.clone())
            .or_insert_with(|| Session {
                revision,
                message: String::new(),
                foreground: false,
                approval: None,
                cursor: None,
                window: None,
                scale: 1.,
            });
        entry.revision = revision;
        change(entry);
    }
    /// Display/remove an entry into the Host's existing pending approval window.
    /// The opaque ID must be Host-generated. This never decides the request.
    /// Nonblocking memory update; no timeout or cancellation.
    pub fn approval_entry(&self, session: &SessionCtx, request: Option<String>) {
        self.update(session, |s| {
            s.approval = request.filter(|id| !id.is_empty() && id.len() <= 256)
        });
    }
    /// Snapshot the current visual state using injected time and physical monitor bounds.
    /// Pure memory work; no waits, timeout or cancellation.
    pub fn scene(&self, monitor: Rect, stopped: bool, healthy: bool) -> Scene {
        crate::scene::build(self, monitor, stopped, healthy)
    }
    /// Number of retained sessions, bounded to 64.
    pub fn session_count(&self) -> usize {
        lock(&self.model).sessions.len()
    }
    pub(crate) fn target(&self) -> Option<(Rect, f64)> {
        lock(&self.model)
            .sessions
            .values()
            .max_by_key(|s| s.revision)
            .and_then(|s| s.window.map(|frame| (frame, s.scale)))
    }
}
impl OverlayProvider for Overlay {
    fn show_target(
        &self,
        session: &SessionCtx,
        window: &WindowInfo,
        at: ScreenPoint,
        kind: CursorKind,
    ) {
        if ![
            at.x,
            at.y,
            window.frame.x,
            window.frame.y,
            window.frame.w,
            window.frame.h,
            window.scale,
        ]
        .iter()
        .all(|v| v.is_finite())
            || window.frame.w <= 0.
            || window.frame.h <= 0.
            || window.scale <= 0.
            || window.scale > 16.
            || window.frame.w > 32768.
            || window.frame.h > 32768.
            || [at.x, at.y, window.frame.x, window.frame.y]
                .iter()
                .any(|v| v.abs() > 10_000_000.)
        {
            return;
        }
        let now = self.clock.now();
        self.update(session, |s| {
            let mut spring = s
                .cursor
                .as_ref()
                .map(|c| c.0.clone())
                .unwrap_or_else(|| Spring::new(at, now, self.config.response));
            spring.retarget(at, now);
            s.cursor = Some((spring, kind, now));
            s.window = Some(window.frame);
            s.scale = window.scale;
        });
    }
    fn status(&self, session: &SessionCtx, text: &StatusText) {
        self.update(session, |s| {
            s.message = clean(&text.message);
            s.foreground = text.foreground;
        });
    }
    fn hide(&self, session: &SessionCtx) {
        lock(&self.model).sessions.remove(&session.session_id);
    }
    fn exclude_from_capture_supported(&self) -> bool {
        self.capture_proven.load(Ordering::Acquire)
    }
}
pub(crate) fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(160)
        .collect()
}
