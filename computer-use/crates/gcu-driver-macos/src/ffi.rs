//! Security.framework signing calls that `security-framework` does not wrap.
//!
//! Team id and bundle signing id come from `SecCodeCopySigningInformation`.
//! The information flag `kSecCSSigningInformation` (`1 << 1`) is not the same
//! bit as the validation flags that reuse low bits. Validity uses flag 0 and a
//! null requirement so the OS does not show UI or fetch trust over the network
//! beyond its local check. The live guest read passes `kSecCSDefaultFlags` (0)
//! and does not call `SecCodeCheckValidity`.

use crate::identity::SignatureState;
use crate::scope_os::CodeIdentity;
use core_foundation::base::CFRelease;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::{
    array::{CFArray, CFArrayRef},
    base::{CFType, CFTypeRef, TCFType},
    data::{CFData, CFDataRef},
    dictionary::CFDictionaryRef,
    string::{CFString, CFStringRef},
};
use gcu_core::driver::{DriverError, Result};
use security_framework::certificate::SecCertificate;
use security_framework::os::macos::code_signing::{Flags, SecStaticCode};
use security_framework_sys::code_signing::{
    kSecGuestAttributePid, SecCodeCopyGuestWithAttributes, SecCodeRef, SecStaticCodeRef,
};
use std::{ffi::c_void, path::Path, ptr};

/// `errSecSuccess`.
const ERR_SEC_SUCCESS: i32 = 0;
/// `errSecCSUnsigned`.
const ERR_SEC_CS_UNSIGNED: i32 = -67_062;
/// `kSecCSSigningInformation`. Information flags, not validation flags.
const SIGNING_INFORMATION: u32 = 1 << 1;
/// `kSecCSDefaultFlags`. Guest lookup only. Not a validity or network flag.
const DEFAULT_FLAGS: u32 = 0;

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecCodeInfoTeamIdentifier: CFStringRef;
    static kSecCodeInfoIdentifier: CFStringRef;
    static kSecCodeInfoCertificates: CFStringRef;
    static kSecCodeInfoCdHashes: CFStringRef;
    static kSecCodeInfoUnique: CFStringRef;

    fn SecCodeCopySigningInformation(
        code: SecStaticCodeRef,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> i32;

    fn SecStaticCodeCheckValidity(
        static_code: SecStaticCodeRef,
        flags: u32,
        requirement: *const c_void,
    ) -> i32;
}

/// Signing facts before ad-hoc downgrade. No path is included in errors.
pub(crate) struct RawSigning {
    /// OS validity result.
    pub state: SignatureState,
    /// `kSecCodeInfoTeamIdentifier`.
    pub team_id: Option<String>,
    /// `kSecCodeInfoIdentifier`.
    pub signing_id: Option<String>,
    /// Leaf certificate subject summary.
    pub subject: Option<String>,
    /// SHA-256 of the leaf certificate DER.
    pub certificate_sha256: Option<String>,
    /// First code-directory hash, lowercase hex.
    pub cdhash: Option<String>,
}

/// Read a static code object. A missing file is [`DriverError::AppNotFound`].
/// An unreadable signature becomes unsigned rather than a path-bearing error.
pub(crate) fn read_signing(path: &Path) -> Result<RawSigning> {
    if std::fs::metadata(path).is_err() {
        return Err(DriverError::AppNotFound);
    }
    let Some(url) = core_foundation::url::CFURL::from_path(
        path,
        path.extension().is_some_and(|ext| ext == "app"),
    ) else {
        return Err(DriverError::AppNotFound);
    };
    let Ok(code) = SecStaticCode::from_path(&url, Flags::empty()) else {
        return Ok(empty(SignatureState::Unsigned));
    };
    let state = validity(&code);
    let mut raw = copy_info(&code).unwrap_or_else(empty_unsigned);
    if state == SignatureState::Unsigned {
        raw.state = SignatureState::Unsigned;
    } else if state == SignatureState::Invalid {
        raw.state = SignatureState::Invalid;
    }
    Ok(raw)
}

fn validity(code: &SecStaticCode) -> SignatureState {
    // SAFETY: `code` is a live static-code object. A null requirement checks the
    // embedded requirement only. Flag 0 does not present UI or force network
    // revocation retrieval.
    let status = unsafe { SecStaticCodeCheckValidity(code.as_concrete_TypeRef(), 0, ptr::null()) };
    match status {
        ERR_SEC_SUCCESS => SignatureState::Valid,
        ERR_SEC_CS_UNSIGNED => SignatureState::Unsigned,
        _ => SignatureState::Invalid,
    }
}

