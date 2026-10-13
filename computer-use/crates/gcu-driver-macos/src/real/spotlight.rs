//! Synchronous Spotlight query for application bundles used in the last 14 days.
//!
//! The discovery worker never calls this on the caller's first `list_apps`.
//! `kMDQuerySynchronous` (`1`) collects the current index and does not require
//! spinning the calling thread's run loop.

use crate::api::SpotlightHit;
use gcu_core::driver::{DriverError, Result};
use objc2_core_foundation::{CFArray, CFDate, CFRetained, CFString};
use objc2_core_services::{
    kMDItemCFBundleIdentifier, kMDItemDisplayName, kMDItemLastUsedDate, kMDItemPath,
    kMDQueryScopeComputer, MDItem, MDQuery,
};
use std::time::SystemTime;

pub(super) fn spotlight_recent() -> Result<Vec<SpotlightHit>> {
    let query_text = CFString::from_str(
        "kMDItemContentType == 'com.apple.application-bundle' && kMDItemLastUsedDate >= $time.now(-1209600)",
    );
    // SAFETY: the query string is a live CFString. Null attribute lists mean
    // "all attributes" and "no sort", which MDQuery accepts.
    let query = unsafe { MDQuery::new(None, Some(&query_text), None, None) }.ok_or(
        DriverError::Native {
            message: "spotlight query could not be created".into(),
            maybe_executed: false,
        },
    )?;
    let scope = scope_array()?;
    // SAFETY: `scope` is a one-element array of the process-lifetime computer
    // scope constant. Search scope must be set before execute. Option 0 adds
    // no extra scope flags. The query copies the array before this returns.
    unsafe { query.set_search_scope(Some(scope.as_opaque()), 0) };
    // SAFETY: flag 1 is kMDQuerySynchronous. The call blocks until the current
    // index is collected and does not register a live update.
    let started = unsafe { query.execute(1) };
    if !started {
        return Err(DriverError::Native {
            message: "spotlight query failed".into(),
            maybe_executed: false,
        });
    }
    let mut hits = Vec::new();
    let count = unsafe { query.result_count() };
    for index in 0..count {
        // SAFETY: `index` is in range. The query owns the MDItem for as long as
        // we do not call disable/enable around this borrow.
        let ptr = unsafe { query.result_at_index(index) };
        if ptr.is_null() {
            continue;
        }
        let item = unsafe { &*ptr.cast::<MDItem>() };
        if let Some(hit) = hit_from(item) {
            hits.push(hit);
        }
    }
    // SAFETY: the query is still alive. Disabling updates releases the result
    // callbacks we never installed; the copied strings are already owned.
    unsafe { query.disable_updates() };
    Ok(hits)
}

fn scope_array() -> Result<CFRetained<CFArray<CFString>>> {
    // SAFETY: kMDQueryScopeComputer is a process-lifetime CoreServices constant.
    let scope = unsafe { kMDQueryScopeComputer }.ok_or(DriverError::Native {
        message: "spotlight scope unavailable".into(),
        maybe_executed: false,
    })?;
    Ok(CFArray::from_objects(&[scope]))
}

fn hit_from(item: &MDItem) -> Option<SpotlightHit> {
    let path = item_string(item, unsafe { kMDItemPath })?;
    let name = item_string(item, unsafe { kMDItemDisplayName }).unwrap_or_else(|| file_name(&path));
    let bundle_id = item_string(item, unsafe { kMDItemCFBundleIdentifier });
    let last_used = item_date(item, unsafe { kMDItemLastUsedDate });
    Some(SpotlightHit {
        path,
        localized_name: name.clone(),
        name,
        bundle_id,
        last_used,
    })
}

fn item_string(item: &MDItem, key: Option<&CFString>) -> Option<String> {
    let key = key?;
    // SAFETY: `key` is a process-lifetime metadata constant. The returned type
    // is +1 and dropped with CFRetained.
    let value = unsafe { item.attribute(Some(key)) }?;
    let text = value.downcast::<CFString>().ok()?.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn item_date(item: &MDItem, key: Option<&CFString>) -> Option<SystemTime> {
    let key = key?;
    // SAFETY: same contract as [`item_string`].
    let value = unsafe { item.attribute(Some(key)) }?;
    value.downcast::<CFDate>().ok()?.to_system_time()
}

fn file_name(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".app")
        .to_owned()
}
