//! Platform ownership and discretionary access checks. All CLI FFI lives here.
use std::{fs::Metadata, io, path::Path};

#[cfg(unix)]
pub(super) fn secure(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if path.is_dir() { 0o700 } else { 0o600 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
#[cfg(unix)]
pub(super) fn check(_: &Path, metadata: &Metadata, directory: bool) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no arguments or memory preconditions.
    let uid = unsafe { libc::geteuid() };
    let mode = if directory { 0o700 } else { 0o600 };
    if metadata.uid() != uid || metadata.mode() & 0o777 != mode {
        return Err(io::Error::other("unsafe private path"));
    }
    Ok(())
}

#[cfg(windows)]
mod windows_acl {
    use super::*;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::{LocalFree, HLOCAL},
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW,
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                    SetNamedSecurityInfoW, SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
                PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
            },
        },
    };
    const SDDL: &str = "D:P(A;;FA;;;OW)";
    struct Descriptor(PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: this allocation was returned by a LocalAlloc-backed security API.
            unsafe {
                LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }
    fn descriptor() -> io::Result<Descriptor> {
        let wide: Vec<u16> = SDDL.encode_utf16().chain(Some(0)).collect();
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: wide is terminated and sd points to initialized writable storage.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                1,
                &mut sd,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Descriptor(sd))
    }
    fn name(path: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }
    pub(super) fn secure(path: &Path) -> io::Result<()> {
        use windows::Win32::Security::{GetSecurityDescriptorDacl, ACL};
        let sd = descriptor()?;
        let mut acl: *mut ACL = std::ptr::null_mut();
        let mut present = windows::core::BOOL::default();
        let mut defaulted = windows::core::BOOL::default();
        // SAFETY: descriptor is valid and all output pointers are writable.
        unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted) }
            .map_err(io::Error::other)?;
        let wide = name(path);
        // SAFETY: name is terminated; ACL is alive for the duration of the call.
        unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
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
    pub(super) fn check(path: &Path) -> io::Result<()> {
        let wide = name(path);
        let mut sd = PSECURITY_DESCRIPTOR::default();
        // SAFETY: path is terminated and sd is a writable security descriptor output.
        unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut sd,
            )
        }
        .ok()
        .map_err(io::Error::other)?;
        let sd = Descriptor(sd);
        let mut text = PWSTR::null();
        // SAFETY: descriptor is valid; the API allocates the output string.
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd.0,
                1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        }
        .map_err(io::Error::other)?;
        // SAFETY: successful conversion returns a terminated UTF-16 allocation.
        let string = unsafe { text.to_string() }.map_err(io::Error::other);
        // SAFETY: text was allocated by the LocalAlloc-backed conversion API.
        unsafe {
            LocalFree(Some(HLOCAL(text.0.cast())));
        }
        if string? != SDDL {
            return Err(io::Error::other("unsafe private ACL"));
        }
        Ok(())
    }
    pub(crate) fn pipe(
        name: &str,
        first: bool,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        let sd = descriptor()?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0 .0,
            bInheritHandle: false.into(),
        };
        // SAFETY: SECURITY_ATTRIBUTES and its descriptor remain alive during creation.
        unsafe {
            tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(
                    name,
                    (&sa as *const SECURITY_ATTRIBUTES).cast_mut().cast(),
                )
        }
    }
}
#[cfg(windows)]
pub(super) fn secure(path: &Path) -> io::Result<()> {
    windows_acl::secure(path)
}
#[cfg(windows)]
pub(super) fn check(path: &Path, _: &Metadata, _: bool) -> io::Result<()> {
    windows_acl::check(path)
}
#[cfg(windows)]
pub(crate) use windows_acl::pipe;
