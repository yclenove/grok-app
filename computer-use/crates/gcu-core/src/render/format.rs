use crate::model::*;

pub(crate) fn role(role: Role) -> &'static str {
    match role {
        Role::Window => "standard window",
        Role::Dialog => "dialog",
        Role::Sheet => "sheet",
        Role::Popover => "popover",
        Role::Menu => "menu",
        Role::MenuItem => "menu item",
        Role::MenuBar => "menu bar",
        Role::Toolbar => "toolbar",
        Role::Button => "button",
        Role::Link => "link",
        Role::CheckBox => "check box",
        Role::Radio => "radio",
        Role::Switch => "switch",
        Role::TextField => "text field",
        Role::SearchField => "search field",
        Role::TextArea => "text area",
        Role::ComboBox => "combo box",
        Role::PopUpButton => "pop up button",
        Role::Slider => "slider",
        Role::Stepper => "stepper",
        Role::ProgressBar => "progress bar",
        Role::List => "list",
        Role::ListItem => "list item",
        Role::Table => "table",
        Role::Row => "row",
        Role::Cell => "cell",
        Role::Column => "column",
        Role::Outline => "outline",
        Role::Tree => "tree",
        Role::TreeItem => "tree item",
        Role::Tab => "tab",
        Role::TabList => "tab list",
        Role::ScrollArea => "scroll area",
        Role::ScrollBar => "scroll bar",
        Role::Group => "group",
        Role::SplitGroup => "split group",
        Role::Image => "image",
        Role::StaticText => "static text",
        Role::Heading => "heading",
        Role::WebArea => "web area",
        Role::Document => "document",
        Role::Canvas => "canvas",
        Role::Unknown => "unknown",
    }
}
pub(crate) fn action(action: &NodeAction) -> String {
    match action {
        NodeAction::Native(name) => name.to_string(),
        NodeAction::Press => "Press".into(),
        NodeAction::ShowMenu => "Show Menu".into(),
        NodeAction::Increment => "Increment".into(),
        NodeAction::Decrement => "Decrement".into(),
        NodeAction::Confirm => "Confirm".into(),
        NodeAction::Cancel => "Cancel".into(),
        NodeAction::Raise => "Raise".into(),
        NodeAction::Expand => "Expand".into(),
        NodeAction::Collapse => "Collapse".into(),
        NodeAction::ScrollIntoView => "Scroll Into View".into(),
        NodeAction::ScrollUp => "Scroll Up".into(),
        NodeAction::ScrollDown => "Scroll Down".into(),
        NodeAction::ScrollLeft => "Scroll Left".into(),
        NodeAction::ScrollRight => "Scroll Right".into(),
        NodeAction::Select => "Select".into(),
        NodeAction::Toggle => "Toggle".into(),
    }
}
pub(crate) fn value(value: &NodeValue) -> String {
    match value {
        NodeValue::Text(text) => text.clone(),
        NodeValue::Number(number) => number.to_string(),
        NodeValue::Bool(boolean) => boolean.to_string(),
        NodeValue::Range { value, min, max } => format!("{value} ({min}–{max})"),
    }
}
pub(crate) fn states(node: &RawNode) -> Vec<&'static str> {
    let mut states = Vec::new();
    for (flag, name) in [
        (States::SELECTED, "selected"),
        (States::SELECTABLE, "selectable"),
        (States::EXPANDED, "expanded"),
        (States::SETTABLE, "settable"),
        (States::FOCUSED, "focused"),
        (States::CHECKED, "checked"),
        (States::MIXED, "mixed"),
        (States::EDITABLE, "editable"),
        (States::FOCUSABLE, "focusable"),
        (States::EXPANDABLE, "expandable"),
        (States::PASSWORD, "password"),
        (States::OFFSCREEN, "offscreen"),
        (States::MODAL, "modal"),
        (States::BUSY, "busy"),
        (States::REQUIRED, "required"),
    ] {
        if node.states.contains(flag) {
            states.push(name);
        }
    }
    if !node.states.contains(States::ENABLED) {
        states.push("disabled");
    }
    states
}
