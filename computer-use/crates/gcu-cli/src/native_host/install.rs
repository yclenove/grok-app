//! Idempotent Chrome/Edge user registration with read-only planning and injected IO.
//! Filesystem operations are blocking; CLI runs them on a five-second worker.
use super::{Error, ExtensionIdentity};
use serde_json::{json, Value};
use std::{
    fs, io,
    io::Write,
    path::{Path, PathBuf},
};

/// Host name used by Chrome, Edge, extension and registry entries.
pub const HOST_NAME: &str = "ai.grok.computer_use";
const DESCRIPTION: &str = "Grok Computer Use native messaging host";

/// Supported browser's per-user registration root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Browser {
    /// Google Chrome.
    Chrome,
    /// Microsoft Edge.
    Edge,
}
impl Browser {
    /// HKCU-only key. There is no machine-wide registration path.
    pub fn registry_key(self) -> String {
        let browser = match self {
            Self::Chrome => "Google\\Chrome",
            Self::Edge => "Microsoft\\Edge",
        };
        format!("Software\\{browser}\\NativeMessagingHosts\\{HOST_NAME}")
    }
}

/// Target layout; platform-independent planning lets fake tests exercise all three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// `~/.config/{google-chrome,microsoft-edge}/NativeMessagingHosts`.
    Linux,
    /// `~/Library/Application Support/{Google/Chrome,Microsoft Edge}/NativeMessagingHosts`.
    Macos,
    /// Manifests under the injected home, with HKCU default values pointing at them.
    Windows,
}
impl Platform {
    /// Compile target's browser layout; no runtime OS/environment probing.
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Linux
        }
    }
    fn manifest(self, home: &Path, browser: Browser) -> PathBuf {
        let root = match (self, browser) {
            (Self::Linux, Browser::Chrome) => ".config/google-chrome/NativeMessagingHosts",
            (Self::Linux, Browser::Edge) => ".config/microsoft-edge/NativeMessagingHosts",
            (Self::Macos, Browser::Chrome) => {
                "Library/Application Support/Google/Chrome/NativeMessagingHosts"
            }
            (Self::Macos, Browser::Edge) => {
                "Library/Application Support/Microsoft Edge/NativeMessagingHosts"
            }
            (Self::Windows, Browser::Chrome) => {
                "AppData/Local/grok-computer-use/NativeMessagingHosts/chrome"
            }
            (Self::Windows, Browser::Edge) => {
                "AppData/Local/grok-computer-use/NativeMessagingHosts/edge"
            }
        };
        home.join(root).join(format!("{HOST_NAME}.json"))
    }
    fn template(self) -> &'static str {
        match self {
            Self::Linux => include_str!("../../../../tools/browser-extension/manifests/linux.json"),
            Self::Macos => include_str!("../../../../tools/browser-extension/manifests/macos.json"),
            Self::Windows => {
                include_str!("../../../../tools/browser-extension/manifests/windows.json")
            }
        }
    }
}

/// Blocking per-user registry access. Tests inject a fake; real implementation is Windows-only.
/// No method may change other values/subkeys. An absent default is `None`.
pub trait Registry {
    /// Read the host's default value without creating a key.
    fn get(&self, browser: Browser) -> io::Result<Option<String>>;
    /// Set/delete only the default value. Delete the key only when it is empty.
    fn set(&mut self, browser: Browser, path: Option<&str>) -> io::Result<()>;
}

/// Registry backend for the compile target. Non-Windows calls reject registry use.
pub struct SystemRegistry;
#[cfg(not(windows))]
impl Registry for SystemRegistry {
    fn get(&self, _: Browser) -> io::Result<Option<String>> {
        Err(io::Error::other("registry unavailable"))
    }
    fn set(&mut self, _: Browser, _: Option<&str>) -> io::Result<()> {
        Err(io::Error::other("registry unavailable"))
    }
}
#[cfg(windows)]
impl Registry for SystemRegistry {
    fn get(&self, browser: Browser) -> io::Result<Option<String>> {
        super::unsafe_registry::get(browser)
    }
    fn set(&mut self, browser: Browser, path: Option<&str>) -> io::Result<()> {
        super::unsafe_registry::set(browser, path)
    }
}

struct FileEdit {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}
struct RegistryEdit {
    browser: Browser,
    before: Option<String>,
    after: Option<String>,
}

/// Concrete plan: read-only construction, no environment access, no implicit home.
pub struct InstallPlan {
    files: Vec<FileEdit>,
    registry: Vec<RegistryEdit>,
    home: PathBuf,
}
impl InstallPlan {
    /// Plan install/removal. Missing identity prevents install, but never prevents removal.
    /// Blocking reads only; callers choose worker/deadline. Project installs are unsupported.
    pub fn new(
        platform: Platform,
        home: &Path,
        gcu: &Path,
        identity: Option<&ExtensionIdentity>,
        remove: bool,
        registry: &dyn Registry,
    ) -> Result<Self, Error> {
        if !home.is_absolute()
            || !gcu.is_absolute()
            || gcu.to_str().is_none()
            || gcu.as_os_str().as_encoded_bytes().contains(&0)
        {
            return Err(Error::Registration);
        }
        if !remove && identity.is_none() {
            return Err(Error::Identity);
        }
        let mut plan = Self {
            files: Vec::new(),
            registry: Vec::new(),
            home: home.to_owned(),
        };
        for browser in [Browser::Chrome, Browser::Edge] {
            let path = platform.manifest(home, browser);
            check_ancestors(home, &path)?;
            let before = read_file(&path)?;
            if let Some(bytes) = &before {
                let value: Value =
                    serde_json::from_slice(bytes).map_err(|_| Error::Registration)?;
                if value["name"] != HOST_NAME
                    || value["description"] != DESCRIPTION
                    || value["type"] != "stdio"
                    || value["path"] != gcu.to_str().ok_or(Error::Registration)?
                    || !owned_origin(&value)
                {
                    return Err(Error::Registration);
                }
            }
            let after = if remove {
                None
            } else {
                let mut value: Value =
                    serde_json::from_str(platform.template()).map_err(|_| Error::Registration)?;
                value["path"] = json!(gcu.to_str().ok_or(Error::Registration)?);
                value["allowed_origins"] = json!([identity.ok_or(Error::Identity)?.origin()]);
                let mut bytes =
                    serde_json::to_vec_pretty(&value).map_err(|_| Error::Registration)?;
                bytes.push(b'\n');
                Some(bytes)
            };
            if platform == Platform::Windows {
                let old = registry.get(browser)?;
                let target = path.to_str().ok_or(Error::Registration)?.to_owned();
                if old.as_deref().is_some_and(|old| old != target) {
                    return Err(Error::Registration);
                }
                plan.registry.push(RegistryEdit {
                    browser,
                    before: old,
                    after: if remove { None } else { Some(target) },
                });
            }
            plan.files.push(FileEdit {
                path,
                before,
                after,
            });
        }
        Ok(plan)
    }

