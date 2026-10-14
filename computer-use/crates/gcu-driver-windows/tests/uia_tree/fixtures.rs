//! Mixed Chromium UIA red-team fixture, including disguised execution descendants.
use super::*;
pub(super) fn mixed_web_fixture() -> FakeUia {
    let button = |name: &str, y: f64| FakeNode {
        properties: Properties {
            control_type: 50000,
            name: Some(name.into()),
            identifier: Some(name.to_owned()),
            frame: Some(Rect {
                x: 110.,
                y: 200. + y,
                w: 100.,
                h: 30.,
            }),
            patterns: Patterns {
                invoke: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            name: Some("Fixture".into()),
            frame: Some(Rect {
                x: 100.,
                y: 200.,
                w: 640.,
                h: 480.,
            }),
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: Properties {
                control_type: 50030,
                framework: Some("Chrome".into()),
                name: Some("Workspace".into()),
                ..Default::default()
            },
            children: vec![
                button("Save draft", 20.),
                FakeNode {
                    properties: Properties {
                        control_type: 50033,
                        name: Some("Debug Console REPL".into()),
                        frame: Some(Rect {
                            x: 100.,
                            y: 280.,
                            w: 640.,
                            h: 200.,
                        }),
                        ..Default::default()
                    },
                    children: vec![
                        // A disguised/unlabeled button inherits the execution marker.
                        button("Continue", 90.),
                        button("", 130.),
                    ],
                },
                button("Run Task...", 300.),
                FakeNode {
                    properties: Properties {
                        control_type: 50004,
                        name: Some("Password".into()),
                        password: true,
                        value: Some(NodeValue::Text("fixture-secret".into())),
                        frame: Some(Rect {
                            x: 110.,
                            y: 540.,
                            w: 120.,
                            h: 30.,
                        }),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
        }],
    })
}

pub(super) fn render_opts() -> RenderOpts {
    RenderOpts::new(
        gcu_protocol::AppInfo {
            id: "fixture.exe".into(),
            name: "Fixture".into(),
            pid: Some(7),
            identity: None,
        },
        gcu_protocol::WindowInfo {
            id: "fixture:8".into(),
            title: "Fixture".into(),
            frame: Rect {
                x: 100.,
                y: 200.,
                w: 640.,
                h: 480.,
            },
            scale: 1.5,
            on_current_desktop: Some(true),
        },
    )
}

pub(super) fn count(node: &gcu_core::model::RawNode) -> usize {
    1 + node.children.iter().map(count).sum::<usize>()
}
pub(super) fn timeout() -> DriverError {
    DriverError::Timeout {
        maybe_executed: false,
    }
}
pub(super) fn native() -> DriverError {
    DriverError::Native {
        message: "fake property failure".into(),
        maybe_executed: false,
    }
}
