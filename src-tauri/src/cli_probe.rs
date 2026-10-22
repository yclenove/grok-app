//! Probe Grok Build CLI on PATH and common locations (B01–B03).
//!
//! Cross-platform notes:
//! - macOS/Windows GUI apps often inherit a sparse PATH (Dock / Explorer), so we
//!   always scan official install dirs + an enriched PATH, not only `which`.
//! - Windows: `.exe` / `.cmd` / `.bat`, PATHEXT, `USERPROFILE`, WinGet/Scoop.
//! - macOS: `~/.grok/bin`, Homebrew Intel/ARM, `~/.local/bin`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::process_util::{self, user_home};

/// Last ACP `agentVersion` observed from `initialize` / TCP probe.
/// Soft cache for Doctor / Runtime honesty — never gates session open.
static LAST_ACP_AGENT_VERSION: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

/// Remember a live ACP agent version banner (best-effort).
pub fn record_acp_agent_version(raw: Option<&str>) {
    record_agent_version_in(&LAST_ACP_AGENT_VERSION, raw);
}

fn record_agent_version_in(cache: &Mutex<Option<String>>, raw: Option<&str>) {
    let Some(s) = raw.map(str::trim).filter(|t| !t.is_empty()) else {
        return;
    };
    if let Ok(mut g) = cache.lock() {
        *g = Some(s.to_string());
    }
}

/// Last recorded ACP agent version, if any session/probe has run this process.
pub fn last_acp_agent_version() -> Option<String> {
    cached_agent_version(&LAST_ACP_AGENT_VERSION)
}

fn cached_agent_version(cache: &Mutex<Option<String>>) -> Option<String> {
    cache.lock().ok().and_then(|g| g.clone())
}

/// Minimum grok CLI version whose flag set `acp_client` is known to work with.
///
/// 0.2.112 is the first version that accepts `doctor --json`; older builds reject
/// it with a raw clap error (`unexpected argument '--'`) and also reject agent
/// flags the host spawns with, which surfaces to the user as `AGENT_CRASHED`
/// with no path to a fix. Bump this together with any change to the flag
/// placement in `acp_client::spawn_with_home`.
pub const MIN_CLI_VERSION: (u64, u64, u64) = (0, 2, 112);

/// Product **recommended** CLI line (Grok Build 1.0). Below this still boots when
/// ≥ [`MIN_CLI_VERSION`]; UI shows a soft upgrade chip, never a hard block.
pub const RECOMMENDED_CLI_VERSION: (u64, u64, u64) = (1, 0, 0);

/// Render [`MIN_CLI_VERSION`] as `x.y.z` for UI copy.
pub fn min_cli_version_str() -> String {
    let (a, b, c) = MIN_CLI_VERSION;
    format!("{a}.{b}.{c}")
}

/// Render [`RECOMMENDED_CLI_VERSION`] as `x.y.z` for UI copy.
pub fn recommended_cli_version_str() -> String {
    let (a, b, c) = RECOMMENDED_CLI_VERSION;
    format!("{a}.{b}.{c}")
}

/// Whether a raw `--version` banner meets the product recommended line.
///
/// - `Some(true)`  — ≥ 1.0.0  
/// - `Some(false)` — parseable but older (still may pass min floor)  
/// - `None`        — unparseable (fail-open, no chip pressure)
pub fn cli_meets_recommended(raw_version: &str) -> Option<bool> {
    let token = extract_version_token(raw_version)?;
    let parsed = crate::app_update::parse_semver(&token)?;
    Some(parsed >= RECOMMENDED_CLI_VERSION)
}

/// Normalize two version banners to semver cores and report skew.
///
/// `true` only when both parse and cores differ. Missing / unparseable → no skew.
pub fn version_tokens_skew(a: Option<&str>, b: Option<&str>) -> bool {
    let ta = a.and_then(extract_version_token);
    let tb = b.and_then(extract_version_token);
    match (ta, tb) {
        (Some(x), Some(y)) => x != y,
        _ => false,
    }
}

