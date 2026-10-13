//! Running apps, bundle facts, and non-activating launch.

use crate::{
    api::{BundleFacts, RunningApp},
    ffi::{self, RawSigning},
    identity::SignatureInfo,
};
use block2::RcBlock;
use gcu_core::driver::{DriverError, Result};
use objc2::msg_send;
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString, NSURL};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};

const MAX_HASH_BYTES: u64 = 512 * 1024 * 1024;

pub(super) fn running_apps() -> Result<Vec<RunningApp>> {
    let mut out = Vec::new();
    for app in running_applications() {
        if let Some(row) = running_from(&app) {
            out.push(row);
        }
    }
    Ok(out)
}

pub(super) fn frontmost_pid() -> Result<Option<u32>> {
    let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
        return Ok(None);
    };
    Ok(pid_of(&app))
}

pub(super) fn unhide(pid: u32) -> Result<()> {
    let app = find_pid(pid).ok_or(DriverError::AppNotFound)?;
    let _ = app.unhide();
    Ok(())
}

pub(super) fn launch(path: &str) -> Result<()> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let config = NSWorkspaceOpenConfiguration::configuration();
    config.setActivates(false);
    config.setPromptsUserIfNeeded(false);
    let (tx, rx) = mpsc::sync_channel(1);
    let block: RcBlock<dyn Fn(*mut NSRunningApplication, *mut NSError)> =
        RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
            let _ = tx.send(!app.is_null() && error.is_null());
        });
    // `&*block` is the `DynBlock` the method stores. AppKit copies it before
    // returning, so the handler can run after this function drops its copy.
    NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
        &url,
        &config,
        Some(&*block),
    );
    match rx.recv_timeout(Duration::from_secs(4)) {
        Ok(true) => Ok(()),
        Ok(false) => Err(DriverError::Native {
            message: "launch failed".into(),
            maybe_executed: true,
        }),
        Err(_) => Err(DriverError::Timeout {
            maybe_executed: true,
        }),
    }
}

pub(super) fn bundle_facts(path: &str) -> Result<BundleFacts> {
    let bundle_path = PathBuf::from(path);
    if !bundle_path.exists() {
        return Err(DriverError::AppNotFound);
    }
    let (bundle_id, localized_name, bundle_name, executable, product, build) =
        read_plist(&bundle_path);
    let executable_path = executable.unwrap_or_else(|| bundle_path.clone());
    if !executable_path.is_file() {
        return Err(DriverError::AppNotFound);
    }
    let sha256 = hash_file(&executable_path)?;
    let signing = ffi::read_signing(if bundle_path.is_dir() {
        bundle_path.as_path()
    } else {
        executable_path.as_path()
    })?;
    let cdhash = signing.cdhash.clone();
    Ok(BundleFacts {
        bundle_id,
        localized_name,
        bundle_name,
        bundle_path: bundle_path.to_string_lossy().into_owned(),
        executable_path: executable_path.to_string_lossy().into_owned(),
        signature: signature_from(signing, product, build),
        cdhash,
        sha256,
        writable_load_path: writable_load(&bundle_path, &executable_path),
    })
}

fn signature_from(
    signing: RawSigning,
    product: Option<String>,
    build: Option<String>,
) -> SignatureInfo {
    SignatureInfo {
        state: signing.state,
        team_id: signing.team_id,
        signing_id: signing.signing_id,
        subject: signing.subject,
        certificate_sha256: signing.certificate_sha256,
        product,
        build,
    }
}

fn running_from(app: &NSRunningApplication) -> Option<RunningApp> {
    let pid = pid_of(app)?;
    let bundle_path = app
        .bundleURL()
        .and_then(|url| url_path(&url))
        .unwrap_or_default();
    let executable_path = app
        .executableURL()
        .and_then(|url| url_path(&url))
        .unwrap_or_default();
    if bundle_path.is_empty() && executable_path.is_empty() {
        return None;
    }
    Some(RunningApp {
        pid,
        bundle_id: app.bundleIdentifier().map(|value| value.to_string()),
        localized_name: app
            .localizedName()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        bundle_path,
        executable_path,
        hidden: app.isHidden(),
    })
}

fn find_pid(pid: u32) -> Option<Retained<NSRunningApplication>> {
    running_applications()
        .into_iter()
        .find(|app| pid_of(app) == Some(pid))
}

/// `NSWorkspace.runningApplications` is missing from objc2-app-kit 0.3.2.
/// The property is `copy` but the selector is not in the copy family, so the
/// return is autoreleased and must be retained inside a pool.
fn running_applications() -> Vec<Retained<NSRunningApplication>> {
    let workspace = NSWorkspace::sharedWorkspace();
    autoreleasepool(|_| {
        // SAFETY: `runningApplications` has existed on NSWorkspace since macOS
        // 10.6 and returns a non-null copied array of NSRunningApplication.
        // NSWorkspace does not require the main thread for this read. Retained
        // keeps the array alive after the pool drains.
        let apps: Retained<NSArray<NSRunningApplication>> =
            unsafe { msg_send![&workspace, runningApplications] };
        apps.to_vec()
    })
}

fn pid_of(app: &NSRunningApplication) -> Option<u32> {
    let pid = app.processIdentifier();
    if pid <= 0 {
        None
    } else {
        Some(pid as u32)
    }
}

fn url_path(url: &NSURL) -> Option<String> {
    let path = url.path()?;
    let text = path.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn read_plist(
    path: &Path,
) -> (
    Option<String>,
    String,
    String,
    Option<PathBuf>,
    Option<String>,
    Option<String>,
) {
    let ns_path = NSString::from_str(&path.to_string_lossy());
    let Some(bundle) = NSBundle::bundleWithPath(&ns_path) else {
        let name = file_stem(path);
        return (None, name.clone(), name, None, None, None);
    };
    let bundle_id = info_string(&bundle, "CFBundleIdentifier");
    let bundle_name = info_string(&bundle, "CFBundleName").unwrap_or_else(|| file_stem(path));
    let localized =
        info_string(&bundle, "CFBundleDisplayName").unwrap_or_else(|| bundle_name.clone());
    let executable = bundle
        .executableURL()
        .and_then(|url| url_path(&url))
        .map(PathBuf::from);
    let product = info_string(&bundle, "CFBundleName");
    let build = info_string(&bundle, "CFBundleVersion");
    (
        bundle_id,
        localized,
        bundle_name,
        executable,
        product,
        build,
    )
}

fn info_string(bundle: &NSBundle, key: &str) -> Option<String> {
    let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key))?;
    let text = value.downcast::<NSString>().ok()?.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|_| DriverError::AppNotFound)?;
    let len = file.metadata().map_err(|_| DriverError::AppNotFound)?.len();
    if len > MAX_HASH_BYTES {
        return Err(DriverError::InvalidRequest);
    }
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(|_| DriverError::Native {
            message: "executable hash failed".into(),
            maybe_executed: false,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn writable_load(bundle: &Path, executable: &Path) -> bool {
    if bundle.is_dir()
        && (is_writable(&bundle.join("Contents/PlugIns"))
            || is_writable(&bundle.join("Contents/Frameworks")))
    {
        return true;
    }
    executable.parent().is_some_and(is_writable)
}

fn is_writable(path: &Path) -> bool {
    let Some(text) = path.as_os_str().to_str() else {
        return false;
    };
    let Ok(c_path) = CString::new(text) else {
        return false;
    };
    // SAFETY: `c_path` is a NUL-terminated path we own. `access` only reads the
    // mode bit and does not follow the caller into a write.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK) == 0 }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("app")
        .to_owned()
}
