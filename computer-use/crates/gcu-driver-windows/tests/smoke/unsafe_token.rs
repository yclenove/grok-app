//! Query-only token oracle, independent of the driver's permission decision.
use super::integrity::{TokenApi, TokenFacts};
use windows::Win32::{Foundation::*, Security::*, System::Threading::*};

pub struct NativeTokens;
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: this oracle owns each valid kernel handle exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>, String> {
    let mut size = 0;
    // SAFETY: size-only query; the expected insufficient-buffer result is
    // validated by the returned size before allocating an aligned buffer.
    unsafe {
        let _ = GetTokenInformation(token, class, None, 0, &mut size);
    }
    if size == 0 || size > 16384 {
        return Err(format!(
            "GetTokenInformation({class:?}) invalid size={size}"
        ));
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned buffer has at least size bytes, and caller owns the token.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )
    }
    .map_err(|e| format!("GetTokenInformation({class:?}): {e}"))?;
    Ok(buffer)
}
impl TokenApi for NativeTokens {
    fn facts(&self, pid: u32) -> Result<TokenFacts, String> {
        // SAFETY: query-only access to the exact Settings or harness PID.
        let process = Handle(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .map_err(|e| format!("OpenProcess({pid}): {e}"))?,
        );
        let mut token = HANDLE::default();
        // SAFETY: valid owned process handle and initialized token output.
        unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) }
            .map_err(|e| format!("OpenProcessToken({pid}): {e}"))?;
        let token = Handle(token);
        let integrity = info(token.0, TokenIntegrityLevel)?;
        let elevation = info(token.0, TokenElevation)?;
        // SAFETY: OS buffer is aligned and live; validate its SID before reading
        // the final subauthority containing the mandatory integrity RID.
        let integrity_rid = unsafe {
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
        Ok(TokenFacts {
            pid,
            integrity_rid,
            elevated,
        })
    }
}
