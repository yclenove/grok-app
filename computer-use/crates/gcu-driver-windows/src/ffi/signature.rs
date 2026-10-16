//! Coherent file hashing and Authenticode certificate/product provenance.
use super::{io, native, string, wide};
use crate::{
    api::ImageInfo,
    identity::{SignatureInfo, SignatureState},
};
use ::windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HANDLE, HWND},
        Security::{Cryptography::*, WinTrust::*},
        Storage::FileSystem::*,
    },
};
use gcu_core::driver::{DriverError, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
};

struct Trust {
    data: WINTRUST_DATA,
    action: ::windows::core::GUID,
}
impl Drop for Trust {
    fn drop(&mut self) {
        self.data.dwStateAction = WTD_STATEACTION_CLOSE;
        // SAFETY: WinVerifyTrust owns state until a matched close; same action/data.
        unsafe {
            WinVerifyTrust(
                HWND::default(),
                &mut self.action,
                (&mut self.data as *mut WINTRUST_DATA).cast(),
            );
        }
    }
}
struct Query {
    store: HCERTSTORE,
    msg: *mut std::ffi::c_void,
}
impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: these handles were returned by CryptQueryObject; closed once.
        unsafe {
            if !self.msg.is_null() {
                let _ = CryptMsgClose(Some(self.msg));
            }
            if !self.store.0.is_null() {
                let _ = CertCloseStore(Some(self.store), 0);
            }
        }
    }
}
fn signature(path: PCWSTR, file: HANDLE) -> Result<SignatureInfo> {
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path,
        hFile: file,
        ..Default::default()
    };
    let mut trust = Trust {
        action: WINTRUST_ACTION_GENERIC_VERIFY_V2,
        data: WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_WHOLECHAIN,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut info },
            dwStateAction: WTD_STATEACTION_VERIFY,
            dwProvFlags: WTD_REVOCATION_CHECK_CHAIN_EXCLUDE_ROOT | WTD_CACHE_ONLY_URL_RETRIEVAL,
            ..Default::default()
        },
    };
    // SAFETY: file is held read-only without write/delete sharing; buffers live
    // through verification and state close. No UI or online retrieval occurs.
    let status = unsafe {
        WinVerifyTrust(
            HWND::default(),
            &mut trust.action,
            (&mut trust.data as *mut WINTRUST_DATA).cast(),
        )
    };
    if status != 0 {
        return Ok(SignatureInfo {
            state: SignatureState::from_wintrust_status(status),
            publisher: None,
            certificate_sha256: None,
            product: None,
        });
    }
    let mut query = Query {
        store: HCERTSTORE::default(),
        msg: std::ptr::null_mut(),
    };
    // SAFETY: path is null-terminated; query owns both output handles.
    unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            path.0.cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            None,
            None,
            None,
            Some(&mut query.store),
            Some(&mut query.msg),
            None,
        )
        .map_err(native)?;
    }
    // SAFETY: helpers access the live verified provider state, not an arbitrary
    // certificate selected by subject name from the untrusted embedded store.
    let certificate = unsafe {
        let provider = WTHelperProvDataFromStateData(trust.data.hWVTStateData);
        if provider.is_null() {
            return Err(native("trust state"));
        }
        let signer = WTHelperGetProvSignerFromChain(provider, 0, false, 0);
        if signer.is_null() {
            return Err(native("trust signer"));
        }
        let cert = WTHelperGetProvCertFromChain(signer, 0);
        if cert.is_null() || (*cert).pCert.is_null() {
            return Err(native("trust certificate"));
        }
        (*cert).pCert
    };
    // SAFETY: verified certificate lives in the trust state until after copying.
    let (publisher, certificate_sha256) = unsafe {
        let size = CertGetNameStringW(certificate, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
        if size == 0 || size > 32768 {
            return Err(native("certificate subject"));
        }
        let mut subject = vec![0u16; size as usize];
        if CertGetNameStringW(
            certificate,
            CERT_NAME_SIMPLE_DISPLAY_TYPE,
            0,
            None,
            Some(&mut subject),
        ) == 0
        {
            return Err(native("certificate subject"));
        }
        let cert = &*certificate;
        let der = std::slice::from_raw_parts(cert.pbCertEncoded, cert.cbCertEncoded as usize);
        (string(&subject), hex(&Sha256::digest(der)))
    };
    Ok(SignatureInfo {
        state: SignatureState::Valid,
        publisher: Some(publisher),
        certificate_sha256: Some(certificate_sha256),
        product: product(path)?,
    })
}
fn product(path: PCWSTR) -> Result<Option<String>> {
    // SAFETY: path is held stable by the non-write-shared image handle.
    let size = unsafe { GetFileVersionInfoSizeW(path, None) };
    if size == 0 {
        return Ok(None);
    }
    if size > 1024 * 1024 {
        return Err(DriverError::InvalidRequest);
    }
    let mut bytes = vec![0u8; size as usize];
    // SAFETY: correct output size and writable buffer, path remains stable.
    unsafe {
        GetFileVersionInfoW(path, None, size, bytes.as_mut_ptr().cast()).map_err(native)?;
    }
    let mut value = std::ptr::null_mut();
    let mut len = 0;
    let translation = wide("\\VarFileInfo\\Translation");
    // SAFETY: VerQueryValue yields a pointer inside the live resource buffer.
    if !unsafe {
        VerQueryValueW(
            bytes.as_ptr().cast(),
            PCWSTR(translation.as_ptr()),
            &mut value,
            &mut len,
        )
    }
    .as_bool()
        || len < 4
    {
        return Ok(None);
    }
    // SAFETY: length verified; reads unaligned words from the resource buffer.
    let (language, codepage) = unsafe {
        (
            value.cast::<u16>().read_unaligned(),
            value.cast::<u16>().add(1).read_unaligned(),
        )
    };
    let mut fields = Vec::new();
    for field in ["OriginalFilename", "ProductName"] {
        let key = wide(&format!(
            "\\StringFileInfo\\{language:04x}{codepage:04x}\\{field}"
        ));
        // SAFETY: queried pointer is inside the live version buffer, len in u16s.
        unsafe {
            if VerQueryValueW(
                bytes.as_ptr().cast(),
                PCWSTR(key.as_ptr()),
                &mut value,
                &mut len,
            )
            .as_bool()
                && len > 0
            {
                fields.push(string(std::slice::from_raw_parts(
                    value.cast::<u16>(),
                    len as usize,
                )));
            }
        }
    }
    Ok(if fields.is_empty() {
        None
    } else {
        Some(fields.join(" / "))
    })
}
pub(super) fn image(path: &str) -> Result<ImageInfo> {
    let _timing = crate::timing::Timing::start("image_verify");
    // Deny write/delete sharing so hash, path, certificate and product refer to
    // the same immutable file while these native calls run (also resolves junctions).
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(path)
        .map_err(io)?;
    let handle = HANDLE(file.as_raw_handle());
    let mut final_path = vec![0u16; 32768];
    // SAFETY: file handle stays open; output buffer length is provided by binding.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            &mut final_path,
            GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0),
        )
    };
    if count == 0 || count as usize >= final_path.len() {
        return Err(native("canonical file path"));
    }
    let canonical_path = string(&final_path[..count as usize]);
    let path = wide(&canonical_path);
    let hashing = crate::timing::Timing::start("image_hash");
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    drop(hashing);
    let signature = {
        let _timing = crate::timing::Timing::start("authenticode");
        signature(PCWSTR(path.as_ptr()), handle)?
    };
    // Conservative boundary: user profile/temp/program data images can load
    // user-writable plugins; absence of this signal is explicitly not a proof.
    let writable_load_path = {
        let _timing = crate::timing::Timing::start("image_load_path");
        writable_loading_path(&canonical_path)?
    };
    Ok(ImageInfo {
        canonical_path,
        sha256: hex(&digest.finalize()),
        signature,
        writable_load_path,
    })
}

fn writable_loading_path(path: &str) -> Result<bool> {
    let normalized = crate::identity::normalize_path(path)?;
    let Some(parent) = std::path::Path::new(&normalized).parent() else {
        return Ok(true);
    };
    let mut dirs = vec![
        parent.to_path_buf(),
        parent.join("plugins"),
        parent.join("extensions"),
    ];
    if let Some(paths) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&paths));
    }
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let path = wide(&dir.to_string_lossy());
        // SAFETY: directory is opened only to check FILE_ADD_FILE access, never
        // modified. BACKUP_SEMANTICS permits an owned directory handle.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                FILE_ADD_FILE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        };
        if let Ok(handle) = handle {
            let _handle = super::Handle(handle);
            return Ok(true);
        }
    }
    Ok(false)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
