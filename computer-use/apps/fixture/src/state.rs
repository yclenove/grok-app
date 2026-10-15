//! Pure state machine and atomic state persistence for `gcu-fixture`.
//!
//! This module has no GUI dependencies and provides:
//! - State representation adhering to test contract expectations (C03-C15, eval oracle).
//! - Redacted password handling (never writes plaintext passwords to state files).
//! - Atomic state file writing via temporary file creation and replacement.
//! - Bounded FIFO event logging (maximum 50 entries) tracking event sources.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Sequence generator for unique temporary file names during atomic writes.
static TEMP_WRITE_SEQ: AtomicU64 = AtomicU64::new(1);

/// Maximum number of recent events preserved in [`FixtureState::event_log`].
pub const MAX_EVENT_LOG_ENTRIES: usize = 50;

/// Maximum number of recent raw keyboard events preserved for test diagnostics.
pub const MAX_KEY_LOG_ENTRIES: usize = 50;

/// Number of columns in the canvas test grid.
pub const CANVAS_GRID_COLS: usize = 8;

/// Number of rows in the canvas test grid.
pub const CANVAS_GRID_ROWS: usize = 6;

/// Total number of rows in the virtualized list.
pub const VIRTUAL_LIST_TOTAL_ROWS: usize = 2000;

/// Source category for an observed UI event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// Event triggered by keyboard input or shortcut.
    Keyboard,
    /// Event triggered by mouse pointer or touch interaction.
    Mouse,
    /// Event triggered through an accessibility action.
    Accessibility,
}

impl fmt::Display for EventSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keyboard => write!(f, "keyboard"),
            Self::Mouse => write!(f, "mouse"),
            Self::Accessibility => write!(f, "accessibility"),
        }
    }
}

/// A recorded interaction event entry in the bounded event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogEntry {
    /// Source subsystem generating the event.
    pub source: EventSource,
    /// High-level action name (e.g., `counter_increment`, `text_change`).
    pub action: String,
    /// Epoch timestamp in milliseconds when the event was recorded.
    pub timestamp_ms: u64,
    /// Optional structured or human-readable event detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A recorded raw key event entry for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawKeyEvent {
    /// Key descriptor or character string.
    pub key: String,
    /// Whether this is a key press (true) or key release (false).
    pub pressed: bool,
    /// State of Ctrl modifier during the event.
    pub ctrl: bool,
    /// State of Shift modifier during the event.
    pub shift: bool,
    /// State of Alt modifier during the event.
    pub alt: bool,
    /// Epoch timestamp in milliseconds.
    pub timestamp_ms: u64,
}

/// Form sub-state structure queried by evaluation tasks and contract oracles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormState {
    /// Text value corresponding to the name input field.
    pub name: String,
    /// Boolean agreement state corresponding to the agree checkbox.
    pub agree: bool,
    /// Count of form submissions.
    pub submitted_count: u64,
}

