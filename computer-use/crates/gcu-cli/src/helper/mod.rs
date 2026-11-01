//! macOS helper and the stdio proxy in front of it.
//!
//! Windows, Linux, and `--driver fake` keep `gcu mcp` in process. A real macOS
//! stdio session is forwarded to the helper app, which is the process that
//! calls the TCC-gated APIs. `gcu agent` is that helper. Launch Services must
//! start it with `open`; executing the binary from a shell does not receive
//! the helper's permission grant.

pub mod auth;
#[cfg(unix)]
pub mod client;
mod error;
#[cfg(unix)]
pub mod frame;
pub(crate) mod identity;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
pub mod proxy;
mod scope_link;
#[cfg(unix)]
mod serve;

use crate::exit::Failure;
pub use error::HelperError;
pub(crate) use scope_link::run_scoped_agent;

/// Run `gcu agent`.
///
/// On macOS this binds the helper socket and serves admitted peers until idle
/// or shutdown. Other systems refuse before any socket is created. Blocking
/// waits are capped at five seconds each. Dropping the future stops the
/// listener. No payload is printed.
pub(crate) async fn run_agent(scene: &str, fake: bool) -> Result<u8, Failure> {
    let mode = crate::args::approval_mode()?;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (scene, fake, mode);
        Err(helper_failure(HelperError::Unsupported))
    }
    #[cfg(target_os = "macos")]
    {
        macos::run_agent(scene, fake, mode).await
    }
}

/// Forward stdio MCP to the helper.
///
/// macOS only. Selection already chose the helper proxy. Up to
/// three attach attempts are made. A replaced helper is asked to exit and
/// launched again with `open`. Blocking calls cap at five seconds. Dropping
/// the future closes the socket.
pub(crate) async fn run_stdio_proxy(
    profile: gcu_mcp::Profile,
    turn_idle_secs: u64,
) -> Result<u8, Failure> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (profile, turn_idle_secs);
        Err(helper_failure(HelperError::Unsupported))
    }
    #[cfg(target_os = "macos")]
    {
        macos::run_stdio_proxy(profile, turn_idle_secs).await
    }
}

/// Start the helper with Launch Services and return.
///
/// The terminal does not probe TCC: the helper's own grant is what matters,
/// and `open` does not return the helper's exit status. macOS only. Blocking
/// waits are capped at five seconds. Cancellation: dropping the future does
/// not stop an `open` that Launch Services already accepted.
#[cfg(target_os = "macos")]
pub(crate) async fn launch_for_onboarding() -> Result<(), Failure> {
    macos::launch_helper().await
}

pub(crate) fn helper_failure(error: HelperError) -> Failure {
    match error {
        HelperError::Unsupported => {
            Failure::arguments("The computer-use helper runs only on macOS")
        }
        HelperError::Refused(_) | HelperError::Foreign | HelperError::UnsignedSelf => {
            Failure::arguments("Computer-use helper rejected the connection")
        }
        HelperError::Busy => Failure::arguments("A computer-use helper is already running"),
        HelperError::Replaced | HelperError::Version | HelperError::Absent => {
            Failure::arguments("Computer-use helper is unavailable")
        }
        HelperError::Timeout | HelperError::Audit | HelperError::Frame | HelperError::Io(_) => {
            Failure::internal("Computer-use helper failed")
        }
    }
}
