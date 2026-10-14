//! Physical user activity guard monitoring physical input events via XInput2.
//!
//! Excludes synthetic injection devices (such as XTEST pointer/keyboard) so that
//! engine-delivered events never refresh the activity clock, while real physical
//! inputs (keyboard, mouse, trackpad) update the clock.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// Source of timing information for physical user activity.
pub trait ActivityClock: Send + Sync {
    /// Milliseconds since last physical user input, or `None` if unknown/unavailable.
    ///
    /// If the monitoring thread crashes, the X11 connection drops, or XI2 is unavailable,
    /// this MUST return `None` rather than reporting an arbitrarily stale timestamp.
    fn ms_since_physical_input(&self) -> Option<u64>;

    /// Whether the activity guard is currently active and healthy.
    fn is_available(&self) -> bool;

    /// Update the availability status of the clock.
    fn set_available(&self, available: bool);

    /// Record a physical input event at the current time.
    fn record_physical_input(&self);
}

/// Thread-safe in-memory clock updated by input event monitors.
#[derive(Debug, Default)]
pub struct SharedActivityClock {
    last_physical_input: Mutex<Option<Instant>>,
    available: AtomicBool,
    test_now: Mutex<Option<Instant>>,
}

impl SharedActivityClock {
    /// Create a new activity clock with the given availability state.
    pub fn new(available: bool) -> Self {
        Self {
            last_physical_input: Mutex::new(None),
            available: AtomicBool::new(available),
            test_now: Mutex::new(None),
        }
    }

    /// Record that physical user input occurred at the current instant.
    pub fn record_physical_input(&self) {
        let now = self.now();
        let mut guard = match self.last_physical_input.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = Some(now);
    }

    /// Explicitly set the timestamp of the last physical input.
    pub fn set_last_input(&self, at: Instant) {
        let mut guard = match self.last_physical_input.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = Some(at);
    }

    /// Clear physical input history. Subsequent reads return `None`.
    pub fn clear_physical_input(&self) {
        let mut guard = match self.last_physical_input.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = None;
    }

    /// Set an explicit `now` instant for deterministic testing.
    pub fn set_test_now(&self, now: Option<Instant>) {
        let mut guard = match self.test_now.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *guard = now;
    }

    /// Update the health/availability of the activity guard.
    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Release);
    }

    fn now(&self) -> Instant {
        let guard = match self.test_now.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.unwrap_or_else(Instant::now)
    }
}

impl ActivityClock for SharedActivityClock {
    fn ms_since_physical_input(&self) -> Option<u64> {
        if !self.available.load(Ordering::Acquire) {
            return None;
        }
        let guard = match self.last_physical_input.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let last = (*guard)?;
        let now = self.now();
        let millis = now.saturating_duration_since(last).as_millis();
        Some(u64::try_from(millis).unwrap_or(u64::MAX))
    }

    fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Release);
    }

    fn record_physical_input(&self) {
        SharedActivityClock::record_physical_input(self);
    }
}

/// Device information used for distinguishing physical from synthetic input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Device ID assigned by the X server.
    pub id: u16,
    /// Human-readable device name reported by X11.
    pub name: String,
    /// Whether this device is an XTEST / synthetic injection device.
    pub is_xtest: bool,
}

impl DeviceInfo {
    /// Create a new [`DeviceInfo`], automatically inferring `is_xtest` from the name.
    pub fn new(id: u16, name: impl Into<String>) -> Self {
        let name = name.into();
        let is_xtest = is_xtest_device_name(&name);
        Self { id, name, is_xtest }
    }
}

/// Determine whether a device name indicates an XTEST synthetic device.
pub fn is_xtest_device_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains("xtest")
}

/// Filter that distinguishes physical user devices from synthetic XTEST devices.
#[derive(Debug, Clone, Default)]
pub struct DeviceFilter {
    xtest_ids: HashSet<u16>,
}

impl DeviceFilter {
    /// Construct a device filter from a known list of devices.
    pub fn from_devices(devices: &[DeviceInfo]) -> Self {
        let xtest_ids = devices
            .iter()
            .filter(|d| d.is_xtest || is_xtest_device_name(&d.name))
            .map(|d| d.id)
            .collect();
        Self { xtest_ids }
    }

    /// Check whether a device ID belongs to an XTEST device.
    pub fn is_xtest(&self, device_id: u16) -> bool {
        self.xtest_ids.contains(&device_id)
    }

    /// Process an input event from `device_id`.
    ///
    /// If the event originated from an XTEST device, it is ignored and returns `false`.
    /// If the event originated from a physical device, `clock.record_physical_input()`
    /// is called and returns `true`.
    pub fn process_event(&self, device_id: u16, clock: &dyn ActivityClock) -> bool {
        if self.is_xtest(device_id) {
            false
        } else {
            clock.record_physical_input();
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_filter_and_clock() {
        let filter = DeviceFilter::from_devices(&[
            DeviceInfo::new(1, "Virtual core pointer"),
            DeviceInfo::new(2, "Virtual core XTEST pointer"),
            DeviceInfo::new(3, "Virtual core XTEST keyboard"),
            DeviceInfo::new(4, "AT Translated Set 2 keyboard"),
        ]);

        assert!(filter.is_xtest(2));
        assert!(filter.is_xtest(3));
        assert!(!filter.is_xtest(1));
        assert!(!filter.is_xtest(4));

        let clock = SharedActivityClock::new(true);
        assert!(clock.is_available());
        assert_eq!(clock.ms_since_physical_input(), None);

        // Event from XTEST device does not update clock
        assert!(!filter.process_event(2, &clock));
        assert_eq!(clock.ms_since_physical_input(), None);

        // Event from physical device does update clock
        assert!(filter.process_event(4, &clock));
        assert!(clock.ms_since_physical_input().is_some());

        // When listener is marked unavailable, clock becomes None
        clock.set_available(false);
        assert!(!clock.is_available());
        assert_eq!(clock.ms_since_physical_input(), None);
    }
}
