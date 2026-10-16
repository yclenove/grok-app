//! Pure app catalog and exact resolution.
//!
//! Queries match the whole bundle id, localized name, bundle name, bundle path,
//! executable path, or the final path component. Matching is case-insensitive
//! and does not fuzzy-match. Two distinct bundles are ambiguous; the driver
//! reports [`gcu_core::driver::DriverError::AmbiguousApp`], which has no
//! candidate list. Candidates stay on [`ResolveOutcome`].

use crate::{
    api::{BundleFacts, RunningApp, SpotlightHit},
    identity::{identity_from_facts, MacIdentity},
    spotlight::within_recent,
};
use gcu_core::driver::Result;
use gcu_protocol::AppInfo;
use std::time::SystemTime;

/// One resolved bundle, with the grant attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedApp {
    /// Portable record. Spotlight-only rows are not represented here.
    pub info: AppInfo,
    /// Authorization identity.
    pub identity: MacIdentity,
    /// Running pid, when this row came from `NSWorkspace`.
    pub pid: Option<u32>,
    /// Hidden flag from the running application. False for Spotlight rows.
    pub hidden: bool,
}

/// Exact resolution. Ambiguous results keep every candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// One bundle matched.
    One(Box<ResolvedApp>),
    /// More than one bundle matched, sorted by bundle id then executable path.
    Ambiguous(Vec<ResolvedApp>),
    /// Nothing matched.
    None,
}

/// Merge running apps with Spotlight hits from the last 14 days.
///
/// A running process wins over a Spotlight row with the same bundle id or path.
/// Spotlight rows carry no identity until resolution loads facts. Hits with no
/// last-used date are dropped. A future timestamp is kept.
pub fn list_catalog(
    running: &[ResolvedApp],
    recent: &[SpotlightHit],
    now: SystemTime,
) -> Vec<AppInfo> {
    let mut apps: Vec<AppInfo> = running.iter().map(|app| app.info.clone()).collect();
    for hit in recent {
        if !within_recent(hit.last_used, now) || running.iter().any(|app| same_target(app, hit)) {
            continue;
        }
        let id = hit
            .bundle_id
            .clone()
            .filter(|value| !value.is_empty())
            .or_else(|| crate::identity::normalize_path(&hit.path).ok())
            .unwrap_or_else(|| hit.path.clone());
        if apps.iter().any(|app| app.id.eq_ignore_ascii_case(&id)) {
            continue;
        }
        let name = display_name(&hit.localized_name, &hit.name, &hit.path);
        apps.push(AppInfo {
            id,
            name,
            pid: None,
            identity: None,
        });
    }
    apps.sort_by(|left, right| left.id.cmp(&right.id));
    apps
}

/// Resolve `query` against facts the caller already loaded.
///
/// `recent` should contain only hits the caller decided could match, each with
/// facts. Running rows that duplicate a recent bundle keep the pid. `direct`
/// is an absolute path the caller opened itself; it is not required to be in
/// Spotlight. An exact path still loses to a second exact match.
pub fn resolve_loaded(
    running: &[(RunningApp, BundleFacts)],
    recent: &[(SpotlightHit, BundleFacts)],
    direct: Option<&BundleFacts>,
    query: &str,
    now: SystemTime,
) -> Result<ResolveOutcome> {
    let mut found = Vec::new();
    for (app, facts) in running {
        if !matches_facts(facts, query) && !matches_running(app, query) {
            continue;
        }
        found.push(resolved_from(facts.clone(), Some(app.pid), app.hidden)?);
    }
    for (hit, facts) in recent {
        if !within_recent(hit.last_used, now) {
            continue;
        }
        if found.iter().any(|app| same_facts(app, facts)) {
            continue;
        }
        if matches_facts(facts, query) || hit_could_match(hit, query) {
            found.push(resolved_from(facts.clone(), None, false)?);
        }
    }
    if let Some(facts) = direct {
        if !found.iter().any(|app| same_facts(app, facts)) && matches_facts(facts, query) {
            found.push(resolved_from(facts.clone(), None, false)?);
        }
    }
    dedup_in_place(&mut found);
    found.sort_by(|left, right| {
        let left_id = left.identity.bundle_id.as_deref().unwrap_or("");
        let right_id = right.identity.bundle_id.as_deref().unwrap_or("");
        left_id.cmp(right_id).then(
            left.identity
                .executable_path
                .cmp(&right.identity.executable_path),
        )
    });
    Ok(match found.len() {
        0 => ResolveOutcome::None,
        1 => ResolveOutcome::One(Box::new(found.remove(0))),
        _ => ResolveOutcome::Ambiguous(found),
    })
}

/// True when a Spotlight hit can match without opening the bundle.
/// Absolute paths under the bundle also match, so an executable path does not
/// force a stat of every recent app.
pub fn hit_could_match(hit: &SpotlightHit, query: &str) -> bool {
    let query = query_key(query);
    if query.is_empty() {
        return false;
    }
    eq(&hit.name, &query)
        || eq(&hit.localized_name, &query)
        || hit.bundle_id.as_ref().is_some_and(|id| eq(id, &query))
        || eq(&hit.path, &query)
        || file_name(&hit.path).is_some_and(|name| eq(name, &query))
        || path_under(&hit.path, &query)
}

