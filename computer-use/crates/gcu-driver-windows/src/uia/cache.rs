//! Cache plans and owned property bags. No COM interface/pattern objects.
use gcu_core::model::{NodeValue, Rect};

/// Maximum children fetched per portable page, including a one-node lookahead.
pub const BATCH_SIZE: usize = 64;
/// All required UIA property IDs, including pattern availability and state.
/// Value properties are cached, but password values are never extracted.
/// No pattern IDs are added to the native cache request.
pub const PROPERTY_IDS: &[i32] = &[
    30001, 30003, 30005, 30008, 30009, 30010, 30011, 30012, 30013, 30019, 30022, 30024, 30025,
    30028, 30031, 30033, 30034, 30035, 30036, 30040, 30041, 30043, 30045, 30046, 30047, 30048,
    30049, 30050, 30070, 30077, 30079, 30086, 30090, 30101,
];

/// Pattern availability, never a cached pattern interface.
#[derive(Clone, Debug, Default)]
pub struct Patterns {
    /// Invoke.
    pub invoke: bool,
    /// Toggle.
    pub toggle: bool,
    /// SelectionItem.
    pub selection_item: bool,
    /// ExpandCollapse.
    pub expand_collapse: bool,
    /// Value.
    pub value: bool,
    /// RangeValue.
    pub range_value: bool,
    /// Scroll.
    pub scroll: bool,
    /// ScrollItem.
    pub scroll_item: bool,
    /// Text (used to distinguish document/text roles).
    pub text: bool,
    /// LegacyIAccessible.
    pub legacy: bool,
}

/// One cached element's owned metadata; optional unsupported properties stay absent.
/// `partial` reports unreliable properties to the completeness pipeline.
#[derive(Clone, Debug)]
pub struct Properties {
    /// UIA_ControlType integer. Unknown IDs are preserved.
    pub control_type: i32,
    /// UIA_AriaRole, when exposed by Chromium/Electron.
    pub aria_role: Option<String>,
    /// Name.
    pub name: Option<String>,
    /// AutomationId.
    pub identifier: Option<String>,
    /// ClassName (also an execution-surface hint).
    pub class_name: Option<String>,
    /// FrameworkId, e.g. Chrome (no browser-specific setting changes).
    pub framework: Option<String>,
    /// HelpText.
    pub help: Option<String>,
    /// BoundingRectangle, in screen physical pixels.
    pub frame: Option<Rect>,
    /// IsEnabled.
    pub enabled: bool,
    /// IsKeyboardFocusable.
    pub focusable: bool,
    /// HasKeyboardFocus.
    pub focused: bool,
    /// IsOffscreen.
    pub offscreen: bool,
    /// IsPassword. Failure to read this must set `partial` and suppress value.
    pub password: bool,
    /// IsRequiredForForm.
    pub required: bool,
    /// WindowIsModal.
    pub modal: bool,
    /// SelectionItemIsSelected.
    pub selected: bool,
    /// ToggleState: 0 off, 1 on, 2 indeterminate.
    pub toggle_state: Option<i32>,
    /// ExpandCollapseState: 0 collapsed, 1 expanded, 2 partially expanded, 3 leaf.
    pub expand_state: Option<i32>,
    /// ValueIsReadOnly / RangeValueIsReadOnly; absent means not proven writable.
    pub read_only: Option<bool>,
    /// Value/RangeValue metadata. Never extracted for password nodes.
    pub value: Option<NodeValue>,
    /// Availability properties read in the same cache request.
    pub patterns: Patterns,
    /// A property read failed or had an unexpected type.
    pub partial: bool,
}
impl Default for Properties {
    fn default() -> Self {
        Self {
            control_type: 50025,
            aria_role: None,
            name: None,
            identifier: None,
            class_name: None,
            framework: None,
            help: None,
            frame: None,
            enabled: true,
            focusable: false,
            focused: false,
            offscreen: false,
            password: false,
            required: false,
            modal: false,
            selected: false,
            toggle_state: None,
            expand_state: None,
            read_only: None,
            value: None,
            patterns: Patterns::default(),
            partial: false,
        }
    }
}
