//! Patch only the CU preference, retaining unrelated and unknown settings.
//! The owner marker coordinates rollback; it never grants execution authority.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(crate) const OWNER_KEY: &str = "_computerUsePreferenceOwner";
const ENABLED_KEY: &str = "computerUseEnabled";

#[derive(Clone)]
pub(super) struct PreferenceReceipt {
    owner: String,
}

#[derive(Clone)]
pub(crate) struct FeaturePreferences {
    path: PathBuf,
}

impl FeaturePreferences {
    #[cfg(test)]
    pub(super) fn at(path: PathBuf) -> Self {
        Self { path }
    }

    #[cfg(test)]
    pub(super) fn snapshot(&self) -> Value {
        self.load().unwrap()
    }

    pub(crate) fn product() -> Self {
        Self {
            path: crate::paths::settings_file(),
        }
    }

    fn load(&self) -> Result<Value, String> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return serde_json::to_value(crate::store::AppSettings::default())
                    .map_err(|_| "Computer Use preference serialization failed".into());
            }
            Err(_) => return Err("Computer Use preference unavailable".into()),
        };
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| "Computer Use preference invalid")?;
        if !value.is_object() || value.get(ENABLED_KEY).is_some_and(|v| !v.is_boolean()) {
            return Err("Computer Use preference invalid".into());
        }
        Ok(value)
    }

    fn write(&self, value: &Value) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|_| "Computer Use preference serialization failed")?;
        crate::store_lock::write_bytes_replace(&self.path, &bytes)
            .map_err(|_| "Computer Use preference write failed".into())
    }

    pub(super) fn enable(&self, expected: bool) -> Result<Option<PreferenceReceipt>, String> {
        let _lock = crate::store_lock::lock_exclusive(&self.path)?;
        let mut value = self.load()?;
        if value
            .get(ENABLED_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
            != expected
        {
            return Err("Computer Use preference changed during approval".into());
        }
        if expected {
            return Ok(None);
        }
        let owner = uuid::Uuid::new_v4().to_string();
        value[ENABLED_KEY] = json!(true);
        value[OWNER_KEY] = json!(owner);
        self.write(&value)?;
        Ok(Some(PreferenceReceipt { owner }))
    }

    fn owns(value: &Value, receipt: Option<&PreferenceReceipt>) -> bool {
        value.get(ENABLED_KEY).and_then(Value::as_bool) == Some(true)
            && receipt.is_none_or(|receipt| {
                value.get(OWNER_KEY).and_then(Value::as_str) == Some(receipt.owner.as_str())
            })
    }

    /// The callback is the final, synchronous Broker commit. Never perform
    /// native preparation or async work while holding this file lock.
    pub(super) fn with_owned<T>(
        &self,
        receipt: Option<&PreferenceReceipt>,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = crate::store_lock::lock_exclusive(&self.path)?;
        if !Self::owns(&self.load()?, receipt) {
            return Err("Computer Use preference changed during approval".into());
        }
        commit()
    }

    pub(super) fn rollback(&self, receipt: &PreferenceReceipt) -> Result<bool, String> {
        let _lock = crate::store_lock::lock_exclusive(&self.path)?;
        let mut value = self.load()?;
        if !Self::owns(&value, Some(receipt)) {
            return Ok(false);
        }
        value[ENABLED_KEY] = json!(false);
        value
            .as_object_mut()
            .expect("validated settings object")
            .remove(OWNER_KEY);
        self.write(&value)?;
        Ok(true)
    }

    pub(crate) fn set_explicit(&self, enabled: bool) -> Result<(), String> {
        let _lock = crate::store_lock::lock_exclusive(&self.path)?;
        let mut value = self.load()?;
        value[ENABLED_KEY] = json!(enabled);
        value
            .as_object_mut()
            .expect("validated settings object")
            .remove(OWNER_KEY);
        self.write(&value)
    }
}

/// Called by the normal whole-settings writer while holding the same lock.
/// An unrelated preference edit must retain an in-progress CU rollback owner.
pub(crate) fn preserve_owner(path: &Path, value: &mut Value) {
    let previous = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    let Some(previous) = previous else { return };
    if previous.get(ENABLED_KEY) == value.get(ENABLED_KEY) {
        if let Some(owner) = previous
            .get(OWNER_KEY)
            .and_then(Value::as_str)
            .filter(|owner| owner.len() <= 128)
        {
            value[OWNER_KEY] = json!(owner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (FeaturePreferences, PathBuf) {
        let root = std::env::temp_dir().join(format!("cu-preference-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = FeaturePreferences {
            path: root.join("settings.json"),
        };
        store.set_explicit(false).unwrap();
        (store, root)
    }

    fn cleanup(store: &FeaturePreferences, root: &Path) {
        std::fs::remove_file(&store.path).unwrap();
        std::fs::remove_file(crate::store_lock::lock_path_for(&store.path)).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn rollback_preserves_a_concurrent_unrelated_setting_and_unknown_fields() {
        let (store, root) = fixture();
        let receipt = store.enable(false).unwrap().unwrap();
        let mut settings: crate::store::AppSettings =
            serde_json::from_value(store.load().unwrap()).unwrap();
        settings.locale = "ja".into();
        crate::store::save_settings_at(&store.path, &settings).unwrap();
        {
            let _lock = crate::store_lock::lock_exclusive(&store.path).unwrap();
            let mut current = store.load().unwrap();
            current["futurePreference"] = json!({"unchanged": 42});
            store.write(&current).unwrap();
        }
        assert!(store.rollback(&receipt).unwrap());
        let result = store.load().unwrap();
        assert_eq!(result[ENABLED_KEY], false);
        assert_eq!(result["locale"], "ja");
        assert_eq!(result["futurePreference"]["unchanged"], 42);
        assert!(result.get(OWNER_KEY).is_none());
        cleanup(&store, &root);
    }

    #[test]
    fn explicit_off_then_on_wins_and_old_receipt_cannot_commit_or_rollback() {
        let (store, root) = fixture();
        let receipt = store.enable(false).unwrap().unwrap();
        store.set_explicit(false).unwrap();
        store.set_explicit(true).unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(store
            .with_owned(Some(&receipt), || {
                calls.set(1);
                Ok(())
            })
            .is_err());
        assert_eq!(calls.get(), 0);
        assert!(!store.rollback(&receipt).unwrap());
        assert_eq!(store.load().unwrap()[ENABLED_KEY], true);
        cleanup(&store, &root);
    }
}