/// Exact match against bundle facts, including localized name and file names.
pub fn matches_facts(facts: &BundleFacts, query: &str) -> bool {
    let query = query_key(query);
    if query.is_empty() {
        return false;
    }
    facts.bundle_id.as_ref().is_some_and(|id| eq(id, &query))
        || eq(&facts.localized_name, &query)
        || eq(&facts.bundle_name, &query)
        || eq(&facts.bundle_path, &query)
        || eq(&facts.executable_path, &query)
        || file_name(&facts.executable_path).is_some_and(|name| eq(name, &query))
        || file_name(&facts.bundle_path).is_some_and(|name| eq(name, &query))
}

/// Match the running-app fields before facts are required.
pub fn matches_running(app: &RunningApp, query: &str) -> bool {
    let query = query_key(query);
    if query.is_empty() {
        return false;
    }
    app.bundle_id.as_ref().is_some_and(|id| eq(id, &query))
        || eq(&app.localized_name, &query)
        || eq(&app.bundle_path, &query)
        || eq(&app.executable_path, &query)
        || file_name(&app.executable_path).is_some_and(|name| eq(name, &query))
        || file_name(&app.bundle_path).is_some_and(|name| eq(name, &query))
        || file_name(&app.bundle_path).is_some_and(|name| eq(name.trim_end_matches(".app"), &query))
}

/// Build a resolved row. `pid` is set for a running process and absent for Spotlight.
pub fn resolved_from(facts: BundleFacts, pid: Option<u32>, hidden: bool) -> Result<ResolvedApp> {
    let name = display_name(
        &facts.localized_name,
        &facts.bundle_name,
        &facts.bundle_path,
    );
    let identity = identity_from_facts(facts)?;
    let grant = identity.app_identity();
    Ok(ResolvedApp {
        info: AppInfo {
            id: grant.id.clone(),
            name,
            pid,
            identity: Some(grant),
        },
        identity,
        pid,
        hidden,
    })
}

fn same_target(app: &ResolvedApp, hit: &SpotlightHit) -> bool {
    if let (Some(left), Some(right)) = (&app.identity.bundle_id, &hit.bundle_id) {
        if left.eq_ignore_ascii_case(right) {
            return true;
        }
    }
    crate::identity::normalize_path(&hit.path)
        .ok()
        .is_some_and(|path| path.eq_ignore_ascii_case(&app.identity.bundle_path))
}

fn same_facts(app: &ResolvedApp, facts: &BundleFacts) -> bool {
    if let (Some(left), Some(right)) = (&app.identity.bundle_id, &facts.bundle_id) {
        if left.eq_ignore_ascii_case(right) {
            return true;
        }
    }
    facts
        .executable_path
        .eq_ignore_ascii_case(&app.identity.executable_path)
        || facts
            .bundle_path
            .eq_ignore_ascii_case(&app.identity.bundle_path)
}

fn dedup_in_place(apps: &mut Vec<ResolvedApp>) {
    let mut kept: Vec<ResolvedApp> = Vec::new();
    for app in apps.drain(..) {
        if let Some(existing) = kept.iter_mut().find(|item| same_identity(item, &app)) {
            if existing.pid.is_none() {
                existing.pid = app.pid;
                existing.info.pid = app.info.pid;
                existing.hidden = app.hidden;
            }
        } else {
            kept.push(app);
        }
    }
    *apps = kept;
}

fn same_identity(left: &ResolvedApp, right: &ResolvedApp) -> bool {
    match (&left.identity.bundle_id, &right.identity.bundle_id) {
        (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => true,
        _ => left
            .identity
            .executable_path
            .eq_ignore_ascii_case(&right.identity.executable_path),
    }
}

fn display_name(localized: &str, fallback: &str, path: &str) -> String {
    if !localized.trim().is_empty() {
        localized.to_owned()
    } else if !fallback.trim().is_empty() {
        fallback.to_owned()
    } else {
        file_name(path)
            .unwrap_or(path)
            .trim_end_matches(".app")
            .to_owned()
    }
}

fn path_under(bundle: &str, query: &str) -> bool {
    let bundle = bundle.trim().trim_end_matches('/').to_lowercase();
    !bundle.is_empty() && (query == bundle || query.starts_with(&(bundle + "/")))
}

fn query_key(query: &str) -> String {
    let trimmed = query.trim();
    if let Ok(path) = crate::identity::normalize_path(trimmed) {
        path.to_lowercase()
    } else {
        trimmed.to_lowercase()
    }
}

fn eq(value: &str, query: &str) -> bool {
    if query.starts_with('/') {
        return crate::identity::normalize_path(value.trim())
            .ok()
            .is_some_and(|path| path.to_lowercase() == query);
    }
    value.trim().to_lowercase() == query
}

fn file_name(path: &str) -> Option<&str> {
    path.rsplit('/').next().filter(|name| !name.is_empty())
}
