//! AT-SPI owner check for a scoped read or handle delivery.
//!
//! Scoped AT-SPI trees and semantic actions are closed. An object path is not
//! a window incarnation, and this card has no production per-node owner
//! observation. [`admit_tree`] refuses without copying the registered
//! incarnation. Handle comparisons below are test-only.

#[cfg(test)]
use crate::identity::scope::{HandleKey, HandleView};
use crate::identity::scope::ScopeBinding;
use gcu_core::driver::DriverError;
#[cfg(test)]
use gcu_core::model::{NativeHandle, RawTree};

/// Scoped AT-SPI trees are closed.
///
/// The registered incarnation is not an observation, so this does not read it.
pub(crate) fn admit_tree(
    binding: &ScopeBinding,
    app_id: &str,
    window_id: &str,
) -> Result<(), DriverError> {
    let _ = (binding, app_id, window_id);
    Err(DriverError::ScopeDenied)
}

/// Seal `tree` to `window_id` after a proof that has just succeeded.
#[cfg(test)]
pub(crate) fn seal_tree(
    binding: &ScopeBinding,
    tree: &RawTree,
    window_id: &str,
) -> Result<(), DriverError> {
    binding.seal_tree(tree, window_id)
}

/// Refuse delivery when the handle's live window is not the sealed window.
#[cfg(test)]
pub(crate) fn admit_handle(
    binding: &ScopeBinding,
    handle: &NativeHandle,
) -> Result<(), DriverError> {
    binding.admit_handle(handle)
}

/// Test and driver helper: record a live handle window.
#[cfg(test)]
pub(crate) fn note_live_handle(binding: &ScopeBinding, key: HandleKey, view: HandleView) {
    binding.oracle().set_handle(key, view);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::scope::{
        HandleKey, HandleView, MapClock, ProcessView, ScopeBinding, ScopeGate, ScopeInstance,
        ScopeWindow,
    };
    use gcu_core::model::{Fingerprint, NativeHandle, RawNode, RawTree, Role, States};
    use smallvec::SmallVec;
    use smol_str::SmolStr;
    use std::sync::Arc;

    fn binding() -> ScopeBinding {
        let owner = ScopeInstance::try_new(42, 1000, "boot-a").unwrap();
        let window = ScopeWindow::try_new("7", 5, owner, "app.fixture").unwrap();
        let gate = ScopeGate::try_new(vec![window]).unwrap();
        let clock = Arc::new(MapClock::new("boot-a"));
        clock.set(42, 1000);
        ScopeBinding::new(gate, clock)
    }

    fn node(id: u64) -> RawNode {
        RawNode {
            role: Role::Button,
            native_role: SmolStr::new("push button"),
            subrole: None,
            name: Some("canary-button".into()),
            value: None,
            description: None,
            placeholder: None,
            help: None,
            identifier: None,
            states: States::empty(),
            actions: SmallVec::new(),
            frame: None,
            text_extras: None,
            url: None,
            children: Vec::new(),
            handle: NativeHandle::new(id),
            fingerprint: Fingerprint(0),
        }
    }

    #[test]
    fn tree_read_requires_the_admitted_owner() {
        let binding = binding();
        assert_eq!(
            admit_tree(&binding, "app.fixture", "7"),
            Err(DriverError::ScopeDenied)
        );
        binding.oracle().set_atspi_pid("app.fixture", Some(42));
        let manufactured = binding.oracle().atspi("app.fixture", "7", 5);
        assert_eq!(manufactured.incarnation, 5);
        assert_eq!(
            admit_tree(&binding, "app.fixture", "7"),
            Err(DriverError::ScopeDenied)
        );
        let clock = MapClock::new("boot-a");
        clock.set(42, 2000);
        let moved = ScopeBinding::new(binding.gate().clone(), Arc::new(clock));
        moved.oracle().set_atspi_pid("app.fixture", Some(42));
        assert_eq!(
            admit_tree(&moved, "app.fixture", "7"),
            Err(DriverError::ScopeDenied)
        );
    }

    #[test]
    fn handle_crossing_windows_is_refused_before_delivery() {
        let binding = binding();
        let tree = RawTree {
            root: Some(node(11)),
            incomplete: None,
        };
        seal_tree(&binding, &tree, "7").unwrap();
        let handle = NativeHandle::new(11u64);
        assert_eq!(
            admit_handle(&binding, &handle),
            Err(DriverError::ScopeDenied)
        );
        note_live_handle(
            &binding,
            HandleKey::Fake(11),
            HandleView {
                window_id: "8".into(),
                incarnation: 5,
                owner: ProcessView::new(42, 1000, "boot-a"),
            },
        );
        let err = admit_handle(&binding, &handle).unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(!err.to_string().contains("canary"));
        note_live_handle(
            &binding,
            HandleKey::Fake(11),
            HandleView {
                window_id: "7".into(),
                incarnation: 5,
                owner: ProcessView::new(42, 1000, "boot-a"),
            },
        );
        assert!(admit_handle(&binding, &handle).is_ok());
    }

    #[tokio::test]
    async fn scoped_tree_and_press_stay_inside_the_admitted_owner() {
        use crate::driver::LinuxDriver;
        use gcu_core::driver::{AppRef, Cancel, Driver, DriverAction, TreeOpts, WindowRef};
        use gcu_protocol::AppInfo;

        let clock = Arc::new(MapClock::new("boot-a"));
        clock.set(42, 1000);
        let owner = ScopeInstance::try_new(42, 1000, "boot-a").unwrap();
        let window = ScopeWindow::try_new("fixture-window", 5, owner, "app.fixture").unwrap();
        let gate = ScopeGate::try_new(vec![window]).unwrap();
        let mut driver = LinuxDriver::new_fake(
            crate::atspi::FakeSource::default(),
            crate::atspi::FakeNode {
                name: Some("canary-tree".into()),
                ..crate::atspi::FakeNode::default()
            },
            Vec::new(),
            Vec::new(),
        );
        driver.bind_scope_binding(Arc::new(ScopeBinding::new(gate, clock.clone())));
        driver
            .scope()
            .unwrap()
            .oracle()
            .set_atspi_pid("app.fixture", Some(42));
        let win = WindowRef {
            app: AppRef {
                info: AppInfo {
                    id: "app.fixture".into(),
                    name: "Fixture".into(),
                    pid: Some(1),
                    identity: None,
                },
            },
            id: "fixture-window".into(),
        };
        let err = driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(!err.to_string().contains("canary"));
        clock.set(42, 2000);
        let err = driver
            .snapshot_tree(&win, &TreeOpts::default(), &Cancel::default())
            .await
            .unwrap_err();
        assert_eq!(err, DriverError::ScopeDenied);
        assert!(!err.to_string().contains("canary"));

        clock.set(42, 1000);
        let pressed = driver
            .perform(
                &DriverAction::Press {
                    el: NativeHandle::new(11u64),
                },
                &Cancel::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(pressed, DriverError::ScopeDenied);
    }
}