/// Core state model for the fixture application.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct FixtureState {
    /// Single-line text input value (`name_field`).
    pub text: String,
    /// Multi-line text input value (`multiline_field`).
    pub multiline: String,
    /// In-memory password storage; explicitly skipped during serialization.
    #[serde(skip)]
    pub password_secret: String,
    /// Indicator that the password field represents a password input.
    pub is_password: bool,
    /// Character length of the password (never exposes plaintext).
    pub password_len: usize,
    /// Primary checkbox toggle state.
    pub checkbox: bool,
    /// Mirror alias of `checkbox` for oracle compatibility.
    pub checked: bool,
    /// Selected index within the radio group (0-indexed).
    pub radio: usize,
    /// Selected index within the combo-box dropdown (0-indexed).
    pub combo_selected: usize,
    /// Mirror alias of `combo_selected` for oracle compatibility.
    pub selected: usize,
    /// Current floating-point slider value.
    pub slider: f64,
    /// Mirror alias of `slider` for oracle compatibility.
    pub value: f64,
    /// Button click counter for C05 exactly-once verification.
    pub counter: u64,
    /// Half-open visible row range `(start, end)` of the 2000-row list.
    pub list_visible_range: (usize, usize),
    /// Mirror alias of `list_visible_range` for oracle compatibility.
    pub visible_range: (usize, usize),
    /// Active tab page index (0-indexed).
    pub tab: usize,
    /// Whether the modal dialog is currently open.
    pub modal_open: bool,
    /// Whether the modal dialog has ever been confirmed/closed.
    pub modal_closed: bool,
    /// Cumulative count of modal dialog confirmations/closures.
    pub modal_closed_count: u64,
    /// Cumulative count of modal dialog openings.
    pub modal_open_count: u64,
    /// Flag indicating whether draggable item was dropped on target.
    pub drag_drop_ok: bool,
    /// Mirror alias of `drag_drop_ok` for oracle compatibility.
    pub drop_ok: bool,
    /// Grid cell coordinates `(col, row)` hit by canvas click, if any.
    pub grid_hit: Option<(usize, usize)>,
    /// Cumulative count of form submission button clicks.
    pub submitted_count: u64,
    /// Cumulative count of clicks on the sensitive "Send" button.
    pub send_clicked: u64,
    /// Form sub-object matching eval oracle queries (e.g., `form.name`).
    pub form: FormState,
    /// Currently focused widget identifier, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_widget: Option<String>,
    /// Recent chronological event log bounded to [`MAX_EVENT_LOG_ENTRIES`].
    pub event_log: Vec<EventLogEntry>,
    /// Bounded trace of recent raw keyboard events with modifier states for test diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_events: Vec<RawKeyEvent>,
}

impl fmt::Debug for FixtureState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixtureState")
            .field("text", &self.text)
            .field("multiline", &self.multiline)
            .field("password_secret", &"[REDACTED]")
            .field("is_password", &self.is_password)
            .field("password_len", &self.password_len)
            .field("checkbox", &self.checkbox)
            .field("checked", &self.checked)
            .field("radio", &self.radio)
            .field("combo_selected", &self.combo_selected)
            .field("selected", &self.selected)
            .field("slider", &self.slider)
            .field("value", &self.value)
            .field("counter", &self.counter)
            .field("list_visible_range", &self.list_visible_range)
            .field("visible_range", &self.visible_range)
            .field("tab", &self.tab)
            .field("modal_open", &self.modal_open)
            .field("modal_closed", &self.modal_closed)
            .field("modal_closed_count", &self.modal_closed_count)
            .field("modal_open_count", &self.modal_open_count)
            .field("drag_drop_ok", &self.drag_drop_ok)
            .field("drop_ok", &self.drop_ok)
            .field("grid_hit", &self.grid_hit)
            .field("submitted_count", &self.submitted_count)
            .field("send_clicked", &self.send_clicked)
            .field("form", &self.form)
            .field("focused_widget", &self.focused_widget)
            .field("event_log_len", &self.event_log.len())
            .field("key_events_len", &self.key_events.len())
            .finish()
    }
}

impl Default for FixtureState {
    fn default() -> Self {
        Self {
            text: String::new(),
            multiline: String::new(),
            password_secret: String::new(),
            is_password: true,
            password_len: 0,
            checkbox: false,
            checked: false,
            radio: 0,
            combo_selected: 0,
            selected: 0,
            slider: 50.0,
            value: 50.0,
            counter: 0,
            list_visible_range: (0, 10),
            visible_range: (0, 10),
            tab: 0,
            modal_open: false,
            modal_closed: false,
            modal_closed_count: 0,
            modal_open_count: 0,
            drag_drop_ok: false,
            drop_ok: false,
            grid_hit: None,
            submitted_count: 0,
            send_clicked: 0,
            form: FormState::default(),
            focused_widget: None,
            event_log: Vec::new(),
            key_events: Vec::new(),
        }
    }
}

