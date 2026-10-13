//! Driver-produced data. Frames use window-relative physical pixels, not logical points.

pub use gcu_protocol::Rect;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::{any::Any, fmt, sync::Arc};

/// Normalized role; unknown platform roles retain their native name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Window accessibility role.
    Window,
    /// Dialog accessibility role.
    Dialog,
    /// Sheet accessibility role.
    Sheet,
    /// Popover accessibility role.
    Popover,
    /// Menu accessibility role.
    Menu,
    /// MenuItem accessibility role.
    MenuItem,
    /// MenuBar accessibility role.
    MenuBar,
    /// Toolbar accessibility role.
    Toolbar,
    /// Button accessibility role.
    Button,
    /// Link accessibility role.
    Link,
    /// CheckBox accessibility role.
    CheckBox,
    /// Radio accessibility role.
    Radio,
    /// Switch accessibility role.
    Switch,
    /// TextField accessibility role.
    TextField,
    /// SearchField accessibility role.
    SearchField,
    /// TextArea accessibility role.
    TextArea,
    /// ComboBox accessibility role.
    ComboBox,
    /// PopUpButton accessibility role.
    PopUpButton,
    /// Slider accessibility role.
    Slider,
    /// Stepper accessibility role.
    Stepper,
    /// ProgressBar accessibility role.
    ProgressBar,
    /// List accessibility role.
    List,
    /// ListItem accessibility role.
    ListItem,
    /// Table accessibility role.
    Table,
    /// Row accessibility role.
    Row,
    /// Cell accessibility role.
    Cell,
    /// Column accessibility role.
    Column,
    /// Outline accessibility role.
    Outline,
    /// Tree accessibility role.
    Tree,
    /// TreeItem accessibility role.
    TreeItem,
    /// Tab accessibility role.
    Tab,
    /// TabList accessibility role.
    TabList,
    /// ScrollArea accessibility role.
    ScrollArea,
    /// ScrollBar accessibility role.
    ScrollBar,
    /// Group accessibility role.
    Group,
    /// SplitGroup accessibility role.
    SplitGroup,
    /// Image accessibility role.
    Image,
    /// StaticText accessibility role.
    StaticText,
    /// Heading accessibility role.
    Heading,
    /// WebArea accessibility role.
    WebArea,
    /// Document accessibility role.
    Document,
    /// Canvas accessibility role.
    Canvas,
    /// Unknown accessibility role.
    Unknown,
}

/// Advertised normalized semantic action; arbitrary native names remain opaque.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeAction {
    /// Press semantic action.
    Press,
    /// ShowMenu semantic action.
    ShowMenu,
    /// Increment semantic action.
    Increment,
    /// Decrement semantic action.
    Decrement,
    /// Confirm semantic action.
    Confirm,
    /// Cancel semantic action.
    Cancel,
    /// Raise semantic action.
    Raise,
    /// Expand semantic action.
    Expand,
    /// Collapse semantic action.
    Collapse,
    /// ScrollIntoView semantic action.
    ScrollIntoView,
    /// ScrollUp semantic action.
    ScrollUp,
    /// ScrollDown semantic action.
    ScrollDown,
    /// ScrollLeft semantic action.
    ScrollLeft,
    /// ScrollRight semantic action.
    ScrollRight,
    /// Select semantic action.
    Select,
    /// Toggle semantic action.
    Toggle,
    /// Driver-specific action name.
    Native(SmolStr),
}

bitflags::bitflags! {
    /// Accessibility states, preserving combinations without allocating strings.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct States: u32 {
        /// enabled state.
        const ENABLED = 1 << 0;
        /// focused state.
        const FOCUSED = 1 << 1;
        /// focusable state.
        const FOCUSABLE = 1 << 2;
        /// selected state.
        const SELECTED = 1 << 3;
        /// selectable state.
        const SELECTABLE = 1 << 4;
        /// expanded state.
        const EXPANDED = 1 << 5;
        /// expandable state.
        const EXPANDABLE = 1 << 6;
        /// checked state.
        const CHECKED = 1 << 7;
        /// mixed state.
        const MIXED = 1 << 8;
        /// editable state.
        const EDITABLE = 1 << 9;
        /// settable state.
        const SETTABLE = 1 << 10;
        /// password state.
        const PASSWORD = 1 << 11;
        /// offscreen state.
        const OFFSCREEN = 1 << 12;
        /// modal state.
        const MODAL = 1 << 13;
        /// busy state.
        const BUSY = 1 << 14;
        /// required state.
        const REQUIRED = 1 << 15;
    }
}