    /// Whether apply would change any file or registry value.
    pub fn is_noop(&self) -> bool {
        self.files.iter().all(|e| e.before == e.after)
            && self.registry.iter().all(|e| e.before == e.after)
    }

    /// Both per-browser manifest locations, useful for preview and temporary-home tests.
    pub fn manifests(&self) -> Vec<PathBuf> {
        self.files.iter().map(|e| e.path.clone()).collect()
    }

    /// Preview exact file changes and registry defaults; never writes or creates directories.
    pub fn render_diff(&self) -> String {
        let mut out = String::new();
        for edit in &self.files {
            if edit.before == edit.after {
                continue;
            }
            out.push_str(&format!(
                "--- {}\n+++ {}\n",
                edit.path.display(),
                edit.path.display()
            ));
            for (prefix, bytes) in [("-", &edit.before), ("+", &edit.after)] {
                if let Some(bytes) = bytes {
                    for line in String::from_utf8_lossy(bytes).lines() {
                        out.push_str(&format!("{prefix}{line}\n"));
                    }
                }
            }
        }
        for edit in &self.registry {
            if edit.before != edit.after {
                out.push_str(&format!(
                    "HKCU\\{}: {:?} -> {:?}\n",
                    edit.browser.registry_key(),
                    edit.before,
                    edit.after
                ));
            }
        }
        out
    }

    /// Verify the plan, back up changed files, then atomically replace individual manifests.
    /// Blocking and not cancellable mid-write; CLI supplies a worker deadline. A failed
    /// apply may have changed one browser; retrying a newly planned operation converges.
    pub fn apply(&self, registry: &mut dyn Registry) -> Result<Vec<PathBuf>, Error> {
        if self.is_noop() {
            return Ok(Vec::new());
        }
        for edit in &self.files {
            check_ancestors(&self.home, &edit.path)?;
            if read_file(&edit.path)? != edit.before {
                return Err(Error::Registration);
            }
        }
        for edit in &self.registry {
            if registry.get(edit.browser)? != edit.before {
                return Err(Error::Registration);
            }
        }
        let mut backups = Vec::new();
        for edit in &self.files {
            if edit.before == edit.after {
                continue;
            }
            let parent = edit.path.parent().ok_or(Error::Registration)?;
            fs::create_dir_all(parent)?;
            check_ancestors(&self.home, &edit.path)?;
            if let Some(bytes) = &edit.before {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|_| Error::Registration)?
                    .as_secs();
                let mut backup = tempfile::Builder::new()
                    .prefix(&format!("{HOST_NAME}.json.gcu-backup-{stamp}-"))
                    .tempfile_in(parent)?;
                // NamedTempFile is 0600 on Unix; Windows also needs an owner-only ACL.
                #[cfg(not(unix))]
                crate::private::restrict_owner(backup.path())?;
                backup.write_all(bytes)?;
                backup.as_file().sync_all()?;
                let (_, path) = backup.keep().map_err(|e| e.error)?;
                backups.push(path);
            }
            if let Some(bytes) = &edit.after {
                crate::private::write_secret(&edit.path, bytes)?;
            } else if edit.before.is_some() {
                fs::remove_file(&edit.path)?;
            }
        }
        for edit in &self.registry {
            if edit.before != edit.after {
                registry.set(edit.browser, edit.after.as_deref())?;
            }
        }
        Ok(backups)
    }
}

fn owned_origin(value: &Value) -> bool {
    value["allowed_origins"].as_array().is_some_and(|a| {
        a.len() == 1
            && a[0].as_str().is_some_and(|s| {
                s.strip_prefix("chrome-extension://")
                    .and_then(|s| s.strip_suffix('/'))
                    .is_some_and(|id| {
                        id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))
                    })
            })
    })
}
fn read_file(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 32_768 => {
            Ok(Some(fs::read(path)?))
        }
        Ok(_) => Err(Error::Registration),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn check_ancestors(home: &Path, path: &Path) -> Result<(), Error> {
    if home
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::Registration);
    }
    match fs::symlink_metadata(home) {
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(Error::Registration),
        Ok(_) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let relative = path.strip_prefix(home).map_err(|_| Error::Registration)?;
    let mut cursor = home.to_owned();
    for component in relative.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(Error::Registration);
        }
        cursor.push(component);
        match fs::symlink_metadata(&cursor) {
            Ok(m) if m.file_type().is_symlink() => return Err(Error::Registration),
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
