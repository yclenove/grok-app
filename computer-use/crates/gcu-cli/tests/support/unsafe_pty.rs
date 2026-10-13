//! Linux PTY fixture using the existing MIT/Apache-2.0 libc dependency.
use std::{fs::File, io, os::fd::FromRawFd};
pub(super) fn stop_process(pid: u32) -> io::Result<()> {
    let pid = i32::try_from(pid).map_err(|_| io::Error::other("invalid child pid"))?;
    // SAFETY: pid identifies the live test-owned child; SIGTERM has no memory preconditions.
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(super) fn open() -> io::Result<(File, File)> {
    let (mut master, mut slave) = (-1, -1);
    // SAFETY: output descriptors point to initialized integers; null name,
    // termios and winsize request system defaults for a fresh test-only PTY.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openpty returned two distinct owned file descriptors.
    let (master_file, slave_file) =
        unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    // SAFETY: master remains open and F_SETFL changes only its nonblocking flag.
    if unsafe { libc::fcntl(master, libc::F_SETFL, libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok((master_file, slave_file))
}
