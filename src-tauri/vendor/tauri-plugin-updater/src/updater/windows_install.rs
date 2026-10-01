//! Grok patch: verified, retained Windows staging -> fallible cleanup -> checked
//! installer launch -> ordinary Tauri exit cleanup -> process exit.
use super::windows_journal::{Journal, Phase};
use super::*;
use crate::install_recovery::{CandidateKey, InstallAttempt, InstallRecovery, PendingInstall};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
mod detached_dispatch;
mod detached_witness;
pub use detached_dispatch::{windows_update_dispatch_entrypoint, WindowsDispatchTrust};
pub(in crate::updater) mod completion;
mod durable;
mod failure;
pub(super) mod inventory;
mod launch_plan;
mod process_witness;
mod verified_artifact;
pub(crate) use durable::WindowsRecovery;

/// Reserved, read-only updater worker bootstrap. Call before starting Tauri.
pub fn windows_update_witness_entrypoint() -> Option<i32> {
    detached_witness::entrypoint()
}

pub(super) struct PreparedWindowsInstaller {
    // Exclude WRITE/DELETE sharing for the exact verified file until exit.
    _read_lease: File,
    // Keep the extracted temp file alive together with its verified read lease.
    _installer: WindowsUpdaterType,
    launch: launch_plan::LaunchPlan,
}

// Retain the original prepared transaction even if an IPC resource is closed.
// A different Update cannot replace a partially drained App's candidate.
struct RetainedInstall {
    update: Update,
    identity: [u8; 32],
    journal: Option<Arc<Journal>>,
}

#[cfg(test)]
static OWNER: InstallRecovery<RetainedInstall> = InstallRecovery::new();

impl Update {
    pub(super) fn install_windows_checked(&self, bytes: &[u8]) -> Result<()> {
        verify_signature(bytes, &self.signature, &self.config.pubkey)?;
        if let Some(owner) = &self.windows_recovery {
            let journal = owner.initialize(self)?;
            return self.install_windows_using(bytes, &owner.catalog, Some(journal));
        }
        #[cfg(test)]
        return self.install_windows_with_owner(bytes, &OWNER);
        #[cfg(not(test))]
        Err(super::windows_journal::pending(
            "Windows durable update owner is not configured",
        ))
    }

    #[cfg(test)]
    fn install_windows_with_owner(
        &self,
        bytes: &[u8],
        owner: &InstallRecovery<RetainedInstall>,
    ) -> Result<()> {
        self.install_windows_using(bytes, owner, None)
    }

    fn install_windows_using(
        &self,
        bytes: &[u8],
        owner: &InstallRecovery<RetainedInstall>,
        journal: Option<Arc<Journal>>,
    ) -> Result<()> {
        // Bind the bytes to THIS checked Update, not a different download rid.
        verify_signature(bytes, &self.signature, &self.config.pubkey)?;
        let identity: [u8; 32] = Sha256::digest(bytes).into();
        let mut retained_update = self.clone();
        // The catalog owns the Update, not an Arc cycle back to itself.
        retained_update.windows_recovery = None;
        let attempt = owner.begin(
            CandidateKey {
                transaction: Arc::as_ptr(&self.windows_install) as usize,
                digest: identity,
            },
            self.version.clone(),
            RetainedInstall {
                update: retained_update,
                identity,
                journal,
            },
        )?;
        Self::run_windows_attempt(attempt, Some(bytes))
    }

