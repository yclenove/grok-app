//! Browser profile deletion and parameter checks. No content WebView is created here.

use std::path::Path;

use gcu_protocol::builtin_browser::{
    is_exact_profile, profile_dir, window_valid, BrowserNavigate, BrowserSync, PROFILE_DIR_NAME,
};
use serde_json::Value;

use crate::Error;

/// `engine.browser.sync` parameters. A window that fails field checks is invalid.
pub(crate) fn sync_params(params: &Value) -> Result<(), Error> {
    let sync: BrowserSync = serde_json::from_value(params.clone()).map_err(|_| Error::Invalid)?;
    if sync
        .browser
        .as_ref()
        .is_some_and(|window| !window_valid(window))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

/// `host.browser.navigate` parameters, before any availability decision.
pub(crate) fn navigate_params(params: &Value) -> Result<(), Error> {
    serde_json::from_value::<BrowserNavigate>(params.clone())
        .map(|_| ())
        .map_err(|_| Error::Invalid)
}

/// Delete only `app_data/cu-browser-profile`.
///
/// A missing directory is success. A symlink, a junction, or a path that
/// escapes `app_data` is an error and is not deleted. Failure is not success.
/// Blocking filesystem IO; call off the UI thread. No network.
pub fn clear_profile(app_data: &Path) -> Result<(), Error> {
    if !app_data.is_absolute() {
        return Err(Error::Invalid);
    }
    let dir = profile_dir(app_data);
    let meta = match std::fs::symlink_metadata(&dir) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::Io(error)),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::Invalid);
    }
    let app_canon = std::fs::canonicalize(app_data)?;
    let dir_canon = std::fs::canonicalize(&dir)?;
    if dir_canon.file_name().and_then(|name| name.to_str()) != Some(PROFILE_DIR_NAME)
        || !is_exact_profile(&app_canon, &dir_canon)
    {
        return Err(Error::Invalid);
    }
    std::fs::remove_dir_all(&dir_canon)?;
    Ok(())
}
