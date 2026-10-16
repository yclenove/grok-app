//! Case table and skip decisions.
//!
//! Shared ids reuse [`gcu_core::testing::CASES`]. Scope rows keep that table's
//! skip text. Capability skips use the same `capability:` / `permission_missing:`
//! strings the shared [`gcu_core::testing::Requirement`] documents.

use gcu_core::testing::{Case, CASES};
use gcu_driver_macos::input::{apply_probe, Probe};
use gcu_protocol::capabilities::{
    BackgroundOperation, Capabilities, CapabilityLimits, DeliveryCapabilities,
    ForegroundCapability, ObserveCapabilities, Platform,
};

/// One finished row in the JSON archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseResult {
    /// Quality-gate id.
    pub id: &'static str,
    /// Short description stored in the archive.
    pub description: &'static str,
    /// `passed`, `skipped`, or `failed`.
    pub outcome: &'static str,
    /// Skip reason or failure text.
    pub reason: Option<String>,
}

impl CaseResult {
    /// A skip with a stable reason.
    pub fn skipped(id: &'static str, description: &'static str, reason: &str) -> Self {
        Self {
            id,
            description,
            outcome: "skipped",
            reason: Some(reason.to_string()),
        }
    }

    /// A passing row.
    pub fn passed(id: &'static str, description: &'static str) -> Self {
        Self {
            id,
            description,
            outcome: "passed",
            reason: None,
        }
    }

    /// A failure. The reason is the assertion text plus the last oracle state.
    pub fn failed(id: &'static str, description: &'static str, reason: String) -> Self {
        Self {
            id,
            description,
            outcome: "failed",
            reason: Some(reason),
        }
    }
}

