//! Chromium and Electron accessibility switches.
//!
//! Those apps hide most of the tree until `AXEnhancedUserInterface` and
//! `AXManualAccessibility` are set. The original values are recorded and
//! written back from [`AccessibilityToggles::restore`], from Drop, and from
//! the driver when a turn or session ends. A panic while the toggles are
//! armed still runs Drop. A process kill cannot.

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use gcu_core::driver::DriverError;
use gcu_core::driver::Result;

/// Attribute this session may have overwritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ToggleName {
    /// `AXEnhancedUserInterface`.
    EnhancedUserInterface,
    /// `AXManualAccessibility`.
    ManualAccessibility,
}

impl ToggleName {
    /// AX attribute name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnhancedUserInterface => "AXEnhancedUserInterface",
            Self::ManualAccessibility => "AXManualAccessibility",
        }
    }
}

/// Synchronous store so Drop can restore without an async runtime.
///
/// `get` returns `None` when the attribute is missing. `set` returns `false`
/// when the attribute cannot be written. That is not a hard error and is not
/// restored. `PermissionMissing` still fails the caller.
pub trait ToggleStore: Send + Sync {
    /// Read the current boolean, if the attribute exists.
    fn get(&self, pid: u32, name: ToggleName) -> Result<Option<bool>>;
    /// Write a boolean. `false` means the app did not accept it.
    fn set(&self, pid: u32, name: ToggleName, value: bool) -> Result<bool>;
}

#[derive(Clone, Copy, Debug)]
struct Arm {
    prev_enhanced: Option<bool>,
    wrote_enhanced: bool,
    prev_manual: Option<bool>,
    wrote_manual: bool,
}

/// Recorded AX switches for one session.
pub struct AccessibilityToggles<S: ToggleStore> {
    store: S,
    arms: Mutex<BTreeMap<u32, Arm>>,
    restored: AtomicBool,
}

impl<S: ToggleStore> AccessibilityToggles<S> {
    /// Empty record. Nothing is written until [`Self::enable`].
    pub fn new(store: S) -> Self {
        Self {
            store,
            arms: Mutex::new(BTreeMap::new()),
            restored: AtomicBool::new(false),
        }
    }

    /// Set both attributes to true for `pid` and remember the previous values.
    ///
    /// A second call for the same pid does nothing. If the second write fails
    /// after the first succeeded, the first write is restored before the error
    /// is returned. Unsupported attributes are skipped.
    pub fn enable(&self, pid: u32) -> Result<()> {
        let arms = self.arms.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        if arms.contains_key(&pid) {
            return Ok(());
        }
        drop(arms);
        let prev_enhanced = self.store.get(pid, ToggleName::EnhancedUserInterface)?;
        let prev_manual = self.store.get(pid, ToggleName::ManualAccessibility)?;
        let wrote_enhanced = self
            .store
            .set(pid, ToggleName::EnhancedUserInterface, true)?;
        let wrote_manual = match self.store.set(pid, ToggleName::ManualAccessibility, true) {
            Ok(wrote) => wrote,
            Err(error) => {
                if wrote_enhanced {
                    let _ = self.store.set(
                        pid,
                        ToggleName::EnhancedUserInterface,
                        prev_enhanced.unwrap_or(false),
                    );
                }
                return Err(error);
            }
        };
        if !wrote_enhanced && !wrote_manual {
            return Ok(());
        }
        let mut arms = self.arms.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        arms.insert(
            pid,
            Arm {
                prev_enhanced,
                wrote_enhanced,
                prev_manual,
                wrote_manual,
            },
        );
        self.restored.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// Write the recorded values back. A second call does not write again.
    ///
    /// Missing originals are restored as `false` only for attributes this
    /// session successfully set.
    pub fn restore(&self) {
        if self.restored.swap(true, Ordering::SeqCst) {
            return;
        }
        let Ok(mut arms) = self.arms.lock() else {
            return;
        };
        let taken = std::mem::take(&mut *arms);
        drop(arms);
        for (pid, arm) in taken {
            if arm.wrote_enhanced {
                let _ = self.store.set(
                    pid,
                    ToggleName::EnhancedUserInterface,
                    arm.prev_enhanced.unwrap_or(false),
                );
            }
            if arm.wrote_manual {
                let _ = self.store.set(
                    pid,
                    ToggleName::ManualAccessibility,
                    arm.prev_manual.unwrap_or(false),
                );
            }
        }
    }
}

impl<S: ToggleStore> AccessibilityToggles<S> {
    /// Borrow the store. Tests read it after restore.
    pub fn store(&self) -> &S {
        &self.store
    }
}

/// Arm process-wide switches for one observation, or do nothing in a scope.
///
/// Scoped mode does not write `AXEnhancedUserInterface` or
/// `AXManualAccessibility`, and it does not record values to restore.
/// A tree that stays incomplete uses the walker's existing incomplete fact.
/// Memory only when `session_scoped` is true. Otherwise this is [`AccessibilityToggles::enable`].
///
/// The macOS snapshot and tests are the callers. A host build that is not
/// macOS and not a test has no observation path.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
pub(crate) fn arm_for_observation<S: ToggleStore>(
    toggles: &AccessibilityToggles<S>,
    pid: u32,
    session_scoped: bool,
) -> Result<()> {
    if session_scoped {
        return Ok(());
    }
    toggles.enable(pid)
}

impl<S: ToggleStore> Drop for AccessibilityToggles<S> {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Driver-held toggle that can be restored without naming the store type.
pub trait ErasedToggle: Send {
    /// Restore recorded values. Safe to call more than once.
    fn restore(&self);
}

impl<S: ToggleStore> ErasedToggle for AccessibilityToggles<S> {
    fn restore(&self) {
        AccessibilityToggles::restore(self);
    }
}

/// In-memory attributes for the restore tests.
#[derive(Clone, Debug, Default)]
pub struct FakeToggleStore {
    values: Arc<Mutex<BTreeMap<(u32, ToggleName), Slot>>>,
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    value: Option<bool>,
    settable: bool,
}

impl FakeToggleStore {
    /// All attributes missing and settable.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shared store. `enable` on any clone writes the same map.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Current value, if a successful set stored one.
    pub fn value(&self, pid: u32, name: ToggleName) -> Option<bool> {
        self.values
            .lock()
            .ok()
            .and_then(|guard| guard.get(&(pid, name)).and_then(|slot| slot.value))
    }