/// Errors occurring during state serialization and file writing.
#[derive(Debug)]
pub enum StateWriteError {
    /// File system or I/O operation error.
    Io(std::io::Error),
    /// JSON serialization failure.
    Serialization(serde_json::Error),
    /// Target destination path is invalid (e.g. is a directory).
    InvalidPath(String),
}

impl fmt::Display for StateWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error during state write: {err}"),
            Self::Serialization(err) => write!(f, "JSON serialization error: {err}"),
            Self::InvalidPath(msg) => write!(f, "Invalid state path: {msg}"),
        }
    }
}

impl std::error::Error for StateWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serialization(err) => Some(err),
            Self::InvalidPath(_) => None,
        }
    }
}

impl From<std::io::Error> for StateWriteError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for StateWriteError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err)
    }
}

impl FixtureState {
    /// Creates a new default fixture state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Synchronizes primary fields to secondary mirror fields and sub-objects.
    pub fn sync_mirrors(&mut self) {
        self.checked = self.checkbox;
        self.selected = self.combo_selected;
        self.value = self.slider;
        self.drop_ok = self.drag_drop_ok;
        self.visible_range = self.list_visible_range;
        self.is_password = true;
        self.password_len = self.password_secret.chars().count();
        self.modal_closed = self.modal_closed_count > 0 && !self.modal_open;

        self.form.name = self.text.clone();
        self.form.agree = self.checkbox;
        self.form.submitted_count = self.submitted_count;
    }

    /// Appends a new event entry, bounding log length to [`MAX_EVENT_LOG_ENTRIES`].
    pub fn log_event(
        &mut self,
        source: EventSource,
        action: impl Into<String>,
        detail: Option<String>,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_millis() as u64)
            .unwrap_or(0);

        self.event_log.push(EventLogEntry {
            source,
            action: action.into(),
            timestamp_ms,
            detail,
        });

