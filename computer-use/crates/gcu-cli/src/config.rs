//! Read-only minimal configuration; unrelated future sections are ignored.
use crate::exit::Failure;
use gcu_host_api::PolicyConfig;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Default, Deserialize)]
struct Config {
    #[serde(default)]
    observe: Observe,
    #[serde(default)]
    policy: Policy,
    #[serde(default)]
    guard: Guard,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Observe {
    max_tree_nodes: Option<u32>,
    max_image_edge: Option<u32>,
    image_format: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    extra_denied_apps: Option<Vec<String>>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Guard {
    user_idle_ms: Option<u64>,
    stop_hotkey: Option<String>,
}

pub(crate) fn path() -> Result<PathBuf, Failure> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root =
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
    Ok(root
        .ok_or_else(|| Failure::arguments("Cannot resolve the user configuration directory"))?
        .join("grok-computer-use/config.toml"))
}
pub(crate) fn load() -> Result<PolicyConfig, Failure> {
    let path = path()?;
    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PolicyConfig::default()),
        Err(_) => return Err(Failure::arguments("Cannot read config.toml")),
    };
    parse(&source)
}
fn parse(source: &str) -> Result<PolicyConfig, Failure> {
    let c: Config = toml::from_str(source).map_err(|_| {
        Failure::arguments("Invalid config.toml: check [observe], [policy], [guard] types and keys")
    })?;
    let mut p = PolicyConfig::default();
    if let Some(v) = c.observe.max_tree_nodes {
        p.max_tree_nodes = v;
    }
    if let Some(v) = c.observe.max_image_edge {
        p.max_image_edge = v;
    }
    if let Some(v) = c.observe.image_format {
        p.image_format = v;
    }
    if let Some(v) = c.policy.extra_denied_apps {
        p.extra_denied_apps = v;
    }
    if let Some(v) = c.guard.user_idle_ms {
        p.user_idle_ms = v;
    }
    if let Some(v) = c.guard.stop_hotkey {
        p.stop_hotkey = v;
    }
    if !(1..=5000).contains(&p.max_tree_nodes)
        || !(1..=2560).contains(&p.max_image_edge)
        || !matches!(p.image_format.as_str(), "png" | "jpeg")
        || p.stop_hotkey.trim().is_empty()
    {
        return Err(Failure::arguments(
            "Invalid config.toml: observation budgets/format or stop_hotkey",
        ));
    }
    Ok(p)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_supported_subset_applies_without_grants() {
        let p = parse("[observe]\nmax_tree_nodes=40\nmax_image_edge=400\nimage_format='jpeg'\n[policy]\nextra_denied_apps=['fake.form']\n[guard]\nuser_idle_ms=12\nstop_hotkey='default'\n[approval]\nalways=[]").ok().unwrap();
        assert_eq!(p.max_tree_nodes, 40);
        assert_eq!(p.max_image_edge, 400);
        assert_eq!(p.image_format, "jpeg");
        assert_eq!(p.extra_denied_apps, ["fake.form"]);
        assert_eq!(p.user_idle_ms, 12);
        assert!(p.always.is_empty());
    }
}
