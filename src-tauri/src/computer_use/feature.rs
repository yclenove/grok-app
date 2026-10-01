//! Feature flag default off. Per-session enablement is independent of official-aux.

use grok_computer_use_core::surface::{surface_allows_local, ComputerUseSurface};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub const FEATURE_DEFAULT: bool = false;

static FEATURE: Mutex<bool> = Mutex::new(FEATURE_DEFAULT);
static SESSIONS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static SURFACES: Mutex<Option<HashMap<String, ComputerUseSurface>>> = Mutex::new(None);

fn sessions() -> std::sync::MutexGuard<'static, Option<HashSet<String>>> {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner())
}

fn feature() -> std::sync::MutexGuard<'static, bool> {
    FEATURE.lock().unwrap_or_else(|e| e.into_inner())
}

fn surfaces() -> std::sync::MutexGuard<'static, Option<HashMap<String, ComputerUseSurface>>> {
    SURFACES.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_feature_enabled(on: bool) {
    *feature() = on;
    if !on {
        *sessions() = Some(HashSet::new());
        *surfaces() = Some(HashMap::new());
    }
}

pub fn feature_enabled() -> bool {
    *feature()
}

pub fn set_session_enabled(app_session_id: &str, on: bool) {
    let mut g = sessions();
    let set = g.get_or_insert_with(HashSet::new);
    let id = app_session_id.trim();
    if id.is_empty() {
        return;
    }
    if on {
        set.insert(id.to_string());
    } else {
        set.remove(id);
        if let Some(map) = surfaces().as_mut() {
            map.remove(id);
        }
    }
}

pub fn set_session_surface(app_session_id: &str, surface: ComputerUseSurface) {
    let id = app_session_id.trim();
    if id.is_empty() {
        return;
    }
    surfaces()
        .get_or_insert_with(HashMap::new)
        .insert(id.to_string(), surface);
}

pub fn session_surface(app_session_id: &str) -> ComputerUseSurface {
    surfaces()
        .as_ref()
        .and_then(|m| m.get(app_session_id.trim()).copied())
        .unwrap_or(ComputerUseSurface::LocalInteractive)
}

pub fn is_session_enabled(app_session_id: &str) -> bool {
    if !feature_enabled() {
        return false;
    }
    sessions()
        .as_ref()
        .map(|s| s.contains(app_session_id.trim()))
        .unwrap_or(false)
}

/// Inject Computer Use MCP when the product flag is on.
/// Independent of `official_aux_with_user_mcp`. Target authorize stays per session.
pub fn should_inject_session_mcp(app_session_id: Option<&str>) -> bool {
    app_session_id
        .is_some_and(|id| is_session_enabled(id) && surface_allows_local(session_surface(id)))
}

#[cfg(feature = "computer-use-probe")]
pub fn run_surface_inject_gates() -> Result<(), String> {
    set_feature_enabled(true);
    set_session_enabled("cu-local", true);
    set_session_surface("cu-local", ComputerUseSurface::LocalInteractive);
    if !should_inject_session_mcp(Some("cu-local")) {
        set_feature_enabled(false);
        return Err("local interactive session must inject when granted".into());
    }
    for (id, surface) in [
        ("cu-im", ComputerUseSurface::RemoteIm),
        ("cu-sched", ComputerUseSurface::Scheduled),
        ("cu-ssh", ComputerUseSurface::Ssh),
    ] {
        set_session_enabled(id, true);
        set_session_surface(id, surface);
        if should_inject_session_mcp(Some(id)) {
            set_feature_enabled(false);
            return Err(format!("{id:?} must not inherit Computer Use"));
        }
    }
    set_feature_enabled(false);
    if should_inject_session_mcp(Some("cu-local")) {
        return Err("disabled flag must not inject session MCP".into());
    }
    match super::mcp_acp_entry("cu-local", "run-off") {
        Err(_) => {}
        Ok(_) => return Err("mcp_acp_entry must fail when Computer Use is off".into()),
    }
    println!("gate: host_surface_inject");
    println!("gate: t2_session_mcp_not_injected_when_disabled");
    Ok(())
}

#[cfg(test)]
mod classify_from_store_tests {
    use super::super::eligibility::run_classify_session_gates;

    #[test]
    fn classify_session_uses_store_and_disk_im() {
        run_classify_session_gates().expect("classify session gates");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_off_and_independent_of_missing_session() {
        set_feature_enabled(false);
        const { assert!(!FEATURE_DEFAULT) };
        assert!(!should_inject_session_mcp(Some("s1")));
        set_feature_enabled(true);
        set_session_enabled("s1", true);
        assert!(should_inject_session_mcp(Some("s1")));
        assert!(!should_inject_session_mcp(Some("s2")));
        assert!(!should_inject_session_mcp(None));
        set_feature_enabled(false);
        assert!(!should_inject_session_mcp(Some("s1")));
    }
}
