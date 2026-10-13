//! Per-target Computer Use runtime source locks.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime_pins::{
    REQUIRED_CHROMIUM_REVISION, REQUIRED_CHROMIUM_VERSION, REQUIRED_JS_RUNTIME_VERSION,
    REQUIRED_PLAYWRIGHT_CORE_VERSION,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTarget {
    WindowsX64,
    MacosArm64,
    MacosX64,
    LinuxX64,
}

impl RuntimeTarget {
    pub fn parse(value: &str) -> Result<Self, LockError> {
        match value {
            "x86_64-windows" | "x86_64-pc-windows-msvc" => Ok(Self::WindowsX64),
            "aarch64-macos" | "aarch64-apple-darwin" => Ok(Self::MacosArm64),
            "x86_64-macos" | "x86_64-apple-darwin" => Ok(Self::MacosX64),
            "x86_64-linux" | "x86_64-unknown-linux-gnu" => Ok(Self::LinuxX64),
            _ => Err(LockError {
                code: "unsupported_target".into(),
                message: format!("no Computer Use runtime target {value}"),
            }),
        }
    }

    pub fn arch(self) -> &'static str {
        match self {
            Self::WindowsX64 => "x86_64-windows",
            Self::MacosArm64 => "aarch64-macos",
            Self::MacosX64 => "x86_64-macos",
            Self::LinuxX64 => "x86_64-linux",
        }
    }

    pub fn lock_name(self) -> &'static str {
        match self {
            Self::WindowsX64 => "windows-x64.lock.json",
            Self::MacosArm64 => "macos-arm64.lock.json",
            Self::MacosX64 => "macos-x64.lock.json",
            Self::LinuxX64 => "linux-x64.lock.json",
        }
    }

    pub fn node_relpath(self) -> &'static str {
        if self == Self::WindowsX64 {
            "bin/node.exe"
        } else {
            "bin/node"
        }
    }

    pub fn node_archive_root(self) -> String {
        let platform = match self {
            Self::WindowsX64 => "win-x64",
            Self::MacosArm64 => "darwin-arm64",
            Self::MacosX64 => "darwin-x64",
            Self::LinuxX64 => "linux-x64",
        };
        format!("node-v{REQUIRED_JS_RUNTIME_VERSION}-{platform}")
    }

    pub fn node_archive_executable(self) -> &'static str {
        if self == Self::WindowsX64 {
            "node.exe"
        } else {
            "bin/node"
        }
    }

    pub fn chromium_root(self) -> &'static str {
        match self {
            Self::WindowsX64 => "chrome-win",
            Self::MacosArm64 | Self::MacosX64 => "chrome-mac",
            Self::LinuxX64 => "chrome-linux",
        }
    }

    pub fn chromium_executable(self) -> &'static str {
        match self {
            Self::WindowsX64 => "chrome.exe",
            Self::MacosArm64 | Self::MacosX64 => "Chromium.app/Contents/MacOS/Chromium",
            Self::LinuxX64 => "chrome",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLock {
    pub target: String,
    pub allowed_redirect_hosts: Vec<String>,
    pub js_runtime: JsRuntimeLock,
    pub playwright: PlaywrightLock,
    pub chromium: ChromiumLock,
}

/// Kept as a type alias for existing Windows fixtures; the lock always declares
/// its canonical target and is validated against the requested build target.
pub type WindowsX64Lock = RuntimeLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsRuntimeLock {
    pub name: String,
    pub version: String,
    pub url: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub archive_root: String,
    pub executable_relpath: String,
    pub executable_sha256: String,
    pub executable_bytes: u64,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaywrightLock {
    pub name: String,
    pub version: String,
    pub url: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub tree_relpath: String,
    pub tree_sha256: String,
    pub tree_files: u32,
    pub tree_bytes: u64,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromiumLock {
    pub name: String,
    pub source: String,
    pub revision: String,
    pub browser_version: String,
    pub platform: String,
    pub url: String,
    pub mirrors: Vec<String>,
    pub archive_relpath: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub archive_root: String,
    pub tree_relpath: String,
    pub tree_sha256: String,
    pub tree_files: u32,
    pub tree_bytes: u64,
    pub executable_relpath: String,
    pub executable_sha256: String,
    pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockError {
    pub code: String,
    pub message: String,
}

pub fn shipped_lock_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("resources")
        .join("computer-use")
        .join("windows-x64.lock.json")
}

pub fn load_lock(path: &Path) -> Result<WindowsX64Lock, LockError> {
    let raw = fs::read(path).map_err(|e| LockError {
        code: "missing".into(),
        message: e.to_string(),
    })?;
    parse_lock(&raw)
}

pub fn parse_lock(raw: &[u8]) -> Result<WindowsX64Lock, LockError> {
    let lock: WindowsX64Lock = serde_json::from_slice(raw).map_err(|e| LockError {
        code: "schema".into(),
        message: e.to_string(),
    })?;
    validate_lock(&lock)?;
    Ok(lock)
}

pub fn validate_lock(lock: &WindowsX64Lock) -> Result<(), LockError> {
    let target = RuntimeTarget::parse(&lock.target)?;
    if lock.target != target.arch() {
        return Err(LockError {
            code: "schema".into(),
            message: "runtime lock must use the canonical target architecture".into(),
        });
    }
    if lock.js_runtime.version != REQUIRED_JS_RUNTIME_VERSION {
        return Err(LockError {
            code: "version_mismatch".into(),
            message: "js-runtime version does not match required pin".into(),
        });
    }
    if lock.playwright.version != REQUIRED_PLAYWRIGHT_CORE_VERSION {
        return Err(LockError {
            code: "version_mismatch".into(),
            message: "playwright version does not match required pin".into(),
        });
    }
    if lock.chromium.revision != REQUIRED_CHROMIUM_REVISION
        || lock.chromium.browser_version != REQUIRED_CHROMIUM_VERSION
    {
        return Err(LockError {
            code: "version_mismatch".into(),
            message: "chromium pin does not match required revision".into(),
        });
    }
    if lock.js_runtime.archive_root != target.node_archive_root()
        || lock.js_runtime.executable_relpath != target.node_archive_executable()
        || lock.playwright.tree_relpath != "playwright/node_modules/playwright-core"
        || lock.chromium.archive_root != target.chromium_root()
        || lock.chromium.tree_relpath != format!("chromium/{}", target.chromium_root())
        || lock.chromium.executable_relpath != target.chromium_executable()
    {
        return Err(LockError {
            code: "layout_mismatch".into(),
            message: "runtime lock layout does not match the compiled target contract".into(),
        });
    }
    for field in [
        &lock.js_runtime.archive_sha256,
        &lock.js_runtime.executable_sha256,
        &lock.playwright.archive_sha256,
        &lock.playwright.tree_sha256,
        &lock.chromium.archive_sha256,
        &lock.chromium.tree_sha256,
        &lock.chromium.executable_sha256,
    ] {
        if field.len() != 64 || !field.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(LockError {
                code: "schema".into(),
                message: "lock hashes must be 64-char sha256 hex".into(),
            });
        }
    }
    if lock.js_runtime.archive_bytes == 0
        || lock.playwright.archive_bytes == 0
        || lock.chromium.archive_bytes == 0
    {
        return Err(LockError {
            code: "schema".into(),
            message: "lock archive bytes must be non-zero".into(),
        });
    }
    for (files, bytes, archive, limits) in [
        (
            lock.chromium.tree_files,
            lock.chromium.tree_bytes,
            lock.chromium.archive_bytes,
            crate::runtime_limits::chromium(),
        ),
        (
            lock.playwright.tree_files,
            lock.playwright.tree_bytes,
            lock.playwright.archive_bytes,
            crate::runtime_limits::playwright(),
        ),
        (
            1,
            lock.js_runtime.executable_bytes,
            lock.js_runtime.archive_bytes,
            crate::runtime_limits::node(),
        ),
    ] {
        if files > limits.max_files
            || bytes > limits.max_total_bytes
            || archive > limits.max_archive_bytes
        {
            return Err(LockError {
                code: "oversize".into(),
                message: "runtime lock exceeds compile-owned resource limits".into(),
            });
        }
    }
    Ok(())
}

pub fn host_allowed(lock: &WindowsX64Lock, host: &str) -> bool {
    let host = host.trim().to_ascii_lowercase();
    lock.allowed_redirect_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(&host))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playwright_materialize::sha256_hex;
    use crate::runtime::PLAYWRIGHT_CORE_TGZ_SHA256;

    #[test]
    fn shipped_windows_x64_lock_matches_runtime_pins() {
        let lock = load_lock(&shipped_lock_path()).expect("load shipped lock");
        assert_eq!(lock.target, "x86_64-windows");
        assert_eq!(lock.js_runtime.version, REQUIRED_JS_RUNTIME_VERSION);
        assert_eq!(lock.playwright.version, REQUIRED_PLAYWRIGHT_CORE_VERSION);
        assert_eq!(lock.playwright.archive_sha256, PLAYWRIGHT_CORE_TGZ_SHA256);
        assert_eq!(lock.chromium.revision, "1140");
        assert_eq!(lock.chromium.browser_version, "130.0.6723.31");
        let raw = fs::read(shipped_lock_path()).unwrap();
        let again = parse_lock(&raw).unwrap();
        assert_eq!(lock, again);
        assert_eq!(sha256_hex(&raw).len(), 64);
        assert!(host_allowed(&lock, "playwright.azureedge.net"));
        assert!(!host_allowed(&lock, "evil.example"));
    }

    #[test]
    fn lock_rejects_wrong_target_and_truncated_hash() {
        let mut lock = load_lock(&shipped_lock_path()).unwrap();
        lock.target = "riscv64-linux".into();
        assert_eq!(validate_lock(&lock).unwrap_err().code, "unsupported_target");
        lock = load_lock(&shipped_lock_path()).unwrap();
        lock.playwright.archive_sha256 = "abcd".into();
        assert_eq!(validate_lock(&lock).unwrap_err().code, "schema");
    }
}
