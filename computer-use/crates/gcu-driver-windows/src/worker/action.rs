//! Resolved action transport. Route ceilings and exact handles survive IPC.
use super::wire::{Element, Window};
use gcu_core::{driver::*, model::*};
use serde::{Deserialize, Serialize};

/// Serializable resolved driver action. No policy decisions happen in this type.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Transport for DriverAction::Press.
    Press {
        /// Resolved el.
        el: Element,
    },
    /// Transport for DriverAction::SecondaryAction.
    SecondaryAction {
        /// Resolved el.
        el: Element,
        /// Resolved action.
        action: NodeAction,
    },
    /// Transport for DriverAction::SetValue.
    SetValue {
        /// Resolved el.
        el: Element,
        /// Resolved value.
        value: String,
    },
    /// Transport for DriverAction::SelectText.
    SelectText {
        /// Resolved el.
        el: Element,
        /// Resolved range.
        range: TextRange,
    },
    /// Transport for DriverAction::ScrollElement.
    ScrollElement {
        /// Resolved el.
        el: Element,
        /// Resolved dir.
        dir: Dir,
        /// Resolved pages.
        pages: f32,
    },
    /// Transport for DriverAction::PointerClick.
    PointerClick {
        /// Resolved win.
        win: Window,
        /// Resolved at.
        at: WindowPoint,
        /// Resolved button.
        button: Button,
        /// Resolved count.
        count: u8,
        /// Resolved route.
        #[serde(with = "PointerRouteDef")]
        route: PointerRoute,
    },
    /// Transport for DriverAction::PointerScroll.
    PointerScroll {
        /// Resolved win.
        win: Window,
        /// Resolved at.
        at: WindowPoint,
        /// Resolved dx.
        dx: i32,
        /// Resolved dy.
        dy: i32,
        /// Resolved route.
        #[serde(with = "PointerRouteDef")]
        route: PointerRoute,
    },
    /// Transport for DriverAction::PointerDrag.
    PointerDrag {
        /// Resolved win.
        win: Window,
        /// Resolved path.
        path: Vec<WindowPoint>,
        /// Resolved route.
        #[serde(with = "PointerRouteDef")]
        route: PointerRoute,
    },
    /// Transport for DriverAction::TypeText.
    TypeText {
        /// Resolved win.
        win: Window,
        /// Resolved focus.
        focus: Option<Box<Element>>,
        /// Resolved text.
        text: String,
        /// Resolved route.
        #[serde(with = "KeyRouteDef")]
        route: KeyRoute,
    },
    /// Transport for DriverAction::PressKeys.
    PressKeys {
        /// Resolved win.
        win: Window,
        /// Resolved chord.
        chord: KeyChord,
        /// Resolved route.
        #[serde(with = "KeyRouteDef")]
        route: KeyRoute,
    },
    /// Transport for DriverAction::Activate.
    Activate {
        /// Resolved win.
        win: Window,
    },
}
impl Action {
    pub(crate) fn encode(a: &DriverAction) -> Result<Self> {
        Ok(match a {
            DriverAction::Press { el } => Self::Press { el: element(el)? },
            DriverAction::SecondaryAction { el, action } => Self::SecondaryAction {
                el: element(el)?,
                action: action.clone(),
            },
            DriverAction::SetValue { el, value } => Self::SetValue {
                el: element(el)?,
                value: value.clone(),
            },
            DriverAction::SelectText { el, range } => Self::SelectText {
                el: element(el)?,
                range: *range,
            },
            DriverAction::ScrollElement { el, dir, pages } => Self::ScrollElement {
                el: element(el)?,
                dir: *dir,
                pages: *pages,
            },
            DriverAction::PointerClick {
                win,
                at,
                button,
                count,
                route,
            } => Self::PointerClick {
                win: Window::from(win),
                at: *at,
                button: *button,
                count: *count,
                route: *route,
            },
            DriverAction::PointerScroll {
                win,
                at,
                dx,
                dy,
                route,
            } => Self::PointerScroll {
                win: Window::from(win),
                at: *at,
                dx: *dx,
                dy: *dy,
                route: *route,
            },
            DriverAction::PointerDrag { win, path, route } => Self::PointerDrag {
                win: Window::from(win),
                path: path.clone(),
                route: *route,
            },
            DriverAction::TypeText {
                win,
                focus,
                text,
                route,
            } => Self::TypeText {
                win: Window::from(win),
                focus: focus
                    .as_ref()
                    .map(|h| element(h).map(Box::new))
                    .transpose()?,
                text: text.clone(),
                route: *route,
            },
            DriverAction::PressKeys { win, chord, route } => Self::PressKeys {
                win: Window::from(win),
                chord: chord.clone(),
                route: *route,
            },
            DriverAction::Activate { win } => Self::Activate {
                win: Window::from(win),
            },
        })
    }
    pub(crate) fn decode(
        self,
        mut resolve: impl FnMut(Element) -> Result<NativeHandle>,
    ) -> Result<DriverAction> {
        Ok(match self {
            Self::Press { el } => DriverAction::Press { el: resolve(el)? },
            Self::SecondaryAction { el, action } => DriverAction::SecondaryAction {
                el: resolve(el)?,
                action,
            },
            Self::SetValue { el, value } => DriverAction::SetValue {
                el: resolve(el)?,
                value,
            },
            Self::SelectText { el, range } => DriverAction::SelectText {
                el: resolve(el)?,
                range,
            },
            Self::ScrollElement { el, dir, pages } => DriverAction::ScrollElement {
                el: resolve(el)?,
                dir,
                pages,
            },
            Self::PointerClick {
                win,
                at,
                button,
                count,
                route,
            } => DriverAction::PointerClick {
                win: win.into(),
                at,
                button,
                count,
                route,
            },
            Self::PointerScroll {
                win,
                at,
                dx,
                dy,
                route,
            } => DriverAction::PointerScroll {
                win: win.into(),
                at,
                dx,
                dy,
                route,
            },
            Self::PointerDrag { win, path, route } => DriverAction::PointerDrag {
                win: win.into(),
                path,
                route,
            },
            Self::TypeText {
                win,
                focus,
                text,
                route,
            } => DriverAction::TypeText {
                win: win.into(),
                focus: focus.map(|el| resolve(*el)).transpose()?,
                text,
                route,
            },
            Self::PressKeys { win, chord, route } => DriverAction::PressKeys {
                win: win.into(),
                chord,
                route,
            },
            Self::Activate { win } => DriverAction::Activate { win: win.into() },
        })
    }
    pub(crate) fn window(&self) -> &Window {
        match self {
            Self::Press { el }
            | Self::SecondaryAction { el, .. }
            | Self::SetValue { el, .. }
            | Self::SelectText { el, .. }
            | Self::ScrollElement { el, .. } => &el.window,
            Self::PointerClick { win, .. }
            | Self::PointerScroll { win, .. }
            | Self::PointerDrag { win, .. }
            | Self::TypeText { win, .. }
            | Self::PressKeys { win, .. }
            | Self::Activate { win } => win,
        }
    }
    pub(crate) fn valid_epoch(&self, epoch: u64) -> bool {
        match self {
            Self::Press { el }
            | Self::SecondaryAction { el, .. }
            | Self::SetValue { el, .. }
            | Self::SelectText { el, .. }
            | Self::ScrollElement { el, .. } => el.epoch == epoch,
            Self::TypeText { focus, .. } => focus.as_ref().is_none_or(|el| el.epoch == epoch),
            _ => true,
        }
    }
}
fn element(h: &NativeHandle) -> Result<Element> {
    h.downcast_ref::<Element>()
        .cloned()
        .ok_or(DriverError::StaleElement)
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "PointerRoute", rename_all = "snake_case")]
enum PointerRouteDef {
    Auto,
    TargetedOnly,
    AllowForeground,
}
#[derive(Serialize, Deserialize)]
#[serde(remote = "KeyRoute", rename_all = "snake_case")]
enum KeyRouteDef {
    Auto,
    TargetedOnly,
    AllowForeground,
}
