//! Independent token oracle for the exact elevated fixture PID.
use windows::Win32::{Foundation::*, Security::*, System::Threading::*};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this oracle owns each process/token handle exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>, String> {
    let mut size = 0;
    // SAFETY: query size only; expected insufficient-buffer error is checked by size.
    unsafe {
        let _ = GetTokenInformation(token, class, None, 0, &mut size);
    }
    if size == 0 || size > 16384 {
        return Err(format!(
            "GetTokenInformation({class:?}) invalid size={size}"
        ));
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned output is at least size bytes; token remains owned by caller.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(|e| {
        format!(
            "GetTokenInformation({class:?}): HRESULT={:?}; {e}",
            e.code()
        )
    })?;
    Ok(buffer)
}
pub fn facts(pid: u32) -> Result<String, String> {
    // SAFETY: query-only access to the explicitly supplied fixture PID.
    let process = Handle(
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|e| format!("OpenProcess({pid}): {e}"))?,
    );
    let mut token = HANDLE::default();
    // SAFETY: owned process, initialized token output and query-only rights.
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let token = Handle(token);
    let elevation = info(token.0, TokenElevation)?;
    let integrity = info(token.0, TokenIntegrityLevel)?;
    // SAFETY: OS TokenIntegrityLevel result contains an aligned mandatory label
    // and its SID in the still-live buffer; validate SID before subauthority access.
    let rid = unsafe {
        let label = &*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
        if !IsValidSid(label.Label.Sid).as_bool() {
            return Err("invalid integrity SID".into());
        }
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err("empty integrity SID".into());
        }
        *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1))
    };
    // SAFETY: OS TokenElevation result contains one aligned TOKEN_ELEVATION.
    let elevated = unsafe { (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated };
    Ok(format!("pid={pid}; GetTokenInformation(TokenElevation)={elevated}; GetTokenInformation(TokenIntegrityLevel)=S-1-16-{rid} (0x{rid:x})"))
}
