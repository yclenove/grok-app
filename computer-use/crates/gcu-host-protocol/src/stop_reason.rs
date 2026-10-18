//! Display-only stop reason marker. It never grants, clears, or withholds Stop.
//!
//! The engine writes one token before it drops the control connection. The App
//! may show that token. Missing, forged, or deleted contents do not change the
//! persistent Stop decision.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Why an engine-local stop closed the control connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// Local hotkey.
    Hotkey,
    /// Panel button on the engine side.
    Panel,
    /// Engine-internal failure after the latch.
    InternalError,
}

impl StopReason {
    /// File token. No other text is accepted.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Hotkey => "hotkey",
            Self::Panel => "panel",
            Self::InternalError => "internal_error",
        }
    }

    /// Exact token, optional trailing newline. Anything else is ignored.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(bytes).ok()?;
        let text = text.strip_suffix('\n').unwrap_or(text);
        let text = text.strip_suffix('\r').unwrap_or(text);
        match text {
            "hotkey" => Some(Self::Hotkey),
            "panel" => Some(Self::Panel),
            "internal_error" => Some(Self::InternalError),
            _ => None,
        }
    }
}

/// File name inside the private state directory.
pub const FILE_NAME: &str = "stop-reason";

/// Private state directory shared by the engine and the App.
///
/// Unix: `$XDG_STATE_HOME/grok-computer-use` or `~/.local/state/grok-computer-use`.
/// macOS and Windows append `state`. This does not create the directory.
#[must_use]
pub fn state_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let dir = root?.join("grok-computer-use");
    #[cfg(any(windows, target_os = "macos"))]
    let dir = dir.join("state");
    Some(dir)
}

/// Default marker path. `None` when the state directory cannot be resolved.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    Some(state_dir()?.join(FILE_NAME))
}

/// Read a marker for display. IO errors and unexpected bytes yield `None`.
///
/// Blocking: one short read. Timeout: none. Cancellation: none.
/// A missing file is not an error and does not clear Stop.
#[must_use]
pub fn read_display(path: &Path) -> Option<StopReason> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 32 {
        return None;
    }
    StopReason::parse(&bytes)
}

/// Read the default marker. Failure and absence are both `None`.
#[must_use]
pub fn read_default() -> Option<StopReason> {
    read_display(&default_path()?)
}

/// Create `dir` as owner-only and write the token as an owner-only file.
///
/// The directory is `0700` and the file is `0600` on Unix. Windows receives an
/// owner-only DACL. Blocking: the write itself. Timeout: caller's budget.
/// Cancellation: none; a partial temp file is not used.
pub fn write_marker(dir: &Path, reason: StopReason) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    restrict(dir)?;
    let path = dir.join(FILE_NAME);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(reason.token().as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    restrict(&path)?;
    Ok(path)
}

fn restrict(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        windows_acl::secure(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(windows)]
mod windows_acl {
    use super::*;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW,
                    SE_FILE_OBJECT,
                },
                GetSecurityDescriptorDacl, ACL, DACL_SECURITY_INFORMATION,
                PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            },
        },
    };

    const SDDL: &str = "D:P(A;;FA;;;OW)";

    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: ConvertStringSecurityDescriptor allocated this with LocalAlloc.
            unsafe {
                LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }

    pub(super) fn secure(path: &Path) -> io::Result<()> {
        let wide: Vec<u16> = SDDL.encode_utf16().chain(Some(0)).collect();
        let mut raw = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` is NUL-terminated and `raw` is writable output storage.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                1,
                &mut raw,
                None,
            )
        }
        .map_err(io::Error::other)?;
        let descriptor = Descriptor(raw);
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut present = windows::core::BOOL::default();
        let mut defaulted = windows::core::BOOL::default();
        // SAFETY: the descriptor is live and every output pointer is writable.
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut acl, &mut defaulted) }
            .map_err(io::Error::other)?;
        let name: Vec<u16> = std::os::windows::ffi::OsStrExt::encode_wide(path.as_os_str())
            .chain(Some(0))
            .collect();
        // SAFETY: `name` is NUL-terminated and the ACL lives until this call returns.
        unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(name.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(acl),
                None,
            )
        }
        .ok()
        .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_only_the_reason_enum() {
        assert_eq!(StopReason::parse(b"hotkey\n"), Some(StopReason::Hotkey));
        assert_eq!(StopReason::parse(b"panel"), Some(StopReason::Panel));
        assert_eq!(
            StopReason::parse(b"internal_error\r\n"),
            Some(StopReason::InternalError)
        );
        assert_eq!(StopReason::parse(b"hotkey "), None);
        assert_eq!(StopReason::parse(b"resume"), None);
        assert_eq!(StopReason::parse(b""), None);
        assert_eq!(StopReason::parse(&[0xff]), None);
    }

    #[test]
    fn missing_and_forged_markers_are_not_a_reason() {
        let dir = tempfile_dir();
        assert_eq!(read_display(&dir.join(FILE_NAME)), None);
        std::fs::write(dir.join(FILE_NAME), b"running\n").unwrap();
        assert_eq!(read_display(&dir.join(FILE_NAME)), None);
        let _ = std::fs::remove_file(dir.join(FILE_NAME));
        assert_eq!(read_display(&dir.join(FILE_NAME)), None);
    }

    #[cfg(unix)]
    #[test]
    fn marker_directory_and_file_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile_dir();
        let path = write_marker(&dir, StopReason::Hotkey).unwrap();
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        assert_eq!(read_display(&path), Some(StopReason::Hotkey));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gcu-stop-reason-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
