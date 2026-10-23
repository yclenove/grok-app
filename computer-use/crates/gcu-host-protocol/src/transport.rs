//! Private inherited streams and pinned fallback endpoints.
use crate::{
    auth::{Bootstrap, ProcessInstance},
    Error,
};
use interprocess::local_socket::{
    prelude::*,
    tokio::{prelude::*, Listener, Stream},
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};
use tokio::io::{AsyncRead, AsyncWrite};
/// Object-safe duplex byte stream; no third-party transport leaks into App APIs.
pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
/// Owned connection. Drop closes the transport.
pub type BoxStream = Box<dyn Duplex>;
/// Import a private inherited duplex descriptor/handle. Duplicates ownership so
/// invalid integers fail as IO errors. Marks the original and duplicate as
/// non-inheritable. Brief nonblocking OS registration only.
pub fn inherited(fd: usize) -> Result<BoxStream, Error> {
    Ok(Box::new(crate::unsafe_platform::duplicate_socket(fd)?))
}
/// Read a length-prefixed launch bootstrap from a secret pipe/socket fd. The
/// complete read has a five-second deadline and is cancellable without leaking
/// a blocking worker. No regular files or public named endpoints are read.
pub async fn bootstrap_fd(fd: usize) -> Result<Bootstrap, Error> {
    let mut reader = SecretReader::new(fd)?;
    tokio::time::timeout(crate::auth::HANDSHAKE_TIMEOUT, async {
        let mut prefix = [0; 4];
        reader.exact(&mut prefix).await?;
        let n = u32::from_be_bytes(prefix) as usize;
        if n == 0 || n > 4096 {
            return Err(Error::Invalid);
        }
        let mut bytes = vec![0; n];
        reader.exact(&mut bytes).await?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)
    })
    .await
    .map_err(|_| Error::Timeout)?
}
/// Read exactly 32 raw secret bytes from an inherited anonymous fd. Five-second
/// deadline; dropping cancels promptly. No environment/argv/logging fallback.
pub async fn token_fd(fd: usize) -> Result<[u8; 32], Error> {
    let mut reader = SecretReader::new(fd)?;
    tokio::time::timeout(crate::auth::HANDSHAKE_TIMEOUT, async {
        let mut bytes = [0; 32];
        reader.exact(&mut bytes).await?;
        Ok(bytes)
    })
    .await
    .map_err(|_| Error::Timeout)?
}
#[cfg(unix)]
struct SecretReader(tokio::io::unix::AsyncFd<std::fs::File>);
#[cfg(unix)]
impl SecretReader {
    fn new(fd: usize) -> Result<Self, Error> {
        use std::os::unix::fs::FileTypeExt;
        let file = crate::unsafe_platform::duplicate_file(fd)?;
        let kind = file.metadata()?.file_type();
        if !kind.is_fifo() && !kind.is_socket() {
            return Err(Error::Authentication);
        }
        crate::unsafe_platform::nonblocking(&file)?;
        Ok(Self(crate::unsafe_platform::register_async(file)?))
    }
    async fn exact(&mut self, mut bytes: &mut [u8]) -> Result<(), Error> {
        while !bytes.is_empty() {
            let mut ready = self.0.readable().await?;
            let read = ready.try_io(|inner| {
                use std::io::Read;
                inner.get_ref().read(bytes)
            });
            match read {
                Ok(Ok(0)) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into()),
                Ok(Ok(n)) => bytes = &mut bytes[n..],
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => continue,
            }
        }
        Ok(())
    }
}
#[cfg(windows)]
struct SecretReader(std::fs::File);
#[cfg(windows)]
impl SecretReader {
    fn new(fd: usize) -> Result<Self, Error> {
        Ok(Self(crate::unsafe_platform::duplicate_file(fd)?))
    }
    async fn exact(&mut self, mut bytes: &mut [u8]) -> Result<(), Error> {
        use std::io::Read;
        while !bytes.is_empty() {
            let available = crate::unsafe_platform::pipe_available(&self.0)? as usize;
            if available == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                continue;
            }
            let n = available.min(bytes.len());
            let read = self.0.read(&mut bytes[..n])?;
            if read == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            bytes = &mut bytes[read..];
        }
        Ok(())
    }
}
/// Kernel identities of a fallback socket and its private directory. A trusted
/// launcher passes this with the secret bootstrap, never via the public path.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EndpointStamp {
    /// Directory device.
    pub directory_device: u64,
    /// Directory inode.
    pub directory_inode: u64,
    /// Socket device.
    pub device: u64,
    /// Socket inode.
    pub inode: u64,
}
/// Single-launch fallback listener. Existing endpoints are never reclaimed.
pub struct PrivateListener {
    listener: Listener,
    path: PathBuf,
    stamp: EndpointStamp,
}
#[cfg(unix)]
fn stamp(path: &Path) -> Result<EndpointStamp, Error> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let directory = path.parent().ok_or(Error::Invalid)?;
    let d = std::fs::symlink_metadata(directory)?;
    let s = std::fs::symlink_metadata(path)?;
    let uid = crate::unsafe_platform::uid();
    if !d.is_dir()
        || d.uid() != uid
        || d.mode() & 0o777 != 0o700
        || !s.file_type().is_socket()
        || s.uid() != uid
        || s.mode() & 0o777 != 0o600
    {
        return Err(Error::Authentication);
    }
    Ok(EndpointStamp {
        directory_device: d.dev(),
        directory_inode: d.ino(),
        device: s.dev(),
        inode: s.ino(),
    })
}
#[cfg(windows)]
fn stamp(_: &Path) -> Result<EndpointStamp, Error> {
    Ok(EndpointStamp {
        directory_device: 0,
        directory_inode: 0,
        device: 0,
        inode: 0,
    })
}
impl EndpointStamp {
    /// Revalidate exact endpoint and private directory identities. Blocking
    /// filesystem metadata only; invoke from a bounded startup worker.
    pub fn check(&self, path: &Path) -> Result<(), Error> {
        let current = stamp(path)?;
        if current.directory_device != self.directory_device
            || current.directory_inode != self.directory_inode
            || current.device != self.device
            || current.inode != self.inode
        {
            return Err(Error::Authentication);
        }
        Ok(())
    }
}
fn name(path: &Path) -> Result<interprocess::local_socket::Name<'_>, Error> {
    #[cfg(unix)]
    {
        Ok(path.to_fs_name::<interprocess::local_socket::GenericFilePath>()?)
    }
    #[cfg(windows)]
    {
        Ok(path
            .as_os_str()
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()?)
    }
}
impl PrivateListener {
    /// Bind exactly once in an existing private 0700 directory on Unix. The
    /// Windows pipe uses owner-only DACL, REJECT_REMOTE_CLIENTS and FIRST_INSTANCE
    /// (interprocess defaults). Unknown/existing endpoints fail closed.
    /// Brief blocking metadata/create calls; use a bounded startup worker.
    pub fn bind(path: &Path) -> Result<Self, Error> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let d = std::fs::symlink_metadata(path.parent().ok_or(Error::Invalid)?)?;
            if !d.is_dir() || d.uid() != crate::unsafe_platform::uid() || d.mode() & 0o777 != 0o700
            {
                return Err(Error::Authentication);
            }
            match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                _ => return Err(Error::Authentication),
            }
        }
        let options = interprocess::local_socket::ListenerOptions::new()
            .name(name(path)?)
            .try_overwrite(false)
            .reclaim_name(false);
        // macOS rejects fchmod on a socket before bind (`Unsupported`), so there the 0o600 mode is
        // applied right after bind. The 0o700 parent directory (checked above, owned by us) keeps
        // every other user out of the endpoint in that window.
        #[cfg(all(unix, not(target_os = "macos")))]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        #[cfg(windows)]
        let options = {
            use interprocess::os::windows::local_socket::ListenerOptionsExt;
            options.security_descriptor(crate::unsafe_platform::pipe_descriptor()?)
        };
        let listener = options.create_tokio()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        let stamp = stamp(path)?;
        Ok(Self {
            listener,
            path: path.into(),
            stamp,
        })
    }
    /// Pin sent through the inherited token descriptor. Pure, nonblocking.
    pub fn stamp(&self) -> EndpointStamp {
        self.stamp.clone()
    }
    /// Accept only one expected process. No timeout by default; launcher owns
    /// the accept deadline. Revalidate path before and after accepting.
    pub async fn accept(&self, expected: &ProcessInstance) -> Result<BoxStream, Error> {
        let pin = self.stamp.clone();
        let path = self.path.clone();
        blocking(move || pin.check(&path)).await?;
        let stream = self.listener.accept().await?;
        let pin = self.stamp.clone();
        let path = self.path.clone();
        let expected = expected.clone();
        blocking(move || {
            pin.check(&path)?;
            check_peer(&stream, &expected)?;
            Ok(Box::new(stream) as BoxStream)
        })
        .await
    }
}
impl Drop for PrivateListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.stamp.check(&self.path).is_ok() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn check_peer(stream: &Stream, expected: &ProcessInstance) -> Result<(), Error> {
    let creds = stream.peer_creds()?;
    #[cfg(unix)]
    let pid = creds.pid().and_then(|pid| u32::try_from(pid).ok());
    #[cfg(windows)]
    let pid = creds.pid();
    if pid.is_some_and(|pid| pid != expected.pid) {
        return Err(Error::Authentication);
    }
    #[cfg(unix)]
    if creds.euid() != Some(crate::unsafe_platform::uid()) {
        return Err(Error::Authentication);
    }
    if ProcessInstance::inspect(expected.pid)? != *expected {
        return Err(Error::Authentication);
    }
    Ok(())
}
/// Connect to a previously pinned fallback endpoint and expected Host instance.
/// Five-second deadline; verifies kernel peer credentials and path before/after.
/// Challenge authentication must follow before any RPC is accepted.
pub async fn connect(
    path: &Path,
    pinned: &EndpointStamp,
    expected: &ProcessInstance,
) -> Result<BoxStream, Error> {
    let pin = pinned.clone();
    let checked_path = path.to_owned();
    blocking(move || pin.check(&checked_path)).await?;
    let stream = tokio::time::timeout(crate::auth::HANDSHAKE_TIMEOUT, Stream::connect(name(path)?))
        .await
        .map_err(|_| Error::Timeout)??;
    let pin = pinned.clone();
    let path = path.to_owned();
    let expected = expected.clone();
    blocking(move || {
        pin.check(&path)?;
        check_peer(&stream, &expected)?;
        Ok(Box::new(stream) as BoxStream)
    })
    .await
}
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::time::timeout(
        crate::auth::HANDSHAKE_TIMEOUT,
        tokio::task::spawn_blocking(work),
    )
    .await
    .map_err(|_| Error::Timeout)?
    .map_err(|_| Error::Authentication)?
}
/// Expected parent for Unix inherited launch, checked independently of wire data.
/// On Windows the inherited capability and launch-secret proof establish it.
pub fn check_launcher(instance: &ProcessInstance) -> Result<(), Error> {
    let parent = crate::unsafe_platform::parent();
    if parent != 0 && parent != instance.pid {
        return Err(Error::Authentication);
    }
    if ProcessInstance::inspect(instance.pid)? != *instance {
        return Err(Error::Authentication);
    }
    Ok(())
}