/// How a row is executed.
#[derive(Clone, Copy, Debug)]
pub enum CaseKind {
    /// Portable body. Still skipped when `need` is unmet.
    Portable {
        /// Capability or permission the body needs.
        need: Need,
    },
    /// Real macOS session. Skipped without a fixture when `needs_fixture`.
    Live {
        /// Capability or permission the body needs.
        need: Need,
        /// The gcu-fixture process must already be discoverable.
        needs_fixture: bool,
    },
    /// Registered scope skip. The text matches the shared table when the id exists there.
    Scope(&'static str),
}

/// Prerequisite checked against the capability table and the session probe.
#[derive(Clone, Copy, Debug)]
pub enum Need {
    /// No capability gate.
    None,
    /// Accessibility tree.
    Tree,
    /// Window screenshot.
    Screenshot,
    /// Semantic accessibility actions.
    Accessibility,
    /// Field-51 background pointer.
    TargetedPointer,
    /// Field-51 background keys.
    TargetedKeys,
    /// Overlay pixels are excluded from capture.
    Overlay,
}

/// One contract row.
pub struct CaseRow {
    /// Quality-gate or macOS-specific id.
    pub id: &'static str,
    /// Archive description.
    pub description: &'static str,
    /// Execution kind.
    pub kind: CaseKind,
}

const C17: &str =
    "scope: ActionRouter stale_geometry + fresh state (P0-08); Driver has no snapshot ID";
const C24: &str =
    "scope: TreePipeline + Renderer budget/truncation (P0-06); raw reads must preserve 2000 rows";
const C25: &str = "scope: ActionRouter pixel hash stale_screenshot gate (P0-08); Driver trait has no pixel hash check";

/// C01–C27 plus the macOS rows from ADR 0006.
pub const ROWS: &[CaseRow] = &[
    CaseRow {
        id: "C01",
        description: "list_apps and resolve name/id/path",
        kind: CaseKind::Live {
            need: Need::None,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C02",
        description: "launch does not activate the foreground",
        kind: CaseKind::Live {
            need: Need::None,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C03",
        description: "fixture tree roles",
        kind: CaseKind::Live {
            need: Need::Tree,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C04",
        description: "screenshot physical size and marker pixel",
        kind: CaseKind::Live {
            need: Need::Screenshot,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C05",
        description: "button press increments the counter once",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C06",
        description: "pixel click hits the canvas grid",
        kind: CaseKind::Live {
            need: Need::TargetedPointer,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C07",
        description: "set_value replaces text",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C08",
        description: "type_text Unicode, ZWJ, and newline",
        kind: CaseKind::Live {
            need: Need::TargetedKeys,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C09",
        description: "cmd+a then BackSpace clears the field",
        kind: CaseKind::Live {
            need: Need::TargetedKeys,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C10",
        description: "checkbox toggle",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C11",
        description: "combo box selects the third item",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C12",
        description: "slider set_value",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C13",
        description: "list scroll updates the visible range",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C14",
        description: "modal open, priority, and confirm",
        kind: CaseKind::Live {
            need: Need::Accessibility,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C15",
        description: "drag the source onto the target",
        kind: CaseKind::Live {
            need: Need::TargetedPointer,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C16",
        description: "background press leaves the other app in front",
        kind: CaseKind::Live {
            need: Need::TargetedPointer,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C17",
        description: "stale geometry gate",
        kind: CaseKind::Scope(C17),
    },
    CaseRow {
        id: "C18",
        description: "deleted handle is stale and has no effect",
        kind: CaseKind::Portable {
            need: Need::Accessibility,
        },
    },
    CaseRow {
        id: "C19",
        description: "closed window is window_gone",
        kind: CaseKind::Portable {
            need: Need::TargetedPointer,
        },
    },
    CaseRow {
        id: "C20",
        description: "password text and screenshot redaction",
        kind: CaseKind::Portable { need: Need::None },
    },
    CaseRow {
        id: "C21",
        description: "overlay excluded from capture",
        kind: CaseKind::Live {
            need: Need::Overlay,
            needs_fixture: false,
        },
    },
    CaseRow {
        id: "C22",
        description: "uncertain action is not replayed",
        kind: CaseKind::Portable {
            need: Need::Accessibility,
        },
    },
    CaseRow {
        id: "C23",
        description: "release_all_input then lowercase typing",
        kind: CaseKind::Live {
            need: Need::TargetedKeys,
            needs_fixture: true,
        },
    },
    CaseRow {
        id: "C24",
        description: "2000-row budget stays in the pipeline",
        kind: CaseKind::Scope(C24),
    },
    CaseRow {
        id: "C25",
        description: "pixel-hash stale screenshot gate",
        kind: CaseKind::Scope(C25),
    },
    CaseRow {
        id: "C26",
        description: "IDE and terminal execution surfaces are marked",
        kind: CaseKind::Portable { need: Need::None },
    },
    CaseRow {
        id: "C27",
        description: "Stop latch, release budget, and hotkey failure",
        kind: CaseKind::Portable { need: Need::None },
    },
    CaseRow {
        id: "M-TCC",
        description: "missing TCC is permission_missing",
        kind: CaseKind::Portable { need: Need::None },
    },
    CaseRow {
        id: "M-CHROMIUM",
        description: "Chromium background click is unavailable and not upgraded",
        kind: CaseKind::Portable { need: Need::None },
    },
    CaseRow {
        id: "M-FIELD51",
        description: "field 51 delivery is checked for actual arrival",
        kind: CaseKind::Portable { need: Need::None },
    },
];

/// Session facts the skip decision may consult. Flags must agree with `caps`.
#[derive(Clone, Debug)]
pub struct Facts {
    /// Accessibility TCC.
    pub accessibility: bool,
    /// Screen Recording TCC.
    pub screen_recording: bool,
    /// [`Probe::background_ready`].
    pub field_51_ready: bool,
    /// gcu-fixture has a window.
    pub fixture_running: bool,
}

/// Skip reason, or `None` when the body should run.
pub fn skip_reason(row: &CaseRow, caps: &Capabilities, facts: &Facts) -> Option<&'static str> {
    match row.kind {
        CaseKind::Scope(reason) => Some(reason),
        CaseKind::Portable { need } | CaseKind::Live { need, .. } => {
            if let Some(reason) = need_skip(need, caps, facts) {
                return Some(reason);
            }
            if let CaseKind::Live { needs_fixture, .. } = row.kind {
                if needs_fixture && !facts.fixture_running {
                    return Some("gcu-fixture is not running");
                }
            }
            None
        }
    }
}

fn need_skip(need: Need, caps: &Capabilities, facts: &Facts) -> Option<&'static str> {
    match need {
        Need::None => None,
        Need::Tree => {
            if !caps.observe.tree {
                Some("capability: observe.tree=false")
            } else if !facts.accessibility {
                Some("permission_missing: accessibility")
            } else {
                None
            }
        }
        Need::Screenshot => {
            if !caps.observe.screenshot {
                Some("capability: observe.screenshot=false")
            } else if !facts.screen_recording {
                Some("permission_missing: screen_recording")
            } else {
                None
            }
        }
        Need::Accessibility => {
            if !caps.delivery.accessibility {
                Some("capability: delivery.accessibility=false")
            } else if !facts.accessibility {
                Some("permission_missing: accessibility")
            } else {
                None
            }
        }
        Need::TargetedPointer => {
            if !caps.delivery.targeted_pointer || !facts.field_51_ready {
                Some("capability: delivery.targeted_pointer=false")
            } else {
                None
            }
        }
        Need::TargetedKeys => {
            if !caps.delivery.targeted_keys || !facts.field_51_ready {
                Some("capability: delivery.targeted_keys=false")
            } else {
                None
            }
        }
        Need::Overlay => {
            if !caps.overlay_excluded_from_capture {
                Some("capability: overlay_excluded_from_capture=false")
            } else {
                None
            }
        }
    }
}

/// Find a row by id.
pub fn row(id: &str) -> &'static CaseRow {
    ROWS.iter().find(|row| row.id == id).expect("row")
}

/// Capabilities before a field-51 probe. Tree, screenshot, and AX delivery are on.
pub fn base_capabilities() -> Capabilities {
    Capabilities {
        platform: Platform::Macos,
        version: "macOS contract".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: true,
        },
        delivery: DeliveryCapabilities {
            accessibility: true,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::Partial,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: Default::default(),
        limits: CapabilityLimits {
            max_tree_nodes: 5000,
            max_image_edge: 2560,
            type_text_max: 0,
        },
    }
}

/// Facts implied by `probe`. Chromium background stays off inside [`apply_probe`].
pub fn facts_from_probe(probe: &Probe, fixture_running: bool) -> Facts {
    Facts {
        accessibility: probe.accessibility,
        screen_recording: true,
        field_51_ready: probe.background_ready(),
        fixture_running,
    }
}

/// Shared scope strings are copied, not reworded.
pub fn assert_shared_scope_text() {
    assert_eq!(shared_skip("C17"), Some(C17));
    assert_eq!(shared_skip("C24"), Some(C24));
    assert_eq!(row("C17").kind_reason(), Some(C17));
    assert_eq!(row("C24").kind_reason(), Some(C24));
    assert!(shared_skip("C04-native").is_some());
    assert!(shared_skip("C23-native").is_some());
    let mut caps = base_capabilities();
    caps.observe.tree = false;
    let case = shared("C03");
    assert_eq!(
        case.skip_reason(&caps),
        Some("capability: observe.tree=false")
    );
    assert_eq!(
        skip_reason(row("C03"), &caps, &granted(false)),
        Some("capability: observe.tree=false")
    );
}

/// Background AppKit rows follow the probe. Chromium is not skipped by that flag.
pub fn assert_background_skip_matches_probe() {
    let ready = Probe::verified_os();
    let mut caps = base_capabilities();
    apply_probe(&mut caps, &ready);
    let facts = facts_from_probe(&ready, true);
    assert!(caps.delivery.targeted_pointer);
    assert_eq!(caps.background_operation, BackgroundOperation::Partial);
    assert_eq!(skip_reason(row("C16"), &caps, &facts), None);
    assert_eq!(skip_reason(row("M-CHROMIUM"), &caps, &facts), None);

    let mut blocked = ready.clone();
    blocked.field_51 = false;
    apply_probe(&mut caps, &blocked);
    let facts = facts_from_probe(&blocked, true);
    assert!(!caps.delivery.targeted_pointer);
    assert!(!facts.field_51_ready);
    assert_eq!(
        skip_reason(row("C16"), &caps, &facts),
        Some("capability: delivery.targeted_pointer=false")
    );
    assert_eq!(
        skip_reason(row("C06"), &caps, &facts),
        Some("capability: delivery.targeted_pointer=false")
    );
    assert_eq!(skip_reason(row("M-CHROMIUM"), &caps, &facts), None);
    assert_ne!(caps.background_operation, BackgroundOperation::Full);
}

/// A missing TCC grant skips the rows that need it, and names `permission_missing`.
pub fn assert_permission_skip_matches_flags() {
    let mut caps = base_capabilities();
    apply_probe(&mut caps, &Probe::verified_os());
    let mut facts = facts_from_probe(&Probe::verified_os(), true);
    facts.accessibility = false;
    facts.screen_recording = false;
    assert_eq!(
        skip_reason(row("C03"), &caps, &facts),
        Some("permission_missing: accessibility")
    );
    assert_eq!(
        skip_reason(row("C05"), &caps, &facts),
        Some("permission_missing: accessibility")
    );
    assert_eq!(
        skip_reason(row("C04"), &caps, &facts),
        Some("permission_missing: screen_recording")
    );
    assert_eq!(skip_reason(row("M-TCC"), &caps, &facts), None);
}

/// Overlay skip tracks the declared capability and does not claim exclusion.
pub fn assert_overlay_skip_matches_capability() {
    let caps = base_capabilities();
    let facts = granted(true);
    assert!(!caps.overlay_excluded_from_capture);
    assert_eq!(
        skip_reason(row("C21"), &caps, &facts),
        Some("capability: overlay_excluded_from_capture=false")
    );
}

fn granted(fixture_running: bool) -> Facts {
    Facts {
        accessibility: true,
        screen_recording: true,
        field_51_ready: false,
        fixture_running,
    }
}

fn shared(id: &str) -> &'static Case {
    CASES
        .iter()
        .find(|case| case.id == id)
        .expect("shared case")
}

fn shared_skip(id: &str) -> Option<&'static str> {
    shared(id).skip
}

impl CaseRow {
    fn kind_reason(&self) -> Option<&'static str> {
        match self.kind {
            CaseKind::Scope(reason) => Some(reason),
            _ => None,
        }
    }
}