        if self.event_log.len() > MAX_EVENT_LOG_ENTRIES {
            let overflow = self.event_log.len() - MAX_EVENT_LOG_ENTRIES;
            self.event_log.drain(0..overflow);
        }
    }

    /// Sets the single-line text value and logs the event.
    pub fn set_text(&mut self, text: String, source: EventSource) {
        let detail = Some(format!("len={}", text.len()));
        self.text = text;
        self.sync_mirrors();
        self.log_event(source, "set_text", detail);
    }

    /// Sets the multi-line text value and logs the event.
    pub fn set_multiline(&mut self, multiline: String, source: EventSource) {
        let detail = Some(format!("len={}", multiline.len()));
        self.multiline = multiline;
        self.sync_mirrors();
        self.log_event(source, "set_multiline", detail);
    }

    /// Sets the in-memory password value without leaking plaintext to disk.
    pub fn set_password(&mut self, password: &str, source: EventSource) {
        self.password_secret = password.to_string();
        self.sync_mirrors();
        let detail = Some(format!("len={}", self.password_len));
        self.log_event(source, "set_password", detail);
    }

    /// Toggles the checkbox value and logs the event.
    pub fn toggle_checkbox(&mut self, source: EventSource) {
        self.checkbox = !self.checkbox;
        self.sync_mirrors();
        let detail = Some(format!("checked={}", self.checkbox));
        self.log_event(source, "toggle_checkbox", detail);
    }

    /// Sets the checkbox value directly.
    pub fn set_checkbox(&mut self, checked: bool, source: EventSource) {
        self.checkbox = checked;
        self.sync_mirrors();
        let detail = Some(format!("checked={}", self.checkbox));
        self.log_event(source, "set_checkbox", detail);
    }

    /// Selects a radio option by index.
    pub fn set_radio(&mut self, index: usize, source: EventSource) {
        self.radio = index;
        self.sync_mirrors();
        let detail = Some(format!("radio={index}"));
        self.log_event(source, "set_radio", detail);
    }

    /// Selects a combo-box item by index.
    pub fn set_combo_selected(&mut self, index: usize, source: EventSource) {
        self.combo_selected = index;
        self.sync_mirrors();
        let detail = Some(format!("combo={index}"));
        self.log_event(source, "set_combo_selected", detail);
    }

    /// Updates the slider value.
    pub fn set_slider(&mut self, val: f64, source: EventSource) {
        self.slider = val;
        self.sync_mirrors();
        let detail = Some(format!("slider={val:.2}"));
        self.log_event(source, "set_slider", detail);
    }

    /// Increments the button click counter exactly once.
    pub fn increment_counter(&mut self, source: EventSource) {
        self.counter = self.counter.saturating_add(1);
        self.sync_mirrors();
        let detail = Some(format!("counter={}", self.counter));
        self.log_event(source, "increment_counter", detail);
    }

    /// Increments form submission count.
    pub fn submit_form(&mut self, source: EventSource) {
        self.submitted_count = self.submitted_count.saturating_add(1);
        self.sync_mirrors();
        let detail = Some(format!("submitted_count={}", self.submitted_count));
        self.log_event(source, "submit_form", detail);
    }

    /// Clicks the sensitive "Send" button (increments count with no external action).
    pub fn click_send(&mut self, source: EventSource) {
        self.send_clicked = self.send_clicked.saturating_add(1);
        self.sync_mirrors();
        let detail = Some(format!("send_clicked={}", self.send_clicked));
        self.log_event(source, "click_send", detail);
    }

    /// Opens the modal dialog.
    pub fn open_modal(&mut self, source: EventSource) {
        self.modal_open = true;
        self.modal_open_count = self.modal_open_count.saturating_add(1);
        self.sync_mirrors();
        self.log_event(source, "open_modal", None);
    }

    /// Closes the modal dialog with confirm/cancel result.
    pub fn close_modal(&mut self, confirmed: bool, source: EventSource) {
        self.modal_open = false;
        if confirmed {
            self.modal_closed_count = self.modal_closed_count.saturating_add(1);
        }
        self.sync_mirrors();
        let detail = Some(if confirmed { "confirmed" } else { "cancelled" }.to_string());
        self.log_event(source, "close_modal", detail);
    }

    /// Sets drag-and-drop status.
    pub fn set_drag_drop_ok(&mut self, ok: bool, source: EventSource) {
        self.drag_drop_ok = ok;
        self.sync_mirrors();
        let detail = Some(format!("ok={ok}"));
        self.log_event(source, "drag_drop", detail);
    }

    /// Sets the canvas grid hit coordinate `(col, row)`.
    pub fn set_grid_hit(&mut self, hit: Option<(usize, usize)>, source: EventSource) {
        self.grid_hit = hit;
        self.sync_mirrors();
        let detail = hit.map(|(c, r)| format!("col={c},row={r}"));
        self.log_event(source, "grid_hit", detail);
    }

    /// Updates visible range of the virtualized list.
    pub fn set_list_visible_range(&mut self, range: (usize, usize)) {
        self.list_visible_range = range;
        self.visible_range = range;
    }

    /// Changes active tab page.
    pub fn set_tab(&mut self, tab: usize, source: EventSource) {
        self.tab = tab;
        self.sync_mirrors();
        let detail = Some(format!("tab={tab}"));
        self.log_event(source, "set_tab", detail);
    }

    /// Updates the currently focused widget name. Returns true if the focus value changed.
    pub fn set_focused_widget(&mut self, widget: Option<String>) -> bool {
        if self.focused_widget != widget {
            self.focused_widget = widget;
            true
        } else {
            false
        }
    }

    /// Records a raw keyboard event with modifier state for test diagnostics.
    pub fn record_key(&mut self, key: String, pressed: bool, ctrl: bool, shift: bool, alt: bool) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_millis() as u64)
            .unwrap_or(0);
        self.key_events.push(RawKeyEvent {
            key,
            pressed,
            ctrl,
            shift,
            alt,
            timestamp_ms,
        });
        if self.key_events.len() > MAX_KEY_LOG_ENTRIES {
            let overflow = self.key_events.len() - MAX_KEY_LOG_ENTRIES;
            self.key_events.drain(0..overflow);
        }
    }

    /// Resets all state values to defaults.
    pub fn reset(&mut self, source: EventSource) {
        *self = Self::default();
        self.log_event(source, "reset", None);
    }

    /// Writes state atomically to the specified file path.
    pub fn write_to_file(&self, path: &Path) -> Result<(), StateWriteError> {
        write_state_atomically(self, path)
    }

    /// Writes state to the file path configured in `$GCU_FIXTURE_STATE`, if set.
    pub fn maybe_persist(&self) -> Result<Option<PathBuf>, StateWriteError> {
        if let Ok(var) = std::env::var("GCU_FIXTURE_STATE") {
            let trimmed = var.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                self.write_to_file(&path)?;
                return Ok(Some(path));
            }
        }
        Ok(None)
    }
}

