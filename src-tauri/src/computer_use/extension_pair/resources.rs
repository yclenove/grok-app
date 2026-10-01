//! Ownership for disposable probe profiles and their child processes.
use std::path::{Path, PathBuf};
use std::process::Child;

pub(super) struct ProbeResources {
    root: PathBuf,
    parent: PathBuf,
    pub profile: PathBuf,
    pub owner: String,
    #[cfg(windows)]
    job: Option<windows::Win32::Foundation::HANDLE>,
    cleaned: bool,
}

impl ProbeResources {
    pub fn create() -> Result<Self, String> {
        let parent = std::env::temp_dir()
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let owner = uuid::Uuid::new_v4().to_string();
        let root = parent.join(format!("grok-cu-pairing-owned-{owner}"));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        let profile = root.join("profile");
        std::fs::write(
            root.join("owner.json"),
            serde_json::to_vec(&serde_json::json!({
                "owner": owner, "purpose": "cu-pairing-probe", "profile": profile,
            }))
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::create_dir(&profile).map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            parent,
            profile,
            owner,
            #[cfg(windows)]
            job: None,
            cleaned: false,
        })
    }

    // The Node child waits for the private `ready` reply before launching Chrome.
    // Assigning a Job first closes the spawn-before-ownership race on Windows.
    pub fn attach(&mut self, child: &Child) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.job = Some(super::super::browser_supervisor::assign_probe_job(child)?);
        }
        #[cfg(not(windows))]
        {
            let _ = child;
        }
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<(), String> {
        if self.cleaned {
            return Ok(());
        }
        #[cfg(windows)]
        if let Some(job) = self.job {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::JobObjects::{
                JobObjectBasicAccountingInformation, QueryInformationJobObject, TerminateJobObject,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            };
            unsafe {
                TerminateJobObject(job, 1).map_err(|e| e.to_string())?;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
                unsafe {
                    QueryInformationJobObject(
                        Some(job),
                        JobObjectBasicAccountingInformation,
                        &mut info as *mut _ as *mut _,
                        std::mem::size_of_val(&info) as u32,
                        None,
                    )
                    .map_err(|e| e.to_string())?;
                }
                if info.ActiveProcesses == 0 {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Err("probe process tree remains alive".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
            unsafe {
                CloseHandle(job).map_err(|e| e.to_string())?;
            }
            self.job = None;
        }
        validate_owner(&self.root, &self.parent, &self.owner)?;
        // Windows can release browser file handles shortly after Job active count
        // reaches zero. Keep the owner marker intact through every bounded retry.
        for attempt in 0..=25 {
            validate_owner(&self.root, &self.parent, &self.owner)?;
            if !self.profile.try_exists().map_err(|e| e.to_string())? {
                break;
            }
            if self.profile.canonicalize().map_err(|e| e.to_string())? != self.profile {
                return Err("probe profile was redirected; preserving files".into());
            }
            match std::fs::remove_dir_all(&self.profile) {
                Ok(()) => break,
                Err(error) if attempt == 25 => return Err(error.to_string()),
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        std::fs::remove_file(self.root.join("owner.json")).map_err(|e| e.to_string())?;
        // Non-recursive: unexpected files in the owner root must be investigated.
        std::fs::remove_dir(&self.root).map_err(|e| e.to_string())?;
        self.cleaned = true;
        Ok(())
    }
}

fn validate_owner(root: &Path, parent: &Path, owner: &str) -> Result<(), String> {
    let resolved = root.canonicalize().map_err(|e| e.to_string())?;
    if resolved != root
        || resolved.parent() != Some(parent)
        || root.file_name().and_then(|s| s.to_str())
            != Some(format!("grok-cu-pairing-owned-{owner}").as_str())
    {
        return Err("probe cleanup target differs from the created owner directory".into());
    }
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("owner.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if marker["owner"] != owner
        || marker["purpose"] != "cu-pairing-probe"
        || Path::new(marker["profile"].as_str().unwrap_or_default()) != root.join("profile")
    {
        return Err("probe profile owner mismatch; preserving files".into());
    }
    Ok(())
}

impl Drop for ProbeResources {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            // Closing this exact Job is still safe if profile validation failed.
            #[cfg(windows)]
            if let Some(job) = self.job.take() {
                unsafe {
                    let _ = windows::Win32::Foundation::CloseHandle(job);
                }
            }
            eprintln!("probe cleanup incomplete; owner-marked profile retained");
        }
    }
}
