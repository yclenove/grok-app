//! The only foreign calls in this crate.

/// Deliver SIGKILL to the process group whose id is `pid`.
///
/// `pid` must be a child placed in its own group with `process_group(0)`.
/// Zero is ignored so this cannot signal the evaluator's own group.
#[cfg(unix)]
pub(crate) fn kill_process_group(pid: u32) {
    let Some(pgid) = i32::try_from(pid).ok().filter(|value| *value > 0) else {
        return;
    };
    // SAFETY: `kill` with a negative pid signals that process group and no
    // other. The id is the child we created, not zero and not this process.
    // SIGKILL does not share memory with the caller.
    let _ = unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

/// Fill `buf` from the system RNG. Failure is an error; there is no weak fallback.
#[cfg(windows)]
pub(crate) fn random_bytes(buf: &mut [u8]) -> Result<(), crate::Error> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(
            algorithm: *mut core::ffi::c_void,
            buffer: *mut u8,
            length: u32,
            flags: u32,
        ) -> i32;
    }
    const USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;
    let length =
        u32::try_from(buf.len()).map_err(|_| crate::Error::msg("could not read randomness"))?;
    // SAFETY: a null algorithm plus BCRYPT_USE_SYSTEM_PREFERRED_RNG writes
    // exactly `length` bytes into `buf` and does not retain the pointer.
    let status = unsafe {
        BCryptGenRandom(
            core::ptr::null_mut(),
            buf.as_mut_ptr(),
            length,
            USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status == 0 {
        Ok(())
    } else {
        Err(crate::Error::msg("could not read randomness"))
    }
}
