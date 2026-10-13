//! Table-driven AX role and subrole mapping.
//!
//! The AX role string is kept on `native_role`. The subrole string is kept
//! separately. Strings that are not in the table stay `Role::Unknown`.

use gcu_core::model::Role;
use smol_str::SmolStr;

/// One known AX role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRow {
    /// AX role string, for example `AXButton`.
    pub ax: &'static str,
    /// Normalized role.
    pub role: Role,
    /// `AXSecureTextField` and its equivalents.
    pub password: bool,
    /// Sheet and dialog roles start modal.
    pub modal: bool,
}

/// Mapped role plus the native strings and the password/modal flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappedAxRole {
    /// Normalized role after subrole overrides.
    pub role: Role,
    /// Original AX role string.
    pub native_role: SmolStr,
    /// Original AX subrole, when present.
    pub subrole: Option<SmolStr>,
    /// Password field. The tree redacts the value.
    pub password: bool,
    /// Modal sheet or dialog.
    pub modal: bool,
}

/// Known AX roles. Subrole overrides are applied in [`map_ax_role`].
pub fn role_table() -> &'static [RoleRow] {
    ROLE_TABLE
}

const ROLE_TABLE: &[RoleRow] = &[
    row("AXButton", Role::Button, false, false),
    row("AXCheckBox", Role::CheckBox, false, false),
    row("AXRadioButton", Role::Radio, false, false),
    row("AXTextField", Role::TextField, false, false),
    row("AXTextArea", Role::TextArea, false, false),
    row("AXSecureTextField", Role::TextField, true, false),
    row("AXComboBox", Role::ComboBox, false, false),
    row("AXPopUpButton", Role::PopUpButton, false, false),
    row("AXSlider", Role::Slider, false, false),
    row("AXMenuItem", Role::MenuItem, false, false),
    row("AXMenuBar", Role::MenuBar, false, false),
    row("AXRow", Role::Row, false, false),
    row("AXCell", Role::Cell, false, false),
    row("AXTable", Role::Table, false, false),
    row("AXOutline", Role::Outline, false, false),
    row("AXTabGroup", Role::TabList, false, false),
    row("AXWebArea", Role::WebArea, false, false),
    row("AXWindow", Role::Window, false, false),
    row("AXSheet", Role::Sheet, false, true),
    row("AXScrollArea", Role::ScrollArea, false, false),
    row("AXStaticText", Role::StaticText, false, false),
    row("AXImage", Role::Image, false, false),
    row("AXLink", Role::Link, false, false),
    row("AXGroup", Role::Group, false, false),
    row("AXSplitGroup", Role::SplitGroup, false, false),
    row("AXToolbar", Role::Toolbar, false, false),
    row("AXList", Role::List, false, false),
    row("AXColumn", Role::Column, false, false),
    row("AXHeading", Role::Heading, false, false),
    row("AXSearchField", Role::SearchField, false, false),
    row("AXSwitch", Role::Switch, false, false),
    row("AXRadioGroup", Role::Group, false, false),
    row("AXProgressIndicator", Role::ProgressBar, false, false),
    row("AXBusyIndicator", Role::ProgressBar, false, false),
    row("AXIncrementor", Role::Stepper, false, false),
    row("AXPopover", Role::Popover, false, false),
    row("AXHelpTag", Role::Popover, false, false),
    row("AXBrowser", Role::WebArea, false, false),
    row("AXGrid", Role::Table, false, false),
    row("AXDisclosureTriangle", Role::Button, false, false),
    row("AXValueIndicator", Role::Slider, false, false),
    row("AXLevelIndicator", Role::ProgressBar, false, false),
    row("AXLayoutArea", Role::Group, false, false),
    row("AXLayoutItem", Role::Group, false, false),
    row("AXDateField", Role::TextField, false, false),
    row("AXTimeField", Role::TextField, false, false),
    row("AXDockItem", Role::Button, false, false),
    row("AXTab", Role::Tab, false, false),
    row("AXDocument", Role::Document, false, false),
    row("AXDrawer", Role::Group, false, false),
    row("AXRelevanceIndicator", Role::ProgressBar, false, false),
    row("AXMenu", Role::Menu, false, false),
    row("AXMenuBarItem", Role::MenuItem, false, false),
    row("AXMenuButton", Role::PopUpButton, false, false),
    row("AXScrollBar", Role::ScrollBar, false, false),
    row("AXSplitter", Role::SplitGroup, false, false),
    row("AXDialog", Role::Dialog, false, true),
    row("AXSystemDialog", Role::Dialog, false, true),
];

const fn row(ax: &'static str, role: Role, password: bool, modal: bool) -> RoleRow {
    RoleRow {
        ax,
        role,
        password,
        modal,
    }
}

/// Map an AX role and optional subrole.
///
/// Subroles that change the normalized role:
/// `AXSecureTextField` forces a password text field, `AXDialog` and
/// `AXSystemDialog` force a modal dialog when the role is a window or group,
/// `AXSheet` stays a sheet and is modal, `AXSwitch` / `AXToggle` become
/// switches, `AXSearchField` becomes a search field, and `AXOutlineRow`
/// becomes a tree item.
pub fn map_ax_role(role: &str, subrole: Option<&str>) -> MappedAxRole {
    let role_name = if role.is_empty() { "AXUnknown" } else { role };
    let found = ROLE_TABLE.iter().find(|row| row.ax == role_name);
    let mut mapped = MappedAxRole {
        role: found.map(|row| row.role).unwrap_or(Role::Unknown),
        native_role: SmolStr::new(role_name),
        subrole: subrole.filter(|value| !value.is_empty()).map(SmolStr::new),
        password: found.is_some_and(|row| row.password),
        modal: found.is_some_and(|row| row.modal),
    };
    apply_subrole(&mut mapped, subrole.unwrap_or(""));
    mapped
}

fn apply_subrole(mapped: &mut MappedAxRole, subrole: &str) {
    if mapped.native_role == "AXSecureTextField" || subrole == "AXSecureTextField" {
        mapped.role = Role::TextField;
        mapped.password = true;
    }
    if subrole == "AXSearchField" && mapped.role == Role::TextField {
        mapped.role = Role::SearchField;
    }
    if subrole == "AXOutlineRow" && mapped.role == Role::Row {
        mapped.role = Role::TreeItem;
    }
    if (subrole == "AXSwitch" || subrole == "AXToggle")
        && matches!(mapped.role, Role::CheckBox | Role::Button | Role::Unknown)
    {
        mapped.role = Role::Switch;
    }
    if subrole == "AXDialog" || subrole == "AXSystemDialog" {
        mapped.modal = true;
        if matches!(mapped.role, Role::Window | Role::Group | Role::Unknown) {
            mapped.role = Role::Dialog;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_table_has_at_least_50_mappings() {
        assert!(ROLE_TABLE.len() >= 50);
        for row in ROLE_TABLE {
            let mapped = map_ax_role(row.ax, None);
            assert_eq!(mapped.role, row.role, "{}", row.ax);
            assert_eq!(mapped.password, row.password, "{}", row.ax);
            assert_eq!(mapped.modal, row.modal, "{}", row.ax);
            assert_eq!(mapped.native_role.as_str(), row.ax);
        }
    }
}