/// Resolve the sibling `agent` launcher next to a `grok` path (or official
/// `~/.grok/bin/agent`). Used for install skew detection only — App ACP always
/// spawns `grok`, never this binary.
pub fn resolve_agent_sidecar_path(grok_path: Option<&str>) -> Option<PathBuf> {
    if let Some(g) = grok_path.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(g);
        if let Some(dir) = p.parent() {
            #[cfg(target_os = "windows")]
            {
                let candidate = dir.join("agent.exe");
                if candidate.is_file() || candidate.is_symlink() {
                    return Some(candidate);
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let candidate = dir.join("agent");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    let home_agent = {
        #[cfg(target_os = "windows")]
        {
            user_home().join(r".grok\bin\agent.exe")
        }
        #[cfg(not(target_os = "windows"))]
        {
            user_home().join(".grok/bin/agent")
        }
    };
    if home_agent.exists() {
        Some(home_agent)
    } else {
        None
    }
}

/// Probe version of the `agent` sidecar if present (best-effort, same timeout).
pub fn probe_agent_sidecar(grok_path: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(path) = resolve_agent_sidecar_path(grok_path) else {
        return (None, None);
    };
    let path_s = path.display().to_string();
    if !is_executable(&path) {
        return (Some(path_s), None);
    }
    let ver = read_version(&path);
    (Some(path_s), ver)
}

/// Relink/copy official `~/.grok/bin/agent` to match `~/.grok/bin/grok` (or the
/// probed grok path when under `~/.grok/bin`). Soft-fail when paths missing.
pub fn repair_agent_sidecar_link(grok_path: Option<&str>) -> Result<String, String> {
    let home_bin = user_home().join(".grok").join("bin");
    #[cfg(target_os = "windows")]
    let grok_bin = home_bin.join("grok.exe");
    #[cfg(not(target_os = "windows"))]
    let grok_bin = home_bin.join("grok");

    let grok = grok_path
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .unwrap_or(grok_bin);

    if !grok.exists() {
        return Err("grok binary not found — install or locate CLI first".into());
    }

    #[cfg(target_os = "windows")]
    {
        let agent = home_bin.join("agent.exe");
        std::fs::create_dir_all(&home_bin).map_err(|e| e.to_string())?;
        let old = PathBuf::from(format!("{}.old", agent.display()));
        let _ = std::fs::remove_file(&old);
        if std::fs::copy(&grok, &agent).is_err() {
            let _ = std::fs::rename(&agent, &old);
            std::fs::copy(&grok, &agent).map_err(|e| format!("copy agent.exe: {e}"))?;
        }
        Ok(agent.display().to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let agent = home_bin.join("agent");
        std::fs::create_dir_all(&home_bin).map_err(|e| e.to_string())?;
        // Prefer same relative target as grok when it is a symlink.
        let link_target = if grok
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            std::fs::read_link(&grok).unwrap_or_else(|_| grok.clone())
        } else {
            // Absolute path to the real binary when grok is a plain file.
            std::fs::canonicalize(&grok).unwrap_or_else(|_| grok.clone())
        };
        let _ = std::fs::remove_file(&agent);
        std::os::unix::fs::symlink(&link_target, &agent)
            .map_err(|e| format!("symlink agent → {}: {e}", link_target.display()))?;
        Ok(agent.display().to_string())
    }
}

/// Pull the first semver-looking token out of a `--version` line.
///
/// The CLI prints shapes like `grok 0.2.112`, `grok-cli v0.2.112 (abc1234)`, or
/// bare `0.2.112`; we only care about the numeric core.
pub fn extract_version_token(raw: &str) -> Option<String> {
    raw.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .map(|t| t.trim().trim_start_matches(['v', 'V']))
        .find(|t| {
            // Require at least `major.minor` with a leading digit so words like
            // "grok" or a git hash are not mistaken for a version.
            let mut parts = t.split('.');
            let major = parts.next().unwrap_or("");
            let minor = parts.next().unwrap_or("");
            !major.is_empty()
                && major.chars().all(|c| c.is_ascii_digit())
                && !minor.is_empty()
                && minor.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(|t| t.to_string())
}

/// Compare a raw `--version` line against [`MIN_CLI_VERSION`].
///
/// Returns `None` when the version cannot be parsed — an unknown version is
/// **not** treated as too old, so a CLI build with an unusual banner still
/// runs instead of being blocked by a false positive.
pub fn cli_version_supported(raw_version: &str) -> Option<bool> {
    let token = extract_version_token(raw_version)?;
    let parsed = crate::app_update::parse_semver(&token)?;
    Some(parsed >= MIN_CLI_VERSION)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliProbeResult {
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: String,
    pub candidates_tried: Vec<String>,
    /// CLI auth material present at ~/.grok/auth.json (not App secrets).
    pub cli_auth_present: bool,
    /// `Some(false)` when the CLI is older than [`MIN_CLI_VERSION`].
    /// `None` when the version could not be probed or parsed (NEW-03).
    pub version_supported: Option<bool>,
    /// Minimum version this app requires, so the UI need not hardcode it.
    pub min_version: String,
    /// Product recommended CLI line (Grok Build 1.0+). Soft guidance only.
    #[serde(default)]
    pub recommended_version: String,
    /// `Some(true)` when version ≥ recommended; `Some(false)` when older;
    /// `None` when unparseable / missing.
    #[serde(default)]
    pub meets_recommended: Option<bool>,
    /// Path of sibling `agent` binary if found (not used for App ACP spawn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_path: Option<String>,
    /// `agent --version` banner when readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_version: Option<String>,
    /// True when both `grok` and `agent` versions parse and differ.
    #[serde(default)]
    pub agent_binary_skew: bool,
    /// Last live ACP `initialize` / TCP-probe `agentVersion` (process cache).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_agent_version: Option<String>,
    /// True when probe `grok --version` and live ACP agentVersion cores differ.
    /// Soft-only: never blocks session open.
    #[serde(default)]
    pub acp_agent_version_skew: bool,
}

pub fn cli_auth_json_present() -> bool {
    user_home().join(".grok").join("auth.json").is_file()
}

/// Expand `~` / `%USERPROFILE%` / `%HOME%` so manual paths work on both OSes.
pub fn expand_user_path(raw: &str) -> PathBuf {
    let s = raw.trim();
    if s.is_empty() {
        return PathBuf::new();
    }

    // Unix-style home prefix
    if s == "~" {
        return user_home();
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        return user_home().join(rest);
    }

    // Windows-style env vars (common when pasting paths)
    let mut out = s.to_string();
    for (key, val) in [
        ("%USERPROFILE%", std::env::var("USERPROFILE").ok()),
        (
            "%HOME%",
            std::env::var("HOME")
                .ok()
                .or_else(|| std::env::var("USERPROFILE").ok()),
        ),
        ("%LOCALAPPDATA%", std::env::var("LOCALAPPDATA").ok()),
        ("%APPDATA%", std::env::var("APPDATA").ok()),
    ] {
        if let Some(v) = val {
            // case-insensitive replace for %VAR%
            let lower = out.to_ascii_lowercase();
            let key_l = key.to_ascii_lowercase();
            if let Some(idx) = lower.find(&key_l) {
                let mut next = String::new();
                next.push_str(&out[..idx]);
                next.push_str(&v);
                next.push_str(&out[idx + key.len()..]);
                out = next;
            }
        }
    }

    PathBuf::from(out)
}

/// Binary basenames to look for (Windows includes extension variants).
fn binary_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["grok.exe", "grok.cmd", "grok.bat", "grok"]
    }
    #[cfg(not(target_os = "windows"))]
    {
        &["grok"]
    }
}

fn push_unique(out: &mut Vec<PathBuf>, seen: &mut std::collections::HashSet<String>, p: PathBuf) {
    if p.as_os_str().is_empty() {
        return;
    }
    // Normalize for dedupe (Windows is case-insensitive)
    let key = {
        #[cfg(target_os = "windows")]
        {
            p.to_string_lossy().to_ascii_lowercase()
        }
        #[cfg(not(target_os = "windows"))]
        {
            p.to_string_lossy().to_string()
        }
    };
    if seen.insert(key) {
        out.push(p);
    }
}

fn push_bin_in_dir(
    out: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<String>,
    dir: PathBuf,
) {
    if dir.as_os_str().is_empty() {
        return;
    }
    for name in binary_names() {
        push_unique(out, seen, dir.join(name));
    }
}

/// Resolve `which` using the process PATH and, if needed, an enriched PATH
/// (GUI apps often miss `~/.grok/bin` / Homebrew / WinGet).
fn which_grok_variants() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let names = binary_names();

    for name in names {
        if let Ok(p) = which::which(name) {
            found.push(p);
        }
    }

    // Search enriched PATH directories explicitly (does not mutate process env).
    if let Some(enriched) = process_util::enriched_path_env() {
        let sep = process_util::path_list_separator();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for name in names {
            if let Ok(p) = which::which_in(name, Some(&enriched), &cwd) {
                found.push(p);
            }
        }
        // Also brute-force join (covers dirs that exist but which_in misses).
        for dir in enriched.split(sep).filter(|d| !d.is_empty()) {
            for name in names {
                let p = PathBuf::from(dir).join(name);
                if p.is_file() {
                    found.push(p);
                }
            }
        }
    }

    found
}

fn candidate_paths(manual: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // 1) Manual path (settings / file picker) — highest priority
    if let Some(m) = manual {
        if !m.trim().is_empty() {
            let expanded = expand_user_path(m);
            push_unique(&mut out, &mut seen, expanded.clone());
            // Windows: user may omit .exe
            #[cfg(target_os = "windows")]
            {
                if expanded.extension().is_none() {
                    push_unique(&mut out, &mut seen, expanded.with_extension("exe"));
                }
            }
        }
    }

    // 2) Official default install layout (xAI install.sh / install.ps1)
    //    Prefer these over ambient PATH so GUI apps match CLI installs.
    let home = user_home();
    #[cfg(target_os = "windows")]
    {
        push_bin_in_dir(&mut out, &mut seen, home.join(r".grok\bin"));
        // Versioned downloads left by installer (before link/copy)
        if let Ok(rd) = std::fs::read_dir(home.join(r".grok\downloads")) {
            for ent in rd.flatten() {
                let p = ent.path();
                let name = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if name.starts_with("grok-")
                    && (name.ends_with(".exe") || !name.contains('.'))
                    && !name.ends_with(".part")
                    && !name.ends_with(".tmp")
                    && !name.ends_with(".old")
                {
                    push_unique(&mut out, &mut seen, p);
                }
            }
        }
        push_bin_in_dir(&mut out, &mut seen, home.join(r".local\bin"));
        // Scoop shims
        push_bin_in_dir(&mut out, &mut seen, home.join(r"scoop\shims"));
        // WinGet / Local programs
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            push_bin_in_dir(&mut out, &mut seen, local.join(r"Microsoft\WinGet\Links"));
            push_bin_in_dir(&mut out, &mut seen, local.join(r"Programs\grok"));
            push_bin_in_dir(&mut out, &mut seen, local.join(r"grok\bin"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            push_bin_in_dir(&mut out, &mut seen, PathBuf::from(pf).join("grok"));
        }
        // Chocolatey
        push_bin_in_dir(
            &mut out,
            &mut seen,
            PathBuf::from(r"C:\ProgramData\chocolatey\bin"),
        );
    }
    #[cfg(not(target_os = "windows"))]
    {
        push_bin_in_dir(&mut out, &mut seen, home.join(".grok/bin"));
        // Versioned downloads (symlink targets)
        if let Ok(rd) = std::fs::read_dir(home.join(".grok/downloads")) {
            for ent in rd.flatten() {
                let p = ent.path();
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with("grok-") && !name.ends_with(".part") && !name.contains(".tmp") {
                    push_unique(&mut out, &mut seen, p);
                }
            }
        }
        push_bin_in_dir(&mut out, &mut seen, home.join(".local/bin"));
        push_bin_in_dir(&mut out, &mut seen, home.join(".cargo/bin"));
        push_bin_in_dir(&mut out, &mut seen, home.join(".bun/bin"));
        push_bin_in_dir(&mut out, &mut seen, PathBuf::from("/opt/homebrew/bin"));
        push_bin_in_dir(&mut out, &mut seen, PathBuf::from("/usr/local/bin"));
        push_bin_in_dir(&mut out, &mut seen, PathBuf::from("/usr/bin"));
    }

    // 3) PATH / which (process + enriched)
    for p in which_grok_variants() {
        push_unique(&mut out, &mut seen, p);
    }

    out
}

fn is_executable(path: &Path) -> bool {
    process_util::looks_runnable(path)
}

/// Public wrapper so spawn paths can gate on version without a full probe.
/// One extra `--version` exec per session start, which is negligible next to
/// spawning the agent itself.
pub fn read_version_of(path: &Path) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::SystemTime;

    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (SystemTime, String)>>> = OnceLock::new();
    // Eligibility must be checked before the mtime cache: removal or revoked
    // execute permission cannot leave a previously probed CLI available.
    if !is_executable(path) {
        if let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            cache.remove(path);
        }
        return None;
    }
    let mtime = fs::metadata(path).and_then(|m| m.modified()).ok();
    if let Some(mt) = mtime {
        if let Ok(g) = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            if let Some((cached_mt, ver)) = g.get(path) {
                if *cached_mt == mt {
                    return Some(ver.clone());
                }
            }
        }
    }
    let ver = read_version(path)?;
    if let Some(mt) = mtime {
        if let Ok(mut g) = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
            g.insert(path.to_path_buf(), (mt, ver.clone()));
        }
    }
    Some(ver)
}

