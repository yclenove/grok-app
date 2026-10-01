//! D9: lock tauri-bundler 2.11.5 NSIS drift and Windows bundle deny list.

use std::path::{Path, PathBuf};

const LOCKED_TAURI: &str = "2.11.5";

const REQUIRED_FRAGMENTS: &[&str] = &[
    "ManifestDPIAware true",
    "ManifestDPIAwareness PerMonitorV2",
    r#"!define WEBVIEW2APPGUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}""#,
    r#"!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCTNAME}""#,
    r#"File "${MAINBINARYSRCPATH}""#,
    r#"!insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}""#,
    "Section WebView2",
    r#"WriteUninstaller "$INSTDIR\uninstall.exe""#,
    "nsis_tauri_utils::RunAsUser",
    "nsis_tauri_utils::SemverCompare",
    "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Run\"",
];

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn locked_tauri_version(lockfile: &str) -> Option<String> {
    let mut saw_name = false;
    for line in lockfile.lines() {
        let t = line.trim();
        if t == "name = \"tauri\"" {
            saw_name = true;
            continue;
        }
        if saw_name && t.starts_with("version = ") {
            return t
                .strip_prefix("version = \"")
                .and_then(|s| s.strip_suffix('"'))
                .map(str::to_string);
        }
        if t.starts_with('[') {
            saw_name = false;
        }
    }
    None
}

pub fn install_section(src: &str) -> Option<&str> {
    let start = src.find("\nSection Install")?;
    let body = &src[start + 1..];
    let end = body.find("\nSectionEnd")?;
    Some(&body[..end])
}

pub fn copies_extra_cargo_bins(install: &str) -> bool {
    install.contains("Copy external binaries")
        && install.contains("{{#each binaries}}")
        && install.contains(r#"File /a "/oname={{this}}""#)
}

pub fn nsis_drift_violations(ours: &str, lockfile: &str) -> Vec<String> {
    let mut hits = Vec::new();
    match locked_tauri_version(lockfile).as_deref() {
        Some(LOCKED_TAURI) => {}
        Some(other) => hits.push(format!(
            "tauri crate is {other}, NSIS fork must be re-reviewed against tauri-bundler {LOCKED_TAURI}"
        )),
        None => hits.push("Cargo.lock missing tauri version".into()),
    }
    if !ours.contains("Forked from tauri-bundler 2.11.5") {
        hits.push("installer.nsi must declare the 2.11.5 fork".into());
    }
    for frag in REQUIRED_FRAGMENTS {
        if !ours.contains(frag) {
            hits.push(format!("missing upstream fragment: {frag}"));
        }
    }
    let Some(install) = install_section(ours) else {
        hits.push("Section Install missing".into());
        return hits;
    };
    if copies_extra_cargo_bins(install) {
        hits.push("Install section still copies extra cargo bins (cu_probe would ship)".into());
    }
    if !ours.contains(r#"Delete "$INSTDIR\\cu_probe.exe""#) {
        hits.push("uninstall must delete leftover cu_probe.exe".into());
    }
    hits
}

pub fn bundle_tree_violations(root: &Path) -> Vec<String> {
    let mut hits = crate::runtime_prepare::package_deny_list_violations(root);
    fn walk(root: &Path, cur: &Path, hits: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(cur) else {
            return;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name == "cu_probe.exe"
                || name == "grok-computer-use-probe.exe"
                || name.ends_with(".pdb") && name.contains("cu_probe")
            {
                hits.push(format!(
                    "probe binary in bundle: {}",
                    p.strip_prefix(root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/")
                ));
            }
            if p.is_dir() {
                walk(root, &p, hits);
            }
        }
    }
    walk(root, root, &mut hits);
    hits.sort();
    hits.dedup();
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nsis_fork_keeps_2_11_5_security_and_drops_extra_bins() {
        let repo = repo_root();
        let ours = std::fs::read_to_string(repo.join("src-tauri/nsis/installer.nsi")).unwrap();
        let lock = std::fs::read_to_string(repo.join("src-tauri/Cargo.lock")).unwrap();
        let hits = nsis_drift_violations(&ours, &lock);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn missing_target_triple_fails_closed() {
        let script = repo_root().join("scripts/tauri-before-build.mjs");
        let out = std::process::Command::new("node")
            .arg(&script)
            .arg("--gate-only")
            .env_remove("TAURI_ENV_TARGET_TRIPLE")
            .env_remove("GROK_CU_BUNDLE_TARGET")
            .output()
            .expect("node");
        assert_ne!(out.status.code(), Some(0), "missing triple must fail");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("TAURI_ENV_TARGET_TRIPLE") || err.contains("GROK_CU_BUNDLE_TARGET"),
            "{err}"
        );
    }

    #[test]
    fn windows_msvc_triple_is_admitted() {
        let script = repo_root().join("scripts/tauri-before-build.mjs");
        let out = std::process::Command::new("node")
            .arg(&script)
            .arg("--gate-only")
            .env("TAURI_ENV_TARGET_TRIPLE", "x86_64-pc-windows-msvc")
            .env_remove("GROK_CU_BUNDLE_TARGET")
            .output()
            .expect("node");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("x86_64-pc-windows-msvc"), "{stdout}");
    }

    #[test]
    fn macos_triple_does_not_require_windows_seed() {
        let script = repo_root().join("scripts/tauri-before-build.mjs");
        let out = std::process::Command::new("node")
            .arg(&script)
            .arg("--gate-only")
            .env("TAURI_ENV_TARGET_TRIPLE", "aarch64-apple-darwin")
            .output()
            .expect("node");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn unsupported_windows_triple_fails_closed() {
        let script = repo_root().join("scripts/tauri-before-build.mjs");
        let out = std::process::Command::new("node")
            .arg(&script)
            .arg("--gate-only")
            .env("TAURI_ENV_TARGET_TRIPLE", "aarch64-pc-windows-msvc")
            .output()
            .expect("node");
        assert_ne!(out.status.code(), Some(0));
    }

    #[test]
    fn bundle_tree_refuses_cu_probe() {
        let dir = std::env::temp_dir().join(format!("cu-d9-deny-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/cu_probe.exe"), b"MZ").unwrap();
        let hits = bundle_tree_violations(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(hits.iter().any(|h| h.contains("cu_probe.exe")), "{hits:?}");
    }
}
