//! Identity of the existing resume endpoint, shared with non-resuming hooks.
use super::{check_dir, check_file, unsafe_platform, write_secret};
use crate::exit::Failure;
use std::{io, path::Path};

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pid: u32,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    pub(crate) name: String,
}
pub(crate) fn publish(directory: &Path, name: Option<String>) -> Result<(), Failure> {
    #[cfg(unix)]
    let endpoint = {
        let _ = name;
        let (device, inode) = socket_identity(directory)?;
        Endpoint {
            pid: std::process::id(),
            device,
            inode,
        }
    };
    #[cfg(windows)]
    let endpoint = Endpoint {
        pid: std::process::id(),
        name: name.ok_or_else(|| Failure::internal("Missing control pipe name"))?,
    };
    let bytes = serde_json::to_vec(&endpoint)
        .map_err(|_| Failure::internal("Cannot encode control endpoint identity"))?;
    write_secret(&directory.join("endpoint-id"), &bytes)
        .map_err(|_| Failure::internal("Cannot publish control endpoint identity"))
}
pub(crate) fn inspect(directory: &Path) -> Result<Endpoint, Failure> {
    check_dir(directory)?;
    let path = directory.join("endpoint-id");
    check_file(&path)?;
    let bytes = std::fs::read(&path)
        .map_err(|_| Failure::arguments("Cannot read control endpoint identity"))?;
    if bytes.len() > 1024 {
        return Err(Failure::arguments("Invalid control endpoint identity"));
    }
    let endpoint: Endpoint = serde_json::from_slice(&bytes)
        .map_err(|_| Failure::arguments("Invalid control endpoint identity"))?;
    #[cfg(unix)]
    if socket_identity(directory)? != (endpoint.device, endpoint.inode) {
        return Err(Failure::arguments("Private control endpoint was replaced"));
    }
    #[cfg(windows)]
    {
        let path = directory.join("endpoint");
        check_file(&path)?;
        let name = std::fs::read_to_string(&path)
            .map_err(|_| Failure::arguments("Cannot read control pipe name"))?;
        if name != endpoint.name || !name.starts_with(r"\\.\pipe\gcu-") {
            return Err(Failure::arguments("Private control endpoint was replaced"));
        }
    }
    Ok(endpoint)
}
pub(crate) fn recheck(directory: &Path, expected: &Endpoint) -> Result<(), Failure> {
    if inspect(directory)? != *expected {
        return Err(Failure::arguments(
            "Private control endpoint changed during connection",
        ));
    }
    Ok(())
}
#[cfg(unix)]
fn socket_identity(directory: &Path) -> Result<(u64, u64), Failure> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let path = directory.join("control.sock");
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| Failure::arguments("Private control socket is unavailable"))?;
    if !metadata.file_type().is_socket() {
        return Err(Failure::arguments(
            "Private control endpoint is not a socket",
        ));
    }
    unsafe_platform::check(&path, &metadata, false).map_err(|_| {
        Failure::arguments("Private control socket owner or permissions are unsafe")
    })?;
    Ok((metadata.dev(), metadata.ino()))
}
#[cfg(unix)]
pub(crate) fn check_peer(stream: &tokio::net::UnixStream, endpoint: &Endpoint) -> io::Result<()> {
    let peer = stream.peer_cred()?;
    if peer.pid().is_some_and(|pid| pid as u32 != endpoint.pid) {
        return Err(io::Error::other(
            "Private control server process was replaced",
        ));
    }
    Ok(())
}
#[cfg(windows)]
pub(crate) fn check_peer(
    stream: &tokio::net::windows::named_pipe::NamedPipeClient,
    endpoint: &Endpoint,
) -> io::Result<()> {
    unsafe_platform::check_pipe_peer(stream, endpoint.pid)
}