/// Wall-clock budget for a single `grok --version` (hung binaries must not
/// freeze the setup gate / async runtime forever).
const VERSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn read_version(path: &Path) -> Option<String> {
    use std::process::Stdio;
    use std::time::Instant;

    // Missing Windows scripts can produce a shell error containing "grok".
    // That error is not a CLI version banner and must never count as one.
    if !is_executable(path) {
        return None;
    }
    let mut cmd = Command::new(path);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // GUI-spawned probes: PATH + HOME + no console window (Windows).
    process_util::apply_cli_env_std(&mut cmd);

    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + VERSION_PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = {
                    let mut s = String::new();
                    if let Some(mut pipe) = child.stdout.take() {
                        use std::io::Read;
                        let _ = pipe.read_to_string(&mut s);
                    }
                    s
                };
                let stderr = {
                    let mut s = String::new();
                    if let Some(mut pipe) = child.stderr.take() {
                        use std::io::Read;
                        let _ = pipe.read_to_string(&mut s);
                    }
                    s
                };
                if status.success() {
                    let line = stdout.lines().next()?.trim().to_string();
                    return if line.is_empty() { None } else { Some(line) };
                }
                // Some builds print version on stderr
                let line = stderr.lines().next()?.trim().to_string();
                if !line.is_empty() && line.to_ascii_lowercase().contains("grok") {
                    return Some(line);
                }
                return None;
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    tracing::warn!(
                        path = %path.display(),
                        "cli --version timed out after {}s — killing",
                        VERSION_PROBE_TIMEOUT.as_secs()
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => return None,
        }
    }
}

