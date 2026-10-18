//! Debug of captures and trees must not print pixels or text.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use gcu_core::driver::Capture;
use gcu_core::model::{Geometry, NodeFlags, NodeValue, Rect, Role, TextExtras};
use gcu_core::tree::{ProcessedNode, ProcessedTree};
use support::node;

#[test]
fn capture_image_and_tree_debug_omit_raw_bytes_and_text() {
    let secret = b"SUPER-SECRET-PIXELS-99";
    let capture = Capture {
        bytes: secret.to_vec(),
        width: 2,
        height: 2,
        mime: "image/png".into(),
        geometry: Geometry {
            frame: Rect {
                x: 0.0,
                y: 0.0,
                w: 2.0,
                h: 2.0,
            },
            scale: 1.0,
        },
    };
    let rendered = format!("{capture:?}");
    assert!(!rendered.contains("SUPER-SECRET-PIXELS-99"));
    assert!(rendered.contains("byte_count"));
    assert!(!rendered.contains("bytes:"));

    let mut raw = node(Role::TextField, "SECRET-NAME-xyz", vec![]);
    raw.value = Some(NodeValue::Text("SECRET-VALUE-xyz".into()));
    raw.text_extras = Some(TextExtras {
        text: Some("SECRET-EXTRA-xyz".into()),
        ..TextExtras::default()
    });
    raw.url = Some("https://secret.example/SECRET-URL-xyz".into());
    let raw_debug = format!("{raw:?}");
    for planted in [
        "SECRET-NAME-xyz",
        "SECRET-VALUE-xyz",
        "SECRET-EXTRA-xyz",
        "SECRET-URL-xyz",
    ] {
        assert!(!raw_debug.contains(planted), "{raw_debug}");
    }
    let value = raw.value.clone().unwrap();
    assert!(!format!("{value:?}").contains("SECRET-VALUE-xyz"));
    let extras = raw.text_extras.clone().unwrap();
    assert!(!format!("{extras:?}").contains("SECRET-EXTRA-xyz"));

    let mut tree = ProcessedTree::default();
    tree.root = Some(ProcessedNode {
        element_index: 0,
        node: raw,
        children: vec![],
        flags: NodeFlags::empty(),
        omitted_nodes: 0,
    });
    let tree_debug = format!("{tree:?}");
    assert!(!tree_debug.contains("SECRET-VALUE-xyz"), "{tree_debug}");
    assert!(!tree_debug.contains("SECRET-NAME-xyz"), "{tree_debug}");
    assert!(!tree_debug.contains("SECRET-EXTRA-xyz"), "{tree_debug}");
}
