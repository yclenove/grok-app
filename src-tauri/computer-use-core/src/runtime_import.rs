//! Bounded, pack-only module loading. This does not launch or attest a browser.

use super::PrepareError;
use crate::runtime::{REQUIRED_JS_RUNTIME_VERSION, REQUIRED_PLAYWRIGHT_CORE_VERSION};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const IMPORT_TIMEOUT: Duration = Duration::from_secs(15);
const PROBE: &str = r#"
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
const [root, nodeVersion, playwrightVersion, receipt, nonce] = process.argv.slice(1);
try {
  const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
  if (pkg.name !== 'playwright-core') process.exit(41);
  if (process.versions.node !== nodeVersion || pkg.version !== playwrightVersion) process.exit(42);
  const { chromium } = await import(pathToFileURL(join(root, 'index.mjs')).href);
  if (!chromium || typeof chromium.name !== 'function' || chromium.name() !== 'chromium' ||
      ['launch', 'launchPersistentContext', 'connectOverCDP'].some(key => typeof chromium[key] !== 'function')) {
    process.exit(43);
  }
  writeFileSync(receipt, nonce, { flag: 'wx' });
  process.exit(0);
} catch {
  process.exit(40);
}
"#;

struct ProbeDirectory(PathBuf);

impl ProbeDirectory {
    fn create() -> Result<Self, PrepareError> {
        let path = std::env::temp_dir().join(format!("grok-cu-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)
            .map_err(|_| PrepareError::new("io", "cannot create import workspace"))?;
        Ok(Self(path))
    }
}

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        // Only the exact UUID directory created by this invocation is owned here.
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ProbeChild(Child);

impl Drop for ProbeChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn wait_for_probe(child: &mut Child, timeout: Duration) -> Result<ExitStatus, PrepareError> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                child.kill().map_err(|_| {
                    PrepareError::new("import_cleanup_failed", "cannot terminate import probe")
                })?;
                child.wait().map_err(|_| {
                    PrepareError::new("import_cleanup_failed", "cannot reap import probe")
                })?;
                return Err(PrepareError::new(
                    "import_timeout",
                    "module import exceeded its deadline",
                ));
            }
            Err(_) => {
                return Err(PrepareError::new(
                    "import_failed",
                    "cannot observe import probe",
                ))
            }
        }
    }
}

pub(crate) fn check_playwright_import(node: &Path, pw: &Path) -> Result<(), PrepareError> {
    check_with_timeout(node, pw, IMPORT_TIMEOUT)
}

fn check_with_timeout(node: &Path, pw: &Path, timeout: Duration) -> Result<(), PrepareError> {
    // Resolve the explicitly supplied pack files before starting: never search PATH.
    let node = fs::canonicalize(node)
        .map_err(|_| PrepareError::new("import_failed", "pack Node executable is missing"))?;
    let pw = fs::canonicalize(pw)
        .map_err(|_| PrepareError::new("import_failed", "pack Playwright directory is missing"))?;
    let workspace = ProbeDirectory::create()?;
    let empty_path = workspace.0.join("empty-path");
    fs::create_dir(&empty_path)
        .map_err(|_| PrepareError::new("io", "cannot create isolated import PATH"))?;
    let receipt = workspace.0.join("receipt");
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut command = Command::new(node);
    command
        .args(["--input-type=module", "--eval", PROBE, "--"])
        .arg(pw)
        .arg(REQUIRED_JS_RUNTIME_VERSION)
        .arg(REQUIRED_PLAYWRIGHT_CORE_VERSION)
        .arg(&receipt)
        .arg(&nonce)
        .current_dir(&workspace.0)
        .env_clear()
        .env("PATH", empty_path)
        .stdin(Stdio::null())
        // No unbounded pipes or module-controlled diagnostics in logs.
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW.
    }
    let mut child = ProbeChild(command.spawn().map_err(|_| {
        PrepareError::new("import_failed", "cannot start the packaged Node executable")
    })?);
    let status = wait_for_probe(&mut child.0, timeout)?;
    match status.code() {
        Some(42) => {
            return Err(PrepareError::new(
                "version_mismatch",
                "pack Node or Playwright version mismatch",
            ))
        }
        Some(41) => {
            return Err(PrepareError::new(
                "import_failed",
                "unexpected Playwright package identity",
            ))
        }
        Some(43) => {
            return Err(PrepareError::new(
                "import_failed",
                "required Chromium APIs are missing",
            ))
        }
        Some(0) => {}
        _ => {
            return Err(PrepareError::new(
                "import_failed",
                "Playwright module could not be loaded",
            ))
        }
    }
    // An entry that exits(0) before the checks must not be accepted as a pass.
    if fs::metadata(&receipt).map(|m| m.len()).ok() != Some(nonce.len() as u64)
        || fs::read(&receipt).ok().as_deref() != Some(nonce.as_bytes())
    {
        return Err(PrepareError::new(
            "import_failed",
            "module import did not produce a completion receipt",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_import_tests.rs"]
mod tests;