fn copy_info(code: &SecStaticCode) -> Option<RawSigning> {
    let mut dict: CFDictionaryRef = ptr::null_mut();
    // SAFETY: `code` is live. On success the out-dictionary is +1 and released
    // below. The flag requests signing information only.
    let status = unsafe {
        SecCodeCopySigningInformation(code.as_concrete_TypeRef(), SIGNING_INFORMATION, &mut dict)
    };
    if status != ERR_SEC_SUCCESS || dict.is_null() {
        if !dict.is_null() {
            // SAFETY: a non-null out-parameter from a failed call is still ours to release.
            unsafe { CFRelease(dict as CFTypeRef) };
        }
        return None;
    }
    // SAFETY: success returned a +1 dictionary. The guard releases it.
    let dict = dict;
    let team = dict_string(dict, unsafe { kSecCodeInfoTeamIdentifier });
    let signing_id = dict_string(dict, unsafe { kSecCodeInfoIdentifier });
    let certificate = first_certificate(dict_value(dict, unsafe { kSecCodeInfoCertificates }));
    let cdhash = cdhash_of(dict);
    // SAFETY: balances the +1 from SecCodeCopySigningInformation.
    unsafe { CFRelease(dict as CFTypeRef) };
    let (subject, certificate_sha256) = match certificate {
        Some(cert) => {
            let summary = {
                let text = cert.subject_summary();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            };
            let hash = sha256_hex(&cert.to_der());
            (summary, Some(hash))
        }
        None => (None, None),
    };
    Some(RawSigning {
        state: SignatureState::Valid,
        team_id: team,
        signing_id,
        subject,
        certificate_sha256,
        cdhash,
    })
}

fn cdhash_of(dict: CFDictionaryRef) -> Option<String> {
    // SAFETY: both keys are process-lifetime Security.framework constants.
    let hashes = dict_value(dict, unsafe { kSecCodeInfoCdHashes });
    if let Some(hash) = first_data_hex(hashes) {
        return Some(hash);
    }
    data_hex(dict_value(dict, unsafe { kSecCodeInfoUnique }))
}

fn dict_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    let value = dict_value(dict, key)?;
    if !value.instance_of::<CFString>() {
        return None;
    }
    // SAFETY: the type id matches CFString. Get-rule retains for the wrapper.
    let text = unsafe { CFString::wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef) };
    let text = text.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn dict_value(dict: CFDictionaryRef, key: CFStringRef) -> Option<CFType> {
    // SAFETY: `dict` is a live dictionary and `key` is a live CFString.
    // CFDictionaryGetValue returns +0; wrap_under_get_rule retains it.
    let value = unsafe { CFDictionaryGetValue(dict, key as *const c_void) };
    if value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_get_rule(value as CFTypeRef) })
    }
}

fn first_certificate(value: Option<CFType>) -> Option<SecCertificate> {
    let value = value?;
    if !value.instance_of::<CFArray>() {
        return None;
    }
    // SAFETY: type id matches CFArray. The array is borrowed from the dictionary.
    let array: CFArray<CFType> =
        unsafe { CFArray::wrap_under_get_rule(value.as_CFTypeRef() as CFArrayRef) };
    let item = array.get(0)?;
    if !item.instance_of::<SecCertificate>() {
        return None;
    }
    // SAFETY: type id matches SecCertificate.
    Some(unsafe { SecCertificate::wrap_under_get_rule(item.as_CFTypeRef() as _) })
}

fn first_data_hex(value: Option<CFType>) -> Option<String> {
    let value = value?;
    if !value.instance_of::<CFArray>() {
        return None;
    }
    // SAFETY: type id matches CFArray.
    let array: CFArray<CFType> =
        unsafe { CFArray::wrap_under_get_rule(value.as_CFTypeRef() as CFArrayRef) };
    let item = array.get(0)?;
    data_hex(Some(item.as_CFType()))
}