/// Portable node value. Password values must be redacted by the observation layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeValue {
    /// Editable or static text.
    Text(String),
    /// Numeric value.
    Number(f64),
    /// Boolean value.
    Bool(bool),
    /// Bounded numeric control.
    Range {
        /// Current value.
        value: f64,
        /// Minimum value.
        min: f64,
        /// Maximum value.
        max: f64,
    },
}

/// Unicode-scalar range, translated into native units by each driver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    /// First Unicode scalar index.
    pub start: usize,
    /// Number of Unicode scalars.
    pub length: usize,
}

/// Optional text details; long text is bounded by TreeOpts.text_limit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TextExtras {
    /// Selected character range.
    pub selected: Option<TextRange>,
    /// Visible character range.
    pub visible: Option<TextRange>,
    /// Extended text, when exposed by the native API.
    pub text: Option<String>,
}

/// Process-local opaque handle. Never serialized or logged with its payload.
#[derive(Clone)]
pub struct NativeHandle(Arc<dyn Any + Send + Sync>);

impl NativeHandle {
    /// Wrap a driver-private handle; no IO or allocation beyond the Arc.
    pub fn new<T: Any + Send + Sync>(value: T) -> Self {
        Self(Arc::new(value))
    }
    /// Wrap an existing shared driver handle.
    pub fn from_arc(value: Arc<dyn Any + Send + Sync>) -> Self {
        Self(value)
    }
    /// Borrow the payload if its concrete type matches.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
    /// Share the payload if its concrete type matches.
    pub fn downcast<T: Any + Send + Sync>(&self) -> Option<Arc<T>> {
        self.0.clone().downcast().ok()
    }
    /// Compare allocation identity, without inspecting the payload.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for NativeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeHandle(..)")
    }
}

/// Deterministic, non-cryptographic node identity; not an authorization token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

/// Unified raw tree node. Drivers preserve structure; core later budgets and renders it.
#[derive(Clone)]
pub struct RawNode {
    /// Normalized role.
    pub role: Role,
    /// Original platform role.
    pub native_role: SmolStr,
    /// Original platform subrole.
    pub subrole: Option<SmolStr>,
    /// Accessible name.
    pub name: Option<String>,
    /// Current value.
    pub value: Option<NodeValue>,
    /// Accessible description.
    pub description: Option<String>,
    /// Input placeholder.
    pub placeholder: Option<String>,
    /// Help text.
    pub help: Option<String>,
    /// Stable native identifier, if exposed.
    pub identifier: Option<String>,
    /// State flags.
    pub states: States,
    /// Advertised actions.
    pub actions: SmallVec<[NodeAction; 4]>,
    /// Window-relative physical-pixel frame.
    pub frame: Option<Rect>,
    /// Optional native text details.
    pub text_extras: Option<TextExtras>,
    /// Link or document URL.
    pub url: Option<String>,
    /// Unmodified native child order.
    pub children: Vec<RawNode>,
    /// Driver-private handle, valid only for its originating driver.
    pub handle: NativeHandle,
    /// Identity excluding volatile value and state.
    pub fingerprint: Fingerprint,
}

impl fmt::Debug for RawNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawNode")
            .field("role", &self.role)
            .field("states", &self.states)
            .field("frame", &self.frame)
            .field("fingerprint", &self.fingerprint)
            .field("children", &self.children)
            .finish_non_exhaustive()
    }
}

/// Raw window observation; canvas-only windows legitimately have no root.
#[derive(Debug, Clone)]
pub struct RawTree {
    /// Accessibility root, if available.
    pub root: Option<RawNode>,
}

/// Geometry used to detect movement, resizing, or scale changes before input.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    /// Window frame in screen physical pixels.
    pub frame: Rect,
    /// Logical-to-physical display scale.
    pub scale: f64,
}

/// Window-relative physical-pixel point (after screenshot coordinate conversion).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowPoint {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

bitflags::bitflags! {
    /// Core observation annotations, separate from platform accessibility states.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct NodeFlags: u16 {
        /// Best-effort terminal/console recognition from native role or name.
        /// Role has no Terminal/Console variants; this also propagates to descendants
        /// so flattening cannot erase an embedded execution surface. Not a safety proof.
        const TERMINAL_LIKE = 1 << 0;
        /// Node is on the focused element's ancestor chain (including the element).
        const FOCUS_PATH = 1 << 1;
        /// Node is on a modal popup's ancestor chain (including the popup).
        const MODAL_PATH = 1 << 2;
        /// Node is inside a modal dialog, sheet, or native alert.
        const MODAL_CONTENT = 1 << 3;
        /// Native title bar or its descendants.
        const TITLE_BAR = 1 << 4;
        /// Background node kept as a one-level summary while a modal is present.
        const SUMMARY = 1 << 5;
    }
}
