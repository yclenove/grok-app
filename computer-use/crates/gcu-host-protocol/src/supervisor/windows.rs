//! Windows launch: one fresh owner pipe per channel, inherited by handle value.
//!
//! The pipe name is random and is not passed to the child. `first_pipe_instance`
//! refuses an existing name instead of attaching to it. Remote clients are
//! rejected. The parent opens the only client, marks that handle inheritable
//! for this spawn, then clears the flag so a later child cannot inherit it.

use std::{io, process::Stdio};
use tokio::net::windows::named_pipe::{ClientOptions, ServerOptions};

use super::{ChildGuard, EngineSession, LaunchRequest};
use crate::supervisor::Error;
use crate::{auth::Secret, server::HostCallbacks};
use std::sync::Arc;

pub(super) async fn spawn_pipe(
    request: &LaunchRequest,
    callbacks: Arc<dyn HostCallbacks>,
) -> Result<EngineSession, Error> {
    let rpc_name = pipe_name("rpc");
    let stop_name = pipe_name("stop");
    let rpc_server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .create(&rpc_name)?;
    let stop_server = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .create(&stop_name)?;
    let rpc_client = ClientOptions::new().open(&rpc_name)?;
    let stop_client = ClientOptions::new().open(&stop_name)?;
    rpc_server.connect().await?;
    stop_server.connect().await?;
    let rpc_raw = raw(&rpc_client);
    let stop_raw = raw(&stop_client);
    let _rpc_inherit = Inherit::acquire(rpc_raw)?;
    let _stop_inherit = Inherit::acquire(stop_raw)?;
    let mut command = std::process::Command::new(&request.path);
    command.arg("engine");
    command.args(&request.extra_args);
    command.arg("--host-fd").arg(rpc_raw.to_string());
    command.arg("--stop-fd").arg(stop_raw.to_string());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let guard = ChildGuard(Some(command.spawn()?));
    drop(_rpc_inherit);
    drop(_stop_inherit);
    drop(rpc_client);
    drop(stop_client);
    super::finish(rpc_server, stop_server, guard, request, callbacks).await
}

pub(super) fn privilege(pid: Option<u32>) -> super::LivePrivilege {
    let Some(pid) = pid else {
        return super::LivePrivilege::unknown();
    };
    match token_flags(pid) {
        Ok((elevated, ui_access)) => super::LivePrivilege {
            elevated,
            ui_access,
            unknown: false,
        },
        Err(_) => super::LivePrivilege::unknown(),
    }
}

fn pipe_name(kind: &str) -> String {
    format!(r"\\.\pipe\gcu-host-{kind}-{}", Secret::random().hex())
}

fn raw(client: &impl std::os::windows::io::AsRawHandle) -> usize {
    client.as_raw_handle() as usize
}

struct Inherit(std::os::windows::io::RawHandle);

impl Inherit {
    fn acquire(handle: usize) -> io::Result<Self> {
        set_inherit(handle, true)?;
        Ok(Self(handle as std::os::windows::io::RawHandle))
    }
}

impl Drop for Inherit {
    fn drop(&mut self) {
        let _ = set_inherit(self.0 as usize, false);
    }
}

fn set_inherit(handle: usize, inherit: bool) -> io::Result<()> {
    use windows::Win32::Foundation::{
        SetHandleInformation, HANDLE, HANDLE_FLAGS, HANDLE_FLAG_INHERIT,
    };
    let flags = if inherit {
        HANDLE_FLAG_INHERIT
    } else {
        HANDLE_FLAGS(0)
    };
    // SAFETY: the handle is a pipe the parent still owns. The call only changes
    // the inherit bit so this spawn can pass it, and the drop clears that bit.
    unsafe {
        SetHandleInformation(HANDLE(handle as *mut _), HANDLE_FLAG_INHERIT.0, flags)
            .map_err(io::Error::other)
    }
}

fn token_flags(pid: u32) -> io::Result<(bool, bool)> {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{TokenElevation, TokenUIAccess, TOKEN_QUERY},
        System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    // SAFETY: OpenProcess validates the pid and returns an owned query handle.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(io::Error::other)?;
    let result = (|| {
        let mut token = HANDLE::default();
        // SAFETY: process is a live query handle; token is a writable output.
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(io::Error::other)?;
        let elevated = dword(token, TokenElevation)?;
        let ui_access = dword(token, TokenUIAccess)?;
        // SAFETY: token is the owned query handle opened above and is not used again.
        let _ = unsafe { CloseHandle(token) };
        Ok((elevated != 0, ui_access != 0))
    })();
    // SAFETY: process is the owned handle opened above and is not used again.
    let _ = unsafe { CloseHandle(process) };
    result
}

fn dword(
    token: windows::Win32::Foundation::HANDLE,
    class: windows::Win32::Security::TOKEN_INFORMATION_CLASS,
) -> io::Result<u32> {
    use windows::Win32::Security::GetTokenInformation;
    let mut value = 0u32;
    let mut returned = 0u32;
    // SAFETY: token is a live query handle. The output is a 4-byte aligned
    // DWORD, which is the documented size of TokenElevation and TokenUIAccess.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some((&mut value as *mut u32).cast()),
            4,
            &mut returned,
        )
    }
    .map_err(io::Error::other)?;
    Ok(value)
}
