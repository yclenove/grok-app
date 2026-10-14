//! Synthetic UIA action fixtures.
use super::*;
use std::sync::Arc;
pub(super) async fn fixture(
    properties: Properties,
) -> (
    Arc<FakeUia>,
    gcu_driver_windows::WindowsDriver,
    WindowRef,
    NativeHandle,
) {
    let (api, _, _, _) = common::fixture().await;
    let source = Arc::new(FakeUia::new(FakeNode {
        properties,
        ..Default::default()
    }));
    let driver = gcu_driver_windows::WindowsDriver::with_uia(
        api,
        source.clone(),
        Arc::new(FakeUiaEvents::default()),
    )
    .await
    .unwrap();
    let app = driver.resolve_app(&AppQuery::from("Editor")).await.unwrap();
    let info = driver.key_window(&app).await.unwrap();
    let win = WindowRef { app, id: info.id };
    let handle = driver
        .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap()
        .root
        .unwrap()
        .handle;
    (source, driver, win, handle)
}

pub(super) fn all_patterns() -> Properties {
    Properties {
        read_only: Some(false),
        patterns: Patterns {
            invoke: true,
            toggle: true,
            selection_item: true,
            expand_collapse: true,
            value: true,
            range_value: true,
            scroll: true,
            scroll_item: true,
            text: true,
            legacy: true,
        },
        ..Default::default()
    }
}

pub(super) async fn portable(props: Properties) -> (FakeUia, NativeHandle) {
    use gcu_driver_windows::uia::{source::UiaSource, UiaElement};
    let source = FakeUia::new(FakeNode {
        properties: props,
        ..Default::default()
    });
    let node = source.root(8).await.unwrap().unwrap();
    source.properties(node).await.unwrap();
    let window = WindowRef {
        app: AppRef {
            info: AppInfo {
                id: "fixture".into(),
                name: "Fixture".into(),
                pid: Some(7),
                identity: None,
            },
        },
        id: "8".into(),
    };
    (source, NativeHandle::new(UiaElement { window, node }))
}
pub(super) fn request(pattern: Pattern, el: NativeHandle) -> (DriverAction, PatternAction) {
    match pattern {
        Pattern::Invoke => (DriverAction::Press { el }, PatternAction::Invoke),
        Pattern::Toggle => (
            DriverAction::SecondaryAction {
                el,
                action: NodeAction::Toggle,
            },
            PatternAction::Toggle,
        ),
        Pattern::SelectionItem => (
            DriverAction::SecondaryAction {
                el,
                action: NodeAction::Select,
            },
            PatternAction::Select,
        ),
        Pattern::ExpandCollapse => (
            DriverAction::SecondaryAction {
                el,
                action: NodeAction::Expand,
            },
            PatternAction::Expand,
        ),
        Pattern::Value => (
            DriverAction::SetValue {
                el,
                value: "12".into(),
            },
            PatternAction::SetValue("12".into()),
        ),
        Pattern::RangeValue => (
            DriverAction::SetValue {
                el,
                value: "12".into(),
            },
            PatternAction::SetRangeValue(12.),
        ),
        Pattern::Scroll => (
            DriverAction::ScrollElement {
                el,
                dir: Dir::Down,
                pages: 1.,
            },
            PatternAction::Scroll {
                dir: Dir::Down,
                pages: 1.,
            },
        ),
        Pattern::ScrollItem => (
            DriverAction::SecondaryAction {
                el,
                action: NodeAction::ScrollIntoView,
            },
            PatternAction::ScrollIntoView,
        ),
        Pattern::Text => (
            DriverAction::SelectText {
                el,
                range: TextRange {
                    start: 1,
                    length: 2,
                },
            },
            PatternAction::SelectText(TextRange {
                start: 1,
                length: 2,
            }),
        ),
        Pattern::Legacy => (DriverAction::Press { el }, PatternAction::LegacyDefault),
    }
}
pub(super) const PATTERNS: &[Pattern] = &[
    Pattern::Invoke,
    Pattern::Toggle,
    Pattern::SelectionItem,
    Pattern::ExpandCollapse,
    Pattern::Value,
    Pattern::RangeValue,
    Pattern::Scroll,
    Pattern::ScrollItem,
    Pattern::Text,
    Pattern::Legacy,
];
pub(super) fn only(pattern: Pattern) -> Properties {
    let mut props = Properties {
        read_only: Some(false),
        ..Default::default()
    };
    let p = &mut props.patterns;
    match pattern {
        Pattern::Invoke => p.invoke = true,
        Pattern::Toggle => p.toggle = true,
        Pattern::SelectionItem => p.selection_item = true,
        Pattern::ExpandCollapse => p.expand_collapse = true,
        Pattern::Value => p.value = true,
        Pattern::RangeValue => p.range_value = true,
        Pattern::Scroll => p.scroll = true,
        Pattern::ScrollItem => p.scroll_item = true,
        Pattern::Text => p.text = true,
        Pattern::Legacy => p.legacy = true,
    }
    props
}