    fn run_windows_attempt(
        attempt: InstallAttempt<'_, RetainedInstall>,
        bytes: Option<&[u8]>,
    ) -> Result<()> {
        // Retain the original Update/config/guard, not just a downloaded handle.
        // Recovery never fetches a manifest, substitutes a version or restages.
        let retained = attempt.value.clone();
        let update = &retained.update;
        let mut staging_failed = false;
        let result = update.windows_install.run(
            retained.identity,
            || {
                let result = match bytes {
                    Some(bytes) => match &retained.journal {
                        Some(journal) => update.stage_durable(bytes, journal, attempt.id()),
                        None => update.stage_windows_installer(bytes),
                    },
                    None => Err(Error::WindowsInstallPending {
                        phase: "task",
                        message: "Original prepared installer is missing; recovery cannot restage"
                            .into(),
                    }),
                };
                staging_failed = bytes.is_some() && result.is_err();
                result
            },
            || {
                attempt.phase("cleanup")?;
                update
                    .before_windows_install
                    .as_ref()
                    .ok_or_else(|| "App cleanup guard is not configured".to_string())?(
                )
            },
            |prepared| {
                attempt.phase("launch")?;
                if let Some(journal) = retained.journal.as_deref() {
                    return detached_dispatch::dispatch(journal, attempt.id());
                }
                #[cfg(test)]
                {
                    durable::dispatch(None, attempt.id(), || prepared.launch.launch())
                }
                #[cfg(not(test))]
                {
                    let _ = &prepared.launch;
                    Err("Durable installer dispatcher is not configured".into())
                }
            },
            || {
                attempt.phase("exit")?;
                if let Some(on_before_exit) = update.on_before_exit.as_ref() {
                    on_before_exit();
                }
                std::process::exit(0);
            },
        );
        let prepared = update.windows_install.has_prepared();
        let result = match &retained.journal {
            Some(journal) => match journal.snapshot() {
                Err(error) => Err(error),
                Ok(Some(record))
                    if matches!(record.phase, Phase::LaunchIntent | Phase::Accepted) =>
                {
                    Err(Error::WindowsInstallPending {
                        phase: "launch_unknown",
                        message: "Durable launch intent has no verified outcome; replay is blocked"
                            .into(),
                    })
                }
                Ok(None) if staging_failed && !prepared => {
                    // Only this owned initial attempt can prove staging failed
                    // before publication/cleanup/dispatch. An absent record on
                    // later recovery is NOT permission to release a candidate.
                    // snapshot() rejects poisoned/uncertain journal writes;
                    // has_prepared() also fails closed on a poisoned lock.
                    result.map_err(|error| match error {
                        error @ Error::WindowsInstallPending { .. } => {
                            Error::WindowsInstallPreflight(error.to_string())
                        }
                        error => error,
                    })
                }
                _ => result,
            },
            None => result,
        };
        attempt.finish(result, prepared)
    }

    fn stage_windows_installer(&self, bytes: &[u8]) -> Result<PreparedWindowsInstaller> {
        let installer = self.extract(bytes)?;
        let path = match &installer {
            WindowsUpdaterType::Nsis { path, .. } | WindowsUpdaterType::Msi { path, .. } => path,
        };
        let mut file = File::options().read(true).share_mode(1).open(path)?; // FILE_SHARE_READ
        let mut actual = Vec::new();
        file.read_to_end(&mut actual)?;
        file.seek(SeekFrom::Start(0))?;
        #[cfg(feature = "zip")]
        let expected = if infer::archive::is_zip(bytes) {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(Error::InvalidUpdaterFormat)?;
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
            let mut expected = Vec::new();
            archive.by_name(name)?.read_to_end(&mut expected)?;
            expected
        } else {
            bytes.to_vec()
        };
        #[cfg(not(feature = "zip"))]
        let expected = bytes.to_vec();
        if actual != expected {
            return Err(Error::InvalidUpdaterFormat);
        }
        let launch = launch_plan::LaunchPlan::new(
            &installer,
            self.config.install_mode(),
            &self.current_exe_args,
            &self.installer_args,
        )
        .map_err(Error::WindowsInstallPreflight)?;
        Ok(PreparedWindowsInstaller {
            _installer: installer,
            launch,
            _read_lease: file,
        })
    }
}

pub(super) fn windows_system_directory() -> std::result::Result<OsString, String> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32768];
    let size = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } as usize;
    if size == 0 || size >= buffer.len() {
        return Err("Windows system directory unavailable".into());
    }
    Ok(OsString::from_wide(&buffer[..size]))
}

#[cfg(test)]
fn launch_checked(file: &Path, arguments: &OsStr) -> std::result::Result<(), String> {
    // Dropping our process handle does not stop the external installer.
    launch_process(file, arguments).map(drop)
}

