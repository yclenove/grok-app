//! Pure profile-path check. Deletion IO lives in the host protocol, not here.

use std::path::{Path, PathBuf};

use super::PROFILE_DIR_NAME;

/// `app_data/cu-browser-profile`. The name is not taken from the caller.
#[must_use]
pub fn profile_dir(app_data: &Path) -> PathBuf {
    app_data.join(PROFILE_DIR_NAME)
}

/// True when `candidate` is exactly the fixed profile directory inside `app_data`.
///
/// Both paths must already be absolute and canonical. A symlink, a junction,
/// or a path that escapes `app_data` is false and must not be deleted.
#[must_use]
pub fn is_exact_profile(app_data: &Path, candidate: &Path) -> bool {
    if !app_data.is_absolute() || !candidate.is_absolute() {
        return false;
    }
    if candidate.parent() != Some(app_data) {
        return false;
    }
    candidate.file_name().and_then(|name| name.to_str()) == Some(PROFILE_DIR_NAME)
}
