#![allow(dead_code)]
use gcu_core::model::*;

pub fn node(role: Role, name: &str, children: Vec<RawNode>) -> RawNode {
    RawNode {
        role,
        native_role: format!("{role:?}").into(),
        subrole: None,
        name: (!name.is_empty()).then(|| name.into()),
        value: None,
        description: None,
        placeholder: None,
        help: None,
        identifier: None,
        states: States::ENABLED,
        actions: Default::default(),
        frame: None,
        text_extras: None,
        url: None,
        children,
        handle: NativeHandle::new(()),
        fingerprint: Fingerprint::default(),
    }
}
pub fn raw(children: Vec<RawNode>) -> RawTree {
    let mut root = node(Role::Window, "Fixture", children);
    gcu_core::fingerprint::refresh_tree(&mut root, Fingerprint::default(), 0);
    RawTree {
        root: Some(root),
        incomplete: None,
    }
}
pub fn opts() -> gcu_core::render::RenderOpts {
    use gcu_protocol::{AppInfo, WindowInfo};
    gcu_core::render::RenderOpts::new(
        AppInfo {
            id: "fake.fixture".into(),
            name: "Fixture".into(),
            pid: Some(42),
            identity: None,
        },
        WindowInfo {
            id: "window".into(),
            title: "Fixture".into(),
            frame: Rect {
                x: 0.,
                y: 0.,
                w: 800.,
                h: 480.,
            },
            scale: 1.,
            on_current_desktop: None,
        },
    )
}
