//! Reuse the Host inherited transport; never publish a control pathname/secret.
use crate::{transport::BoxStream, Error};
use std::process::{Command, Stdio};

/// Attach a private duplex control connection as the child's stdin. Returns
/// only the supervisor end. Call before spawn; dropping the command closes the
/// child end. No control content is placed on argv, env, or disk. Windows uses
/// Rust's stdio HANDLE_LIST spawn path (stdin/stdout/stderr only); no arbitrary
/// inheritable handles are enabled. Windows runtime proof is still not_run.
///
/// Brief socket/pipe creation; Windows connect caps at five seconds. No process
/// is started here. Cancellation drops the unexposed endpoints.
pub async fn prepare(command: &mut Command) -> Result<BoxStream, Error> {
    #[cfg(unix)]
    {
        let (parent, child) = std::os::unix::net::UnixStream::pair()?;
        parent.set_nonblocking(true)?;
        let child = std::os::fd::OwnedFd::from(child);
        command.stdin(Stdio::from(child));
        Ok(Box::new(tokio::net::UnixStream::from_std(parent)?))
    }
    #[cfg(windows)]
    {
        let name = format!(
            r"\\.\pipe\gcu-scope-{}",
            crate::auth::Secret::random().hex()
        );
        let server = crate::unsafe_platform::scope_pipe(&name)?;
        // An overlapped handle not registered with the parent's IOCP. The
        // engine registers its inherited duplicate with its own runtime.
        use std::os::windows::fs::OpenOptionsExt;
        let client = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(windows::Win32::Storage::FileSystem::FILE_FLAG_OVERLAPPED.0)
            .open(&name)?;
        tokio::time::timeout(crate::auth::HANDSHAKE_TIMEOUT, server.connect())
            .await
            .map_err(|_| Error::Timeout)??;
        command.stdin(Stdio::from(client));
        Ok(Box::new(server))
    }
}

/// Import only inherited private socket/pipe stdin, using the Host importer
/// which marks original and duplicate close-on-exec/non-inheritable. Regular
/// files, ordinary stdio pipes and environment/argv scope content are refused.
/// Nonblocking registration only; no timeout or cancellation.
pub fn inherited() -> Result<BoxStream, Error> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let file = crate::unsafe_platform::duplicate_file(0)?;
        if !file.metadata()?.file_type().is_socket() {
            return Err(Error::Authentication);
        }
        let stream = crate::transport::inherited(0)?;
        crate::unsafe_platform::detach_scope_stdin(0)?;
        Ok(stream)
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let handle = std::io::stdin().as_raw_handle() as usize;
        crate::unsafe_platform::check_scope_pipe(handle)?;
        let stream = crate::transport::inherited(handle)?;
        crate::unsafe_platform::detach_scope_stdin(handle)?;
        Ok(stream)
    }
}
