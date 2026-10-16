//! The monitored driver returns worker identities, never worker-local UIA handles.
#![cfg_attr(not(windows), allow(dead_code))]
use gcu_core::{driver::WindowRef, model::NativeHandle};
use gcu_driver_windows::worker::{Element, Window};

pub fn identity(handle: &NativeHandle, win: &WindowRef) -> Result<Element, String> {
    let element = handle
        .downcast_ref::<Element>()
        .ok_or("Invoke control expected a worker::Element from the monitored driver")?;
    let root = u64::from_str_radix(win.id.split(':').next().unwrap_or(""), 16)
        .map_err(|e| format!("Invoke control window HWND: {e}"))?;
    if element.window != Window::from(win) || element.hwnd != root || element.runtime_id.is_empty()
    {
        return Err(format!(
            "Invoke control identity does not match fixture root: window={}; element={element:?}",
            win.id
        ));
    }
    Ok(element.clone())
}

// GetForegroundWindow can return zero during activation (observed in win16).
// A zero alone proves no owner. Require a real fixture-owned changed HWND too,
// and reject every other process; this never changes driver equality checks.
pub fn fixture_activation(pid: u32, changed: &[(u64, u32)]) -> bool {
    pid != 0
        && changed
            .last()
            .is_some_and(|(hwnd, owner)| *hwnd != 0 && *owner == pid)
        && changed
            .iter()
            .all(|(hwnd, owner)| (*hwnd == 0 && *owner == 0) || (*hwnd != 0 && *owner == pid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_activation_gap_requires_fixture_owner_and_rejects_unrelated_owner() {
        assert!(fixture_activation(7, &[(0, 0), (8, 7), (8, 7)]));
        assert!(fixture_activation(7, &[(8, 7)]));
        assert!(!fixture_activation(7, &[(0, 0)]));
        assert!(!fixture_activation(7, &[(8, 7), (0, 0)]));
        assert!(!fixture_activation(7, &[(8, 7), (9, 11)]));
        assert!(!fixture_activation(7, &[]));
    }

    #[tokio::test]
    async fn monitored_proxy_preserves_exact_invoke_identity_and_rejects_wrong_root() {
        let (_, _, _, win) = crate::common::fixture().await;
        let element = Element {
            window: Window::from(&win),
            hwnd: u64::from_str_radix(win.id.split(':').next().unwrap(), 16).unwrap(),
            runtime_id: vec![42, 8, 19],
            generation: 3,
            slot: 19,
            fingerprint: 17,
            epoch: 2,
        };
        // Exercise the serialized proxy used by worker Tree::decode, rather
        // than constructing an in-process UiaElement (the win16 failure).
        let wire = serde_json::to_vec(&element).unwrap();
        let proxy: Element = serde_json::from_slice(&wire).unwrap();
        let handle = NativeHandle::new(proxy);
        assert_eq!(identity(&handle, &win).unwrap(), element);
        assert!(handle
            .downcast_ref::<gcu_driver_windows::uia::UiaElement>()
            .is_none());
        let mut other = win.clone();
        other.id = "9:another-window".into();
        assert!(identity(&handle, &other).is_err());
        assert!(identity(&NativeHandle::new(()), &win).is_err());
    }
}