fn classify_source(path: &Path, manual_first: bool) -> String {
    if manual_first {
        return "manual".into();
    }
    let s = path.to_string_lossy();
    let lower = s.to_ascii_lowercase();
    if lower.contains(".grok") {
        "common_path".into()
    } else {
        "path".into()
    }
}

pub fn probe_cli(manual_path: Option<&str>) -> CliProbeResult {
    probe_candidates(
        candidate_paths(manual_path),
        manual_path.map(|m| !m.trim().is_empty()).unwrap_or(false),
        cli_auth_json_present(),
        is_executable,
        read_version,
        probe_agent_sidecar,
        last_acp_agent_version,
    )
}

/// Shared selection logic. Explicit inputs let tests exercise the production
/// algorithm without discovering or running an ambient user installation.
pub(crate) fn probe_candidates(
    candidates: Vec<PathBuf>,
    manual_set: bool,
    cli_auth_present: bool,
    mut runnable: impl FnMut(&Path) -> bool,
    mut version_probe: impl FnMut(&Path) -> Option<String>,
    sidecar_probe: impl FnOnce(Option<&str>) -> (Option<String>, Option<String>),
    acp_version: impl FnOnce() -> Option<String>,
) -> CliProbeResult {
    let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();

    // Prefer a candidate that both looks runnable AND answers --version.
    let mut fallback = None;
    let mut selected = None;

    for (i, path) in candidates.iter().enumerate() {
        if !runnable(path) {
            continue;
        }
        let source = classify_source(path, manual_set && i == 0);
        if let Some(version) = version_probe(path) {
            selected = Some((path.clone(), source, Some(version)));
            break;
        }
        if fallback.is_none() {
            fallback = Some((path.clone(), source, None));
        }
    }

    // Runnable file that failed --version still counts as "found" so the user
    // can try connecting; UI can show missing version.
    finish_probe_result(
        selected.or(fallback),
        tried,
        cli_auth_present,
        sidecar_probe,
        acp_version,
    )
}

