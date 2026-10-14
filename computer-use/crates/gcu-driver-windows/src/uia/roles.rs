//! UIA ControlType mapping. Unknown IDs remain Unknown with their native ID.
use gcu_core::model::Role;

/// All documented UIA control types, in ID order, for table-driven contracts.
pub const CONTROL_ROLES: &[(i32, Role)] = &[
    (50000, Role::Button),
    (50001, Role::Group),
    (50002, Role::CheckBox),
    (50003, Role::ComboBox),
    (50004, Role::TextField),
    (50005, Role::Link),
    (50006, Role::Image),
    (50007, Role::ListItem),
    (50008, Role::List),
    (50009, Role::Menu),
    (50010, Role::MenuBar),
    (50011, Role::MenuItem),
    (50012, Role::ProgressBar),
    (50013, Role::Radio),
    (50014, Role::ScrollBar),
    (50015, Role::Slider),
    (50016, Role::Stepper),
    (50017, Role::Group),
    (50018, Role::TabList),
    (50019, Role::Tab),
    (50020, Role::StaticText),
    (50021, Role::Toolbar),
    (50022, Role::Popover),
    (50023, Role::Tree),
    (50024, Role::TreeItem),
    (50025, Role::Unknown),
    (50026, Role::Group),
    (50027, Role::Group),
    (50028, Role::Table),
    (50029, Role::Row),
    (50030, Role::Document),
    (50031, Role::SplitGroup),
    (50032, Role::Window),
    (50033, Role::Group),
    (50034, Role::Group),
    (50035, Role::Column),
    (50036, Role::Table),
    (50037, Role::Group),
    (50038, Role::Unknown),
    (50039, Role::Group),
    (50040, Role::Group),
];

/// Map a native control type and optional ARIA role. ARIA refines only native
/// document/custom/pane/edit roles; it cannot turn a native button into a window.
/// Synchronous and allocation-free, with no cancellation or IO.
pub fn map_role(control_type: i32, aria_role: Option<&str>, modal: bool) -> Role {
    if control_type == 50032 && modal {
        return Role::Dialog;
    }
    if matches!(control_type, 50004 | 50025 | 50030 | 50033) {
        match aria_role {
            Some("document" | "webarea") if control_type == 50030 => return Role::WebArea,
            Some("textbox") if control_type == 50030 => return Role::TextArea,
            Some("searchbox") => return Role::SearchField,
            Some("heading") => return Role::Heading,
            Some("dialog" | "alertdialog") => return Role::Dialog,
            Some("cell" | "gridcell") => return Role::Cell,
            Some("row") => return Role::Row,
            _ => {}
        }
    }
    CONTROL_ROLES
        .iter()
        .find(|(id, _)| *id == control_type)
        .map_or(Role::Unknown, |(_, role)| *role)
}
