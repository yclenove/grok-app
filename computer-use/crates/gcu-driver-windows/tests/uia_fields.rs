//! Exercise the production property decoder before binding it to FakeUia.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use gcu_core::{
    driver::{Cancel, TreeOpts},
    model::{IncompleteReason, States},
};
use gcu_driver_windows::uia::{
    cache::{
        read::{decode, Fields},
        Properties,
    },
    source::{FakeNode, FakeUia},
    tree::build_raw_tree,
};

struct FieldFailure(i32);
impl Fields for FieldFailure {
    fn boolean(&mut self, id: i32) -> Option<bool> {
        (id != self.0).then_some(id == 30010)
    }
    fn integer(&mut self, id: i32) -> Option<i32> {
        (id != self.0).then_some(50000)
    }
    fn string(&mut self, id: i32) -> Option<String> {
        (id != self.0).then(|| "counter_button".into())
    }
    fn number(&mut self, _: i32) -> Option<f64> {
        None
    }
    fn frame(&mut self) -> Option<gcu_core::model::Rect> {
        None
    }
    fn runtime_id(&mut self) -> Option<Vec<i32>> {
        Some(vec![42, 2])
    }
    fn failure(&self) -> Option<String> {
        Some(format!(
            "GetCachedPropertyValueEx({}): HRESULT 80004005; elapsed_ms=7",
            self.0
        ))
    }
}
fn fixture(p: Properties) -> FakeUia {
    FakeUia::new(FakeNode {
        properties: Properties {
            control_type: 50032,
            ..Default::default()
        },
        children: vec![FakeNode {
            properties: p,
            ..Default::default()
        }],
    })
}

#[tokio::test]
async fn single_optional_field_failure_keeps_tree_complete_and_other_fields() {
    for id in [
        30005, 30011, 30003, 30008, 30009, 30010, 30022, 30031, 30043, 30090,
    ] {
        let p = decode(&mut FieldFailure(id));
        assert!(!p.partial, "field {id}");
        if id == 30005 {
            assert!(p.name.is_none());
        }
        if id == 30011 {
            assert!(p.identifier.is_none());
        }
        let source = fixture(p);
        let raw = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap();
        assert!(raw.incomplete.is_none(), "field {id}: {raw:?}");
        assert_eq!(raw.root.unwrap().children.len(), 1);
    }
}

#[tokio::test]
async fn password_field_failure_preserves_privacy_and_marks_tree_incomplete() {
    let p = decode(&mut FieldFailure(30019));
    assert!(p.partial && p.password && p.value.is_none());
    let raw = build_raw_tree(&fixture(p), 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        raw.incomplete.unwrap().reason,
        IncompleteReason::PartialError
    );
    assert!(raw.root.unwrap().children[0]
        .states
        .contains(States::PASSWORD));
}

#[tokio::test]
async fn child_enumeration_failure_is_incomplete_and_recovery_reread_is_complete() {
    let source = fixture(decode(&mut FieldFailure(30005)));
    source.fail_children(
        1,
        gcu_core::driver::DriverError::Native {
            message: "GetFirstChildElementBuildCache: HRESULT 80004005; elapsed_ms=9".into(),
            maybe_executed: false,
        },
    );
    let raw = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert_eq!(
        raw.incomplete.unwrap().reason,
        IncompleteReason::PartialError
    );
    assert!(raw.root.unwrap().children.is_empty());
    source.clear_read_errors();
    let raw = build_raw_tree(&source, 8, &TreeOpts::default(), &Cancel::default())
        .await
        .unwrap();
    assert!(raw.incomplete.is_none());
    assert_eq!(raw.root.unwrap().children.len(), 1);
}