fn finish_probe_result(
    selection: Option<(PathBuf, String, Option<String>)>,
    candidates_tried: Vec<String>,
    cli_auth_present: bool,
    sidecar_probe: impl FnOnce(Option<&str>) -> (Option<String>, Option<String>),
    acp_version: impl FnOnce() -> Option<String>,
) -> CliProbeResult {
    let (path, source, version) = match selection {
        Some((path, source, version)) => (Some(path.display().to_string()), source, version),
        None => (None, "not_found".into(), None),
    };
    let found = path.is_some();
    let version_supported = version.as_deref().and_then(cli_version_supported);
    let meets_recommended = version.as_deref().and_then(cli_meets_recommended);
    let (agent_path, agent_version) = if found {
        sidecar_probe(path.as_deref())
    } else {
        (None, None)
    };
    let agent_binary_skew = version_tokens_skew(version.as_deref(), agent_version.as_deref());
    let acp_agent_version = acp_version();
    let acp_agent_version_skew =
        version_tokens_skew(version.as_deref(), acp_agent_version.as_deref());
    CliProbeResult {
        found,
        path,
        version,
        source,
        candidates_tried,
        cli_auth_present,
        version_supported,
        min_version: min_cli_version_str(),
        recommended_version: recommended_cli_version_str(),
        meets_recommended,
        agent_path,
        agent_version,
        agent_binary_skew,
        acp_agent_version,
        acp_agent_version_skew,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) struct LauncherDir(PathBuf);

    impl LauncherDir {
        pub(crate) fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("grok-synthetic-cli-{}", uuid::Uuid::new_v4()));
            Self::create(path).expect("create owned synthetic CLI directory")
        }

        pub(crate) fn create(path: PathBuf) -> std::io::Result<Self> {
            // Acquire cleanup ownership only after exclusive creation succeeds.
            // A collision must not remove the directory owned by somebody else.
            fs::create_dir(&path)?;
            Ok(Self(path))
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for LauncherDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Only a temporary, synthetic script is executable; never a user's CLI.
    pub(crate) fn version_launcher(
        root: &Path,
        banner: &str,
        stderr: bool,
        exit_code: u8,
    ) -> PathBuf {
        assert!(
            banner
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b" .()-".contains(&byte)),
            "synthetic banner must not contain shell syntax"
        );
        #[cfg(target_os = "windows")]
        let (path, body) = {
            let redirect = if stderr { " >&2" } else { "" };
            (
                root.join("grok.cmd"),
                format!(
                    "@echo off\r\nif not \"%~1\"==\"--version\" exit /b 9\r\necho {banner}{redirect}\r\nexit /b {exit_code}\r\n"
                ),
            )
        };
        #[cfg(not(target_os = "windows"))]
        let (path, body) = {
            let redirect = if stderr { " >&2" } else { "" };
            (
                root.join("grok"),
                format!(
                    "#!/bin/sh\n[ \"$1\" = \"--version\" ] || exit 9\nprintf '%s\\n' '{banner}'{redirect}\nexit {exit_code}\n"
                ),
            )
        };
        fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        path
    }
}

#[cfg(test)]
#[path = "cli_probe_tests.rs"]
mod tests;