fn data_hex(value: Option<CFType>) -> Option<String> {
    let value = value?;
    if !value.instance_of::<CFData>() {
        return None;
    }
    // SAFETY: type id matches CFData.
    let data = unsafe { CFData::wrap_under_get_rule(value.as_CFTypeRef() as CFDataRef) };
    let bytes = data.bytes();
    if bytes.is_empty() {
        None
    } else {
        Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn empty(state: SignatureState) -> RawSigning {
    RawSigning {
        state,
        team_id: None,
        signing_id: None,
        subject: None,
        certificate_sha256: None,
        cdhash: None,
    }
}

fn empty_unsigned() -> RawSigning {
    empty(SignatureState::Unsigned)
}

/// Live code identity of `pid`.
///
/// Calls `SecCodeCopyGuestWithAttributes` with `kSecGuestAttributePid` and
/// `kSecCSDefaultFlags`, then `SecCodeCopySigningInformation` with
/// `kSecCSSigningInformation`. Each call reads again. There is no cache.
///
/// Blocks the caller inside Security.framework. No thread, timeout, or
/// cancellation. Does not call `SecCodeCheckValidity`, show UI, or fetch
/// trust. A path-based [`SecStaticCode`] read is not a fallback. Pid reuse
/// is the caller's check: this function does not read the process start time.
///
/// Returns `None` when `pid` is `0`, does not fit in a positive `pid_t`,
/// does not exist, the call fails, or the identifier or cdhash is missing.
/// Team id may be absent. A partial identity is not returned.
pub(crate) fn guest_code_identity(pid: u32) -> Option<CodeIdentity> {
    let pid = i32::try_from(pid).ok().filter(|value| *value > 0)?;
    let code = copy_guest(pid)?;
    let (dict, dict_owner) = copy_guest_info(&code)?;
    // SAFETY: both keys are process-lifetime Security.framework constants.
    let (identifier_key, team_key) =
        unsafe { (kSecCodeInfoIdentifier, kSecCodeInfoTeamIdentifier) };
    let identifier = dict_string(dict, identifier_key)?;
    let team_id = dict_string(dict, team_key);
    let cdhash = cdhash_of(dict)?;
    drop(dict_owner);
    drop(code);
    Some(CodeIdentity {
        identifier,
        team_id,
        cdhash,
    })
}

fn copy_guest(pid: i32) -> Option<CFType> {
    // SAFETY: `kSecGuestAttributePid` is a process-lifetime CFString constant.
    // wrap_under_get_rule retains it. Drop releases that retain.
    let key = unsafe { CFString::wrap_under_get_rule(kSecGuestAttributePid) };
    let attributes = CFDictionary::from_CFType_pairs(&[(key, CFNumber::from(pid))]);
    let mut guest: SecCodeRef = ptr::null_mut();
    // SAFETY: `attributes` is a live dictionary and is not retained by the call.
    // A null host selects the caller. `guest` is a writable out-parameter.
    // `DEFAULT_FLAGS` is `kSecCSDefaultFlags`: no UI and no network trust fetch.
    // On success `guest` is +1. A non-null out-parameter on failure is also +1.
    let status = unsafe {
        SecCodeCopyGuestWithAttributes(
            ptr::null_mut(),
            attributes.as_concrete_TypeRef(),
            DEFAULT_FLAGS,
            &mut guest,
        )
    };
    if status != ERR_SEC_SUCCESS {
        drop(own_created(guest as CFTypeRef));
        return None;
    }
    own_created(guest as CFTypeRef)
}

fn copy_guest_info(code: &CFType) -> Option<(CFDictionaryRef, CFType)> {
    let mut dict: CFDictionaryRef = ptr::null_mut();
    // SAFETY: `code` is a live SecCode. SecCode is a SecStaticCode, so the
    // pointer is the argument `SecCodeCopySigningInformation` accepts.
    // `SIGNING_INFORMATION` requests the info dictionary only. It is not
    // `SecCodeCheckValidity`. On success `dict` is +1. A non-null
    // out-parameter on failure is also +1.
    let status = unsafe {
        SecCodeCopySigningInformation(
            code.as_CFTypeRef() as SecStaticCodeRef,
            SIGNING_INFORMATION,
            &mut dict,
        )
    };
    if status != ERR_SEC_SUCCESS {
        drop(own_created(dict as CFTypeRef));
        return None;
    }
    let owner = own_created(dict as CFTypeRef)?;
    Some((dict, owner))
}

/// Own a +1 CF object. Null is absence.
fn own_created(reference: CFTypeRef) -> Option<CFType> {
    if reference.is_null() {
        return None;
    }
    // SAFETY: `reference` is a non-null +1 CF object. Drop releases it.
    Some(unsafe { CFType::wrap_under_create_rule(reference) })
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
}
