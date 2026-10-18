//! Engine-local Windows activity adapter; never sends Host activity RPCs.
use gcu_mcp::host::PhysicalActivity;
use std::sync::{Arc, Weak};

/// Forwards the Windows guard clock. The fake entry does not install it.
#[cfg_attr(not(all(windows, feature = "native-windows")), allow(dead_code))]
pub(crate) struct WindowsActivity(Weak<gcu_driver_windows::WindowsDriver>);
impl WindowsActivity {
    /// Share the driver the product process still holds.
    #[cfg_attr(not(all(windows, feature = "native-windows")), allow(dead_code))]
    pub(crate) fn new(driver: &Arc<gcu_driver_windows::WindowsDriver>) -> Self {
        Self(Arc::downgrade(driver))
    }
}
impl PhysicalActivity for WindowsActivity {
    fn ms_since_physical_input(&self) -> Option<u64> {
        self.0.upgrade()?.ms_since_physical_input()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_driver_windows::input::hotkey::{FakeSafetyApi, StopLatch};
    struct Latch;
    impl StopLatch for Latch {
        fn is_stopped(&self) -> bool {
            false
        }
        fn request_stop(&self) {}
    }
    #[tokio::test]
    async fn windows_activity_without_physical_observation_forwards_none() {
        let api = Arc::new(gcu_driver_windows::testing::FakeWinApi::default());
        let driver = gcu_driver_windows::WindowsDriver::with_api(api)
            .await
            .unwrap();
        let driver = Arc::new(driver);
        assert_eq!(
            WindowsActivity::new(&driver).ms_since_physical_input(),
            None
        );
    }
    #[tokio::test]
    async fn windows_activity_with_physical_observation_forwards_age_and_revocation() {
        use gcu_driver_windows::input::hooks::HookEvent;
        let api = Arc::new(gcu_driver_windows::testing::FakeWinApi::default());
        let guard = FakeSafetyApi::default().start(Default::default(), Arc::new(Latch));
        let driver = gcu_driver_windows::WindowsDriver::with_api(api)
            .await
            .unwrap()
            .with_safety(guard.clone());
        let driver = Arc::new(driver);
        let activity = WindowsActivity::new(&driver);
        assert_eq!(activity.ms_since_physical_input(), None);
        guard
            .activity_clock()
            .event(HookEvent::Mouse(0), std::time::Instant::now());
        assert_eq!(activity.ms_since_physical_input(), Some(0));
        guard.disable("test transport failure");
        assert_eq!(activity.ms_since_physical_input(), None);
    }
}