    /// When false, [`ToggleStore::set`] returns `Ok(false)`.
    pub fn set_settable(&self, pid: u32, name: ToggleName, settable: bool) {
        if let Ok(mut guard) = self.values.lock() {
            guard
                .entry((pid, name))
                .or_insert(Slot {
                    value: None,
                    settable: true,
                })
                .settable = settable;
        }
    }
}

impl ToggleStore for FakeToggleStore {
    fn get(&self, pid: u32, name: ToggleName) -> Result<Option<bool>> {
        let guard = self.values.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        Ok(guard.get(&(pid, name)).and_then(|slot| slot.value))
    }

    fn set(&self, pid: u32, name: ToggleName, value: bool) -> Result<bool> {
        let mut guard = self.values.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        let slot = guard.entry((pid, name)).or_insert(Slot {
            value: None,
            settable: true,
        });
        if !slot.settable {
            return Ok(false);
        }
        slot.value = Some(value);
        Ok(true)
    }
}

impl ToggleStore for Arc<FakeToggleStore> {
    fn get(&self, pid: u32, name: ToggleName) -> Result<Option<bool>> {
        (**self).get(pid, name)
    }

    fn set(&self, pid: u32, name: ToggleName, value: bool) -> Result<bool> {
        (**self).set(pid, name, value)
    }
}

/// Apps that need the enhanced accessibility attributes turned on.
///
/// Matching is the bundle id (prefix or an `electron` substring) or a name
/// token `electron` / `chromium`. The bare word `code` is not enough.
pub fn needs_enhanced_accessibility(bundle_id: &str, name: &str) -> bool {
    let id = bundle_id.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "com.google.chrome",
        "org.chromium.chromium",
        "com.microsoft.edgemac",
        "com.brave.browser",
        "com.vivaldi.vivaldi",
        "com.operasoftware.opera",
        "company.thebrowser.browser",
        "com.microsoft.vscode",
        "com.visualstudio.code.oss",
        "com.slack.slack",
        "com.hnc.discord",
        "com.figma.desktop",
        "notion.id",
        "md.obsidian",
        "com.todesktop.",
        "com.github.electron",
        "org.whispersystems.signal-desktop",
    ];
    if id.contains("electron") || MARKERS.iter().any(|marker| id.contains(marker)) {
        return true;
    }
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word == "electron" || word == "chromium")
}