fn launch_process(
    file: &Path,
    arguments: &OsStr,
) -> std::result::Result<Option<OwnedHandle>, String> {
    use windows_sys::Win32::UI::{
        Shell::{
            ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS,
            SHELLEXECUTEINFOW,
        },
        WindowsAndMessaging::SW_SHOW,
    };
    launch_plan::validate_command(file, arguments)?;
    let file = encode_wide(file);
    let arguments = encode_wide(arguments);
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of_val(&info) as u32;
    // The caller has no Windows message loop and may exit after custody ACK.
    // Finish the Shell handoff before returning; this does not wait for install.
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC;
    info.lpVerb = windows_sys::w!("open");
    info.lpFile = file.as_ptr();
    info.lpParameters = arguments.as_ptr();
    info.nShow = SW_SHOW;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(format!(
            "Windows installer launch failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Launch acceptance is not successful installation. The external installer
    // owns completion/rollback; this parent only exits after a successful launch.
    Ok(if info.hProcess.is_null() {
        None
    } else {
        Some(unsafe { OwnedHandle::from_raw_handle(info.hProcess) })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(super) fn refusing_update(fixture: &serde_json::Value, count: Arc<AtomicUsize>) -> Update {
        let guard_count = count;
        Update {
            windows_recovery: None,
            windows_install: Default::default(),
            run_on_main_thread: Arc::new(Box::new(|_| panic!("must not dispatch UI"))),
            config: Config {
                pubkey: fixture["publicKey"].as_str().unwrap().into(),
                ..Default::default()
            },
            on_before_exit: Some(Arc::new(|| panic!("must not exit"))),
            before_windows_install: Some(Arc::new(move || {
                guard_count.fetch_add(1, Ordering::SeqCst);
                Err("test never grants installer launch".into())
            })),
            body: None,
            current_version: "0.0.0".into(),
            version: "0.0.1-owned-test".into(),
            date: None,
            target: "windows-x86_64".into(),
            download_url: "https://example.invalid/not-used".parse().unwrap(),
            signature: fixture["signature"].as_str().unwrap().into(),
            raw_json: serde_json::Value::Null,
            timeout: None,
            proxy: None,
            no_proxy: true,
            headers: HeaderMap::new(),
            extract_path: PathBuf::new(),
            app_name: "owned-updater-recovery-test".into(),
            installer_args: Vec::new(),
            current_exe_args: Vec::new(),
            configure_client: None,
        }
    }

    #[test]
    fn resource_table_close_keeps_production_staging_and_original_cleanup_recoverable() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/signed-inert-stage.json"))
                .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let update = refusing_update(&fixture, count.clone());
        let weak = Arc::downgrade(&update.windows_install);
        let mut resources = tauri::ResourceTable::default();
        let rid = resources.add(update);
        let resource = resources.get::<Update>(rid).unwrap();
        let owner = InstallRecovery::new();
        let bytes = fixture["payload"].as_str().unwrap().as_bytes();
        assert!(matches!(
            resource.install_windows_with_owner(bytes, &owner),
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        let path = resource
            .windows_install
            .inspect_prepared(|prepared| match &prepared._installer {
                WindowsUpdaterType::Nsis { path, .. } | WindowsUpdaterType::Msi { path, .. } => {
                    path.clone()
                }
            })
            .unwrap();
        let id = owner.snapshot().unwrap().unwrap().candidate_id;
        resources.close(rid).unwrap();
        drop(resource);
        assert!(resources.get::<Update>(rid).is_err());
        assert!(weak.upgrade().is_some());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(File::options().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert!(matches!(
            Update::run_windows_attempt(owner.resume(&id).unwrap(), None),
            Err(Error::WindowsInstallPending {
                phase: "cleanup",
                ..
            })
        ));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(owner.snapshot().unwrap().unwrap().candidate_id, id);
        let replacement = refusing_update(&fixture, count.clone());
        assert!(matches!(
            replacement.install_windows_with_owner(bytes, &owner),
            Err(Error::WindowsInstallBusy)
        ));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        drop(owner);
        assert!(weak.upgrade().is_none());
        assert!(!path.exists()); // TempPath drops only after the protected lease.
        std::fs::remove_dir(path.parent().unwrap()).unwrap(); // only this owned stage
    }

    #[test]
    fn signature_and_format_are_checked_before_app_cleanup_or_exit() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/signed-format-rejection.json"
        ))
        .unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let guard_count = count.clone();
        let update = Update {
            windows_recovery: None,
            windows_install: Default::default(),
            run_on_main_thread: Arc::new(Box::new(|_| panic!("must not dispatch UI"))),
            config: Config {
                pubkey: fixture["publicKey"].as_str().unwrap().into(),
                ..Default::default()
            },
            on_before_exit: Some(Arc::new(|| panic!("must not exit"))),
            before_windows_install: Some(Arc::new(move || {
                guard_count.fetch_add(1, Ordering::SeqCst);
                Err("test never grants installer launch".into())
            })),
            body: None,
            current_version: "0.0.0".into(),
            version: "0.0.1-owned-test".into(),
            date: None,
            target: "windows-x86_64".into(),
            download_url: "https://example.invalid/not-used".parse().unwrap(),
            signature: fixture["signature"].as_str().unwrap().into(),
            raw_json: serde_json::Value::Null,
            timeout: None,
            proxy: None,
            no_proxy: true,
            headers: HeaderMap::new(),
            extract_path: PathBuf::new(),
            app_name: "owned-updater-format-test".into(),
            installer_args: Vec::new(),
            current_exe_args: Vec::new(),
            configure_client: None,
        };
        let bytes = fixture["payload"].as_str().unwrap().as_bytes();
        assert!(verify_signature(bytes, &update.signature, &update.config.pubkey).unwrap());
        let mut tampered = bytes.to_vec();
        tampered[0] ^= 1;
        assert!(matches!(update.install(tampered), Err(Error::Minisign(_))));
        assert!(matches!(
            update.install(bytes),
            Err(Error::InvalidUpdaterFormat)
        ));
        let mut wrong_update = update.clone();
        wrong_update.signature =
            base64::engine::general_purpose::STANDARD.encode("not the checked release signature");
        assert!(wrong_update.install(bytes).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        assert!(!update.windows_install.has_prepared());
    }

    #[test]
    #[ignore = "owned child fixture; executed and checked by real_shell_launch_accepts_owned_child"]
    fn owned_launch_child() {
        use std::io::Write;
        let exe = std::env::current_exe().unwrap();
        assert_eq!(exe.file_name().unwrap(), "owned-updater-launch-fixture.exe");
        let root = exe.parent().unwrap();
        assert_eq!(
            std::fs::read(root.join("fixture-purpose")).unwrap(),
            b"updater owned launch test only"
        );
        let mut marker = File::options()
            .write(true)
            .create_new(true)
            .open(root.join("child-accepted"))
            .unwrap();
        write!(marker, "{}", std::process::id()).unwrap();
    }

    #[test]
    fn real_shell_launch_accepts_owned_child() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{
                GetExitCodeProcess, GetProcessId, TerminateProcess, WaitForSingleObject,
            },
        };
        // Only this private copy of the test harness can run. No real installer,
        // registry update, installed App, user's clipboard or existing UI is used.
        let root = tempfile::tempdir().unwrap();
        let exe = root.path().join("owned-updater-launch-fixture.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        // Exercise real loader acceptance while the same no-write/no-delete
        // lease used by durable staging remains held, not just an unleased exe.
        let lease = super::windows_journal::protected_read(&exe).unwrap();
        std::fs::write(
            root.path().join("fixture-purpose"),
            b"updater owned launch test only",
        )
        .unwrap();
        let mut nul_file = exe.as_os_str().to_owned();
        nul_file.push("\0not-the-executable");
        assert!(launch_process(Path::new(&nul_file), OsStr::new("")).is_err());
        assert!(launch_process(&exe, OsStr::new("--ignored\0hidden")).is_err());
        assert!(!root.path().join("child-accepted").exists());
        let child = launch_process(&exe, OsStr::new("--exact updater::windows_install::tests::owned_launch_child --ignored --test-threads=1")).unwrap().expect("a new owned executable must yield a process handle");
        let handle = child.as_raw_handle();
        let pid = unsafe { GetProcessId(handle) };
        let identity = durable::process_identity(&child).unwrap();
        assert_eq!(identity.pid, pid);
        assert!(identity.created > 0);
        let wait = unsafe { WaitForSingleObject(handle, 10_000) };
        if wait != WAIT_OBJECT_0 {
            // Reap only the exact process handle we just created; never search by
            // process name or terminate the installed product.
            unsafe {
                TerminateProcess(handle, 71);
                WaitForSingleObject(handle, 10_000);
            }
            panic!("owned child failed to terminate: wait={wait}");
        }
        let mut exit = u32::MAX;
        assert_ne!(unsafe { GetExitCodeProcess(handle, &mut exit) }, 0);
        assert_eq!(exit, 0);
        assert_eq!(durable::process_identity(&child).unwrap(), identity);
        assert_eq!(
            std::fs::read_to_string(root.path().join("child-accepted")).unwrap(),
            pid.to_string()
        );
        drop(child);
        drop(lease);
    }
    #[test]
    fn missing_installer_is_a_real_launch_error_not_exit_permission() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            launch_checked(&root.path().join("missing-installer.exe"), OsStr::new("")).is_err()
        );
    }
    #[test]
    fn protected_staged_file_cannot_be_replaced_or_written() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("owned-stage.bin");
        std::fs::write(&path, b"owned bytes").unwrap();
        let lease = File::options()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(File::options().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        drop(lease);
        std::fs::remove_file(path).unwrap();
    }
}
