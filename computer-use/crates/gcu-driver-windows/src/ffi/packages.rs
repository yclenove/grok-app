//! Resolve registered packaged-app manifests to the real entry executable.
use super::{allocated_string, native, string, wide};
use ::windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::ERROR_INSUFFICIENT_BUFFER,
        Storage::Packaging::Appx::*,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, STGM_READ, STGM_SHARE_DENY_WRITE},
        UI::Shell::SHCreateStreamOnFileEx,
    },
};
use gcu_core::driver::{DriverError, Result};
use std::path::Path;

pub(super) fn executable(aumid: &str) -> Result<String> {
    let (family, _) = aumid.split_once('!').ok_or(DriverError::AppNotFound)?;
    let family = wide(family);
    let mut count = 0;
    let mut length = 0;
    // SAFETY: sizing query with two writable sizes and no output buffers.
    let status = unsafe {
        GetPackagesByPackageFamily(PCWSTR(family.as_ptr()), &mut count, None, &mut length, None)
    };
    if status != ERROR_INSUFFICIENT_BUFFER || count == 0 || count > 128 || length > 32768 {
        return Err(DriverError::AppNotFound);
    }
    let mut names = vec![PWSTR::null(); count as usize];
    let mut buffer = vec![0u16; length as usize];
    // SAFETY: both buffers have the exact sizes returned by the OS; pointers in
    // names reference buffer and are only used while buffer remains live.
    unsafe {
        GetPackagesByPackageFamily(
            PCWSTR(family.as_ptr()),
            &mut count,
            Some(names.as_mut_ptr()),
            &mut length,
            Some(PWSTR(buffer.as_mut_ptr())),
        )
        .ok()
        .map_err(native)?;
    }
    let mut candidates = Vec::new();
    for name in names.into_iter().take(count as usize) {
        let mut path_length = 0;
        // SAFETY: name points into the live package-name buffer; size-only query.
        let status = unsafe { GetPackagePathByFullName(PCWSTR(name.0), &mut path_length, None) };
        if status != ERROR_INSUFFICIENT_BUFFER || path_length == 0 || path_length > 32768 {
            return Err(DriverError::AppNotFound);
        }
        let mut path = vec![0u16; path_length as usize];
        // SAFETY: correctly sized output buffer and package full-name pointer.
        unsafe {
            GetPackagePathByFullName(
                PCWSTR(name.0),
                &mut path_length,
                Some(PWSTR(path.as_mut_ptr())),
            )
            .ok()
            .map_err(native)?;
        }
        let root = string(&path);
        let manifest = wide(&format!("{root}\\AppxManifest.xml"));
        // SAFETY: COM initialized by caller; streams and interfaces stay local.
        let entry = unsafe {
            let stream = SHCreateStreamOnFileEx(
                PCWSTR(manifest.as_ptr()),
                (STGM_READ | STGM_SHARE_DENY_WRITE).0,
                0,
                false,
                None,
            )
            .map_err(native)?;
            let factory: IAppxFactory =
                CoCreateInstance(&AppxFactory, None, CLSCTX_INPROC_SERVER).map_err(native)?;
            let reader = factory.CreateManifestReader(&stream).map_err(native)?;
            let apps = reader.GetApplications().map_err(native)?;
            let mut found = None;
            while apps.GetHasCurrent().map_err(native)?.as_bool() {
                let app = apps.GetCurrent().map_err(native)?;
                let id = allocated_string(app.GetAppUserModelId().map_err(native)?)?;
                if id == aumid {
                    found = Some(allocated_string(
                        app.GetStringValue(::windows::core::w!("Executable"))
                            .map_err(native)?,
                    )?);
                }
                let _ = apps.MoveNext().map_err(native)?;
            }
            found
        };
        if let Some(entry) = entry {
            let path = Path::new(&root).join(entry);
            let canonical = std::fs::canonicalize(&path).map_err(super::io)?;
            let canonical_root = std::fs::canonicalize(&root).map_err(super::io)?;
            if !canonical.starts_with(&canonical_root) {
                return Err(DriverError::InvalidRequest);
            }
            candidates.push(canonical.to_string_lossy().into_owned());
        }
    }
    candidates.sort();
    candidates.dedup();
    match candidates.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(DriverError::AppNotFound),
        _ => Err(DriverError::AmbiguousApp),
    }
}