/// Writes `state` atomically to `destination_path` using a temporary file in the same directory.
pub fn write_state_atomically(
    state: &FixtureState,
    destination_path: &Path,
) -> Result<(), StateWriteError> {
    if destination_path.is_dir() {
        return Err(StateWriteError::InvalidPath(format!(
            "destination is a directory: {}",
            destination_path.display()
        )));
    }

    let parent_dir = match destination_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    fs::create_dir_all(parent_dir).map_err(StateWriteError::Io)?;

    let seq = TEMP_WRITE_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let thread_str = format!("{:?}", std::thread::current().id());
    let thread_clean: String = thread_str.chars().filter(|c| c.is_alphanumeric()).collect();
    let file_stem = destination_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("fixture_state");
    let temp_name = format!(".{file_stem}.tmp.{pid}_{thread_clean}_{seq}");
    let temp_path = parent_dir.join(temp_name);

    let serialized_bytes =
        serde_json::to_vec_pretty(state).map_err(StateWriteError::Serialization)?;

    let write_res = (|| -> Result<(), std::io::Error> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&serialized_bytes)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(StateWriteError::Io(err));
    }

    if let Err(err) = fs::rename(&temp_path, destination_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(StateWriteError::Io(err));
    }

    Ok(())
}

/// Pure helper computing grid cell index `(col, row)` from a click offset.
pub fn compute_grid_cell(
    click_pos: (f32, f32),
    grid_origin: (f32, f32),
    cell_size: (f32, f32),
    dimensions: (usize, usize),
) -> Option<(usize, usize)> {
    let (click_x, click_y) = click_pos;
    let (grid_min_x, grid_min_y) = grid_origin;
    let (cell_w, cell_h) = cell_size;
    let (cols, rows) = dimensions;

    if cell_w <= 0.0 || cell_h <= 0.0 {
        return None;
    }
    if click_x < grid_min_x || click_y < grid_min_y {
        return None;
    }
    let offset_x = click_x - grid_min_x;
    let offset_y = click_y - grid_min_y;
    let col = (offset_x / cell_w).floor() as usize;
    let row = (offset_y / cell_h).floor() as usize;
    if col < cols && row < rows {
        Some((col, row))
    } else {
        None
    }
}

/// Pure helper computing virtual list visible row range `(start, end)`.
pub fn compute_virtual_list_rows(
    scroll_offset_y: f32,
    viewport_height: f32,
    row_height: f32,
    total_rows: usize,
) -> (usize, usize) {
    if row_height <= 0.0 || total_rows == 0 {
        return (0, 0);
    }
    let start = (scroll_offset_y / row_height).floor().max(0.0) as usize;
    let end = ((scroll_offset_y + viewport_height) / row_height)
        .ceil()
        .max(0.0) as usize;
    let clamped_start = start.min(total_rows);
    let clamped_end = end.min(total_rows).max(clamped_start);
    (clamped_start, clamped_end)
}

/// Pure helper testing whether a drop coordinate falls within a target bounding box.
pub fn hit_test_rect(pt: (f32, f32), min: (f32, f32), max: (f32, f32)) -> bool {
    pt.0 >= min.0 && pt.0 <= max.0 && pt.1 >= min.1 && pt.1 <= max.1
}
