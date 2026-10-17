//! Build the macOS helper application bundle.
//!
//! The bundle is ad-hoc signed with an identifier-only designated requirement
//! so a recompile keeps the same TCC identity (ADR 0006). Release Developer ID
//! signing is a later phase. `codesign` runs only when a binary was copied and
//! the tool exists; otherwise the plist and icon are still written and the
//! command exits 0.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Dev bundle id. Matches `gcu_cli::helper::auth::DEV_BUNDLE_ID`.
pub const DEV_BUNDLE_ID: &str = "ai.grok.computer-use.helper.dev";
/// Release bundle id. Matches `gcu_cli::helper::auth::RELEASE_BUNDLE_ID`.
pub const RELEASE_BUNDLE_ID: &str = "ai.grok.computer-use.helper";
const DEV_APP_NAME: &str = "Grok Computer Use (Dev)";
const RELEASE_APP_NAME: &str = "Grok Computer Use";

/// Info.plist for the helper app.
///
/// `release` selects the stable bundle id. English usage strings name the
/// helper app, not the terminal. Blocking: none. Timeout: none. Cancellation: none.
#[must_use]
pub fn info_plist(release: bool) -> String {
    let id = if release {
        RELEASE_BUNDLE_ID
    } else {
        DEV_BUNDLE_ID
    };
    let name = if release {
        RELEASE_APP_NAME
    } else {
        DEV_APP_NAME
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>
	<string>en</string>
	<key>CFBundleExecutable</key>
	<string>gcu</string>
	<key>CFBundleIconFile</key>
	<string>icon.png</string>
	<key>CFBundleIdentifier</key>
	<string>{id}</string>
	<key>CFBundleInfoDictionaryVersion</key>
	<string>6.0</string>
	<key>CFBundleName</key>
	<string>{name}</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleShortVersionString</key>
	<string>0.0.0</string>
	<key>CFBundleVersion</key>
	<string>0.0.0</string>
	<key>LSMinimumSystemVersion</key>
	<string>14.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSAccessibilityUsageDescription</key>
	<string>Grok Computer Use helper reads and performs the actions you approve. macOS grants Accessibility to this helper app, not to the terminal that launched it.</string>
	<key>NSScreenCaptureUsageDescription</key>
	<string>Grok Computer Use helper captures the windows you approve. macOS grants Screen Recording to this helper app, not to the terminal that launched it.</string>
</dict>
</plist>
"#
    )
}

/// `codesign` argv for an identifier-only ad-hoc signature.
///
/// `--force` is included so a rebuilt app can be signed again. The requirement
/// string is `designated => identifier "<id>"`. Returns an error when `bundle_id`
/// could escape that string. Blocking: none. Timeout: none. Cancellation: none.
pub fn codesign_argv(bundle_id: &str, app: &Path) -> Result<Vec<String>> {
    if !valid_identifier(bundle_id) {
        bail!("bundle id is not safe for a designated requirement");
    }
    Ok(vec![
        "codesign".to_string(),
        "-s".to_string(),
        "-".to_string(),
        "-i".to_string(),
        bundle_id.to_string(),
        // `-r` takes a *file path* unless the value is attached with `=`; a separate
        // argument made macOS read the requirement text as a missing file.
        format!("-r=designated => identifier \"{bundle_id}\""),
        "--force".to_string(),
        app.display().to_string(),
    ])
}

fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && value.len() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// `cargo xtask bundle-macos [--release] [--binary PATH] [--output DIR]`.
///
/// Writes a `.app` under `computer-use/target/gcu-helper` by default. A missing
/// default binary still writes the plist and icon. Blocking: filesystem copy
/// and, when present, one `codesign` invocation. Timeout: none beyond the
/// `codesign` process itself. Cancellation: none.
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut release = false;
    let mut binary: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--release" => release = true,
            "--binary" => {
                binary = Some(PathBuf::from(args.next().context("missing --binary path")?))
            }
            "--output" => {
                output = Some(PathBuf::from(args.next().context("missing --output path")?))
            }
            other => bail!("unknown bundle-macos argument: {other}"),
        }
    }
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let name = if release {
        RELEASE_APP_NAME
    } else {
        DEV_APP_NAME
    };
    let bundle_id = if release {
        RELEASE_BUNDLE_ID
    } else {
        DEV_BUNDLE_ID
    };
    let root = output.unwrap_or_else(|| workspace.join("target/gcu-helper"));
    let app = root.join(format!("{name}.app"));
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    fs::write(contents.join("Info.plist"), info_plist(release))?;
    let icon = workspace.join("apps/gcu-helper-macos/assets/icon.png");
    fs::copy(&icon, resources.join("icon.png"))
        .with_context(|| format!("missing helper icon at {}", icon.display()))?;
    let default_binary = workspace.join(if release {
        "target/release/gcu"
    } else {
        "target/debug/gcu"
    });
    let binary = binary.unwrap_or(default_binary);
    let copied = if binary.is_file() {
        let dest = macos.join("gcu");
        fs::copy(&binary, &dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&dest)?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&dest, permissions)?;
        }
        true
    } else {
        eprintln!("helper binary not found; wrote plist and icon only");
        false
    };
    if copied {
        sign(&app, bundle_id)?;
    }
    println!("wrote {}", app.display());
    Ok(())
}

fn sign(app: &Path, bundle_id: &str) -> Result<()> {
    let argv = codesign_argv(bundle_id, app)?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    match command.output() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("codesign not available; would run: {}", argv.join(" "));
            Ok(())
        }
        Err(error) => bail!("cannot run codesign: {error}"),
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => bail!(
            "codesign failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn codesign_requirement_is_identifier_only() {
        let argv = codesign_argv(DEV_BUNDLE_ID, Path::new("/tmp/Helper.app")).unwrap();
        assert!(argv.windows(2).any(|pair| pair == ["-s", "-"]));
        assert!(argv.windows(2).any(|pair| pair == ["-i", DEV_BUNDLE_ID]));
        // Real macOS: a detached `-r <text>` is parsed as a file name and fails with
        // "No such file or directory", so the requirement must be one `-r=<text>` argument.
        assert!(
            argv.contains(&format!("-r=designated => identifier \"{DEV_BUNDLE_ID}\"")),
            "{argv:?}"
        );
        assert!(!argv.iter().any(|arg| arg == "-r"), "{argv:?}");
        assert!(argv.iter().any(|arg| arg == "--force"));
    }

    #[test]
    fn codesign_rejects_quotes_in_identifier() {
        assert!(codesign_argv("bad\"id", Path::new("a")).is_err());
        assert!(codesign_argv("has space", Path::new("a")).is_err());
    }

    #[test]
    fn dev_plist_matches_checked_in_template() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../apps/gcu-helper-macos/Info.plist");
        // A Windows checkout may convert the template to CRLF; the bundle is always written with LF.
        let file = fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        assert_eq!(file, info_plist(false));
    }

    #[test]
    fn release_plist_names_release_bundle() {
        let plist = info_plist(true);
        assert!(plist.contains(RELEASE_BUNDLE_ID));
        assert!(!plist.contains("helper.dev"));
        assert!(plist.contains("<key>LSUIElement</key>\n\t<true/>"));
        assert!(plist.contains("not to the terminal"));
    }
}
