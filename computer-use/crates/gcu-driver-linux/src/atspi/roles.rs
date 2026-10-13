//! AT-SPI role to `gcu_core::model::Role` mapping table.

use gcu_core::model::Role as CoreRole;
use smol_str::SmolStr;

/// Convert an AT-SPI role into its normalized core role and a native role identifier.
///
/// Native role string retains the AT-SPI role name (e.g. `"ATSPI_ROLE_PUSH_BUTTON"`).
/// Unknown or unmapped roles map to `CoreRole::Unknown`.
pub fn map_atspi_role(role: atspi::Role) -> (CoreRole, SmolStr) {
    let native_name = role.name();
    let native_identifier = format!(
        "ATSPI_ROLE_{}",
        native_name.to_ascii_uppercase().replace([' ', '-'], "_")
    );
    let native_role = SmolStr::new(native_identifier);

    let core_role = match role {
        atspi::Role::Button | atspi::Role::PushButtonMenu => CoreRole::Button,
        atspi::Role::CheckBox | atspi::Role::CheckMenuItem => CoreRole::CheckBox,
        atspi::Role::RadioButton | atspi::Role::RadioMenuItem => CoreRole::Radio,
        atspi::Role::ToggleButton => CoreRole::Switch,
        atspi::Role::Entry => CoreRole::TextField,
        atspi::Role::PasswordText => CoreRole::TextField,
        atspi::Role::Text => CoreRole::TextArea,
        atspi::Role::ComboBox => CoreRole::ComboBox,
        atspi::Role::Slider => CoreRole::Slider,
        atspi::Role::SpinButton => CoreRole::Stepper,
        atspi::Role::ProgressBar | atspi::Role::LevelBar => CoreRole::ProgressBar,
        atspi::Role::Menu | atspi::Role::PopupMenu => CoreRole::Menu,
        atspi::Role::MenuBar => CoreRole::MenuBar,
        atspi::Role::MenuItem | atspi::Role::TearoffMenuItem => CoreRole::MenuItem,
        atspi::Role::ToolBar => CoreRole::Toolbar,
        atspi::Role::List | atspi::Role::ListBox => CoreRole::List,
        atspi::Role::ListItem => CoreRole::ListItem,
        atspi::Role::Table => CoreRole::Table,
        atspi::Role::TableCell => CoreRole::Cell,
        atspi::Role::TableRow => CoreRole::Row,
        atspi::Role::TableColumnHeader | atspi::Role::ColumnHeader => CoreRole::Column,
        atspi::Role::RowHeader => CoreRole::Row,
        atspi::Role::Tree | atspi::Role::TreeTable => CoreRole::Tree,
        atspi::Role::TreeItem => CoreRole::TreeItem,
        atspi::Role::PageTab => CoreRole::Tab,
        atspi::Role::PageTabList => CoreRole::TabList,
        atspi::Role::ScrollBar => CoreRole::ScrollBar,
        atspi::Role::ScrollPane => CoreRole::ScrollArea,
        atspi::Role::Panel
        | atspi::Role::Grouping
        | atspi::Role::RootPane
        | atspi::Role::LayeredPane => CoreRole::Group,
        atspi::Role::SplitPane => CoreRole::SplitGroup,
        atspi::Role::Dialog | atspi::Role::Alert => CoreRole::Dialog,
        atspi::Role::Frame
        | atspi::Role::Window
        | atspi::Role::InternalFrame
        | atspi::Role::DesktopFrame => CoreRole::Window,
        atspi::Role::Link => CoreRole::Link,
        atspi::Role::Image | atspi::Role::Icon => CoreRole::Image,
        atspi::Role::Label | atspi::Role::Static => CoreRole::StaticText,
        atspi::Role::Heading => CoreRole::Heading,
        atspi::Role::DocumentWeb => CoreRole::WebArea,
        atspi::Role::DocumentText
        | atspi::Role::DocumentFrame
        | atspi::Role::DocumentSpreadsheet
        | atspi::Role::DocumentPresentation => CoreRole::Document,
        atspi::Role::Canvas | atspi::Role::DrawingArea => CoreRole::Canvas,
        atspi::Role::Notification | atspi::Role::ToolTip => CoreRole::Popover,
        // Terminal has no native Role variant in core; mapped to Unknown and flagged
        // as terminal_like via heuristics.
        atspi::Role::Terminal => CoreRole::Unknown,
        _ => CoreRole::Unknown,
    };

    (core_role, native_role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_driven_role_mapping_covers_over_40_roles() {
        let cases: &[(atspi::Role, CoreRole, &str)] = &[
            (atspi::Role::Button, CoreRole::Button, "ATSPI_ROLE_BUTTON"),
            (
                atspi::Role::PushButtonMenu,
                CoreRole::Button,
                "ATSPI_ROLE_PUSH_BUTTON_MENU",
            ),
            (
                atspi::Role::CheckBox,
                CoreRole::CheckBox,
                "ATSPI_ROLE_CHECK_BOX",
            ),
            (
                atspi::Role::CheckMenuItem,
                CoreRole::CheckBox,
                "ATSPI_ROLE_CHECK_MENU_ITEM",
            ),
            (
                atspi::Role::RadioButton,
                CoreRole::Radio,
                "ATSPI_ROLE_RADIO_BUTTON",
            ),
            (
                atspi::Role::RadioMenuItem,
                CoreRole::Radio,
                "ATSPI_ROLE_RADIO_MENU_ITEM",
            ),
            (
                atspi::Role::ToggleButton,
                CoreRole::Switch,
                "ATSPI_ROLE_TOGGLE_BUTTON",
            ),
            (atspi::Role::Entry, CoreRole::TextField, "ATSPI_ROLE_ENTRY"),
            (
                atspi::Role::PasswordText,
                CoreRole::TextField,
                "ATSPI_ROLE_PASSWORD_TEXT",
            ),
            (atspi::Role::Text, CoreRole::TextArea, "ATSPI_ROLE_TEXT"),
            (
                atspi::Role::ComboBox,
                CoreRole::ComboBox,
                "ATSPI_ROLE_COMBO_BOX",
            ),
            (atspi::Role::Slider, CoreRole::Slider, "ATSPI_ROLE_SLIDER"),
            (
                atspi::Role::SpinButton,
                CoreRole::Stepper,
                "ATSPI_ROLE_SPIN_BUTTON",
            ),
            (
                atspi::Role::ProgressBar,
                CoreRole::ProgressBar,
                "ATSPI_ROLE_PROGRESS_BAR",
            ),
            (
                atspi::Role::LevelBar,
                CoreRole::ProgressBar,
                "ATSPI_ROLE_LEVEL_BAR",
            ),
            (atspi::Role::Menu, CoreRole::Menu, "ATSPI_ROLE_MENU"),
            (
                atspi::Role::PopupMenu,
                CoreRole::Menu,
                "ATSPI_ROLE_POPUP_MENU",
            ),
            (
                atspi::Role::MenuBar,
                CoreRole::MenuBar,
                "ATSPI_ROLE_MENU_BAR",
            ),
            (
                atspi::Role::MenuItem,
                CoreRole::MenuItem,
                "ATSPI_ROLE_MENU_ITEM",
            ),
            (
                atspi::Role::TearoffMenuItem,
                CoreRole::MenuItem,
                "ATSPI_ROLE_TEAROFF_MENU_ITEM",
            ),
            (
                atspi::Role::ToolBar,
                CoreRole::Toolbar,
                "ATSPI_ROLE_TOOL_BAR",
            ),
            (atspi::Role::List, CoreRole::List, "ATSPI_ROLE_LIST"),
            (atspi::Role::ListBox, CoreRole::List, "ATSPI_ROLE_LIST_BOX"),
            (
                atspi::Role::ListItem,
                CoreRole::ListItem,
                "ATSPI_ROLE_LIST_ITEM",
            ),
            (atspi::Role::Table, CoreRole::Table, "ATSPI_ROLE_TABLE"),
            (
                atspi::Role::TableCell,
                CoreRole::Cell,
                "ATSPI_ROLE_TABLE_CELL",
            ),
            (atspi::Role::TableRow, CoreRole::Row, "ATSPI_ROLE_TABLE_ROW"),
            (
                atspi::Role::TableColumnHeader,
                CoreRole::Column,
                "ATSPI_ROLE_TABLE_COLUMN_HEADER",
            ),
            (
                atspi::Role::ColumnHeader,
                CoreRole::Column,
                "ATSPI_ROLE_COLUMN_HEADER",
            ),
            (
                atspi::Role::RowHeader,
                CoreRole::Row,
                "ATSPI_ROLE_ROW_HEADER",
            ),
            (atspi::Role::Tree, CoreRole::Tree, "ATSPI_ROLE_TREE"),
            (
                atspi::Role::TreeTable,
                CoreRole::Tree,
                "ATSPI_ROLE_TREE_TABLE",
            ),
            (
                atspi::Role::TreeItem,
                CoreRole::TreeItem,
                "ATSPI_ROLE_TREE_ITEM",
            ),
            (atspi::Role::PageTab, CoreRole::Tab, "ATSPI_ROLE_PAGE_TAB"),
            (
                atspi::Role::PageTabList,
                CoreRole::TabList,
                "ATSPI_ROLE_PAGE_TAB_LIST",
            ),
            (
                atspi::Role::ScrollBar,
                CoreRole::ScrollBar,
                "ATSPI_ROLE_SCROLL_BAR",
            ),
            (
                atspi::Role::ScrollPane,
                CoreRole::ScrollArea,
                "ATSPI_ROLE_SCROLL_PANE",
            ),
            (atspi::Role::Panel, CoreRole::Group, "ATSPI_ROLE_PANEL"),
            (
                atspi::Role::Grouping,
                CoreRole::Group,
                "ATSPI_ROLE_GROUPING",
            ),
            (
                atspi::Role::SplitPane,
                CoreRole::SplitGroup,
                "ATSPI_ROLE_SPLIT_PANE",
            ),
            (atspi::Role::Dialog, CoreRole::Dialog, "ATSPI_ROLE_DIALOG"),
            (atspi::Role::Alert, CoreRole::Dialog, "ATSPI_ROLE_ALERT"),
            (atspi::Role::Frame, CoreRole::Window, "ATSPI_ROLE_FRAME"),
            (atspi::Role::Window, CoreRole::Window, "ATSPI_ROLE_WINDOW"),
            (atspi::Role::Link, CoreRole::Link, "ATSPI_ROLE_LINK"),
            (atspi::Role::Image, CoreRole::Image, "ATSPI_ROLE_IMAGE"),
            (atspi::Role::Label, CoreRole::StaticText, "ATSPI_ROLE_LABEL"),
            (
                atspi::Role::Heading,
                CoreRole::Heading,
                "ATSPI_ROLE_HEADING",
            ),
            (
                atspi::Role::DocumentWeb,
                CoreRole::WebArea,
                "ATSPI_ROLE_DOCUMENT_WEB",
            ),
            (
                atspi::Role::DocumentText,
                CoreRole::Document,
                "ATSPI_ROLE_DOCUMENT_TEXT",
            ),
            (atspi::Role::Canvas, CoreRole::Canvas, "ATSPI_ROLE_CANVAS"),
            (
                atspi::Role::Terminal,
                CoreRole::Unknown,
                "ATSPI_ROLE_TERMINAL",
            ),
            (
                atspi::Role::Unknown,
                CoreRole::Unknown,
                "ATSPI_ROLE_UNKNOWN",
            ),
        ];

        assert!(
            cases.len() >= 40,
            "must cover at least 40 common roles, got {}",
            cases.len()
        );

        for (atspi_role, expected_core, expected_native) in cases {
            let (core, native) = map_atspi_role(*atspi_role);
            assert_eq!(
                core, *expected_core,
                "mismatch for role {atspi_role:?}: expected {expected_core:?}, got {core:?}"
            );
            assert_eq!(
                native.as_str(),
                *expected_native,
                "native mismatch for role {atspi_role:?}"
            );
        }
    }
}
