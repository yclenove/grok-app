//! macOS audit-token signatures and the real helper process.
//!
//! Peer identity comes from `LOCAL_PEERTOKEN` plus `SecCodeCopyGuestWithAttributes`.
//! A path-based signature check is not used. `getsockopt` failure fails closed.

use super::auth::{
    current_bundle_id, launch_command, valid_idle_secs, CodeSignature, HANDSHAKE_TIMEOUT,
    IDLE_LIMIT,
};
use super::client::{client_attach, ClientAttach, ClientConfig, Sigterm};
use super::error::HelperError;
use super::frame::WireProfile;
use super::proxy::{
    start_listener, AuditSource, ListenConfig, ListenStop, PeerObservation, SessionRegistry,
};
use crate::exit::Failure;
use async_trait::async_trait;
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::data::{CFData, CFDataRef};
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::string::{CFString, CFStringRef};
use gcu_core::driver::Driver;
use security_framework_sys::code_signing::{
    kSecGuestAttributeAudit, SecCodeCopyGuestWithAttributes, SecCodeCopySelf, SecCodeRef,
};
use std::ffi::c_void;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const SOL_LOCAL: i32 = 0;
const LOCAL_PEERTOKEN: i32 = 0x006;
const ERR_SEC_SUCCESS: i32 = 0;
const SIGNING_INFORMATION: u32 = 1 << 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct AuditToken {
    val: [u32; 8],
}

/// Audit-token signature source. Self identity is read once.
pub struct MacAudit {
    local: CodeSignature,
}

impl MacAudit {
    /// Read this process's code signature.
    ///
    /// The Security.framework call runs on a blocking thread and is capped at
    /// [`HANDSHAKE_TIMEOUT`]. Dropping the future cancels the wait. Unsigned
    /// or unreadable self identity is [`HelperError::UnsignedSelf`].
    pub async fn load() -> Result<Self, HelperError> {
        let signature = read_self().await?;
        Ok(Self { local: signature })
    }
}

#[async_trait]
impl AuditSource for MacAudit {
    async fn peer(&self, stream: &tokio::net::UnixStream) -> Result<PeerObservation, HelperError> {
        let token = peer_token(stream)?;
        let uid = euid_of(token);
        let pid = pid_of(token);
        let signature = signature_of_token(token).await;
        Ok(PeerObservation {
            uid,
            pid,
            signature,
        })
    }

    async fn local(&self) -> Result<CodeSignature, HelperError> {
        Ok(self.local.clone())
    }
}

fn peer_token(stream: &tokio::net::UnixStream) -> Result<AuditToken, HelperError> {
    let mut token = AuditToken { val: [0; 8] };
    let mut len = std::mem::size_of::<AuditToken>() as libc::socklen_t;
    // SAFETY: `fd` is a live socket. `token` and `len` are writable for the
    // duration of the call. `SOL_LOCAL` / `LOCAL_PEERTOKEN` write an
    // `audit_token_t` and do not retain the pointer.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            SOL_LOCAL,
            LOCAL_PEERTOKEN,
            &mut token as *mut AuditToken as *mut c_void,
            &mut len,
        )
    };
    if rc != 0 || len as usize != std::mem::size_of::<AuditToken>() {
        return Err(HelperError::Audit);
    }
    Ok(token)
}

fn euid_of(token: AuditToken) -> u32 {
    // SAFETY: `token` is a by-value audit token copied from getsockopt. libbsm
    // reads those 32 bytes and returns the effective uid. It does not retain them.
    unsafe { audit_token_to_euid(token) }
}

fn pid_of(token: AuditToken) -> u32 {
    // SAFETY: same token as [`euid_of`]. The call returns a pid and retains nothing.
    let pid = unsafe { audit_token_to_pid(token) };
    u32::try_from(pid).unwrap_or(0)
}

async fn read_self() -> Result<CodeSignature, HelperError> {
    let joined = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio::task::spawn_blocking(signature_of_self),
    )
    .await
    .map_err(|_| HelperError::Timeout)?
    .map_err(|_| HelperError::Io("join"))?;
    joined.ok_or(HelperError::UnsignedSelf)
}

async fn signature_of_token(token: AuditToken) -> Option<CodeSignature> {
    let joined = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        tokio::task::spawn_blocking(move || signature_of_guest(token)),
    )
    .await;
    match joined {
        Ok(Ok(signature)) => signature,
        Ok(Err(_)) | Err(_) => None,
    }
}

fn signature_of_self() -> Option<CodeSignature> {
    unsafe {
        let mut code: SecCodeRef = std::ptr::null_mut();
        if SecCodeCopySelf(0, &mut code) != ERR_SEC_SUCCESS || code.is_null() {
            if !code.is_null() {
                core_foundation::base::CFRelease(code as CFTypeRef);
            }
            return None;
        }
        let signature = copy_signature(code);
        core_foundation::base::CFRelease(code as CFTypeRef);
        signature
    }
}

fn signature_of_guest(token: AuditToken) -> Option<CodeSignature> {
    let bytes = token_bytes(&token);
    let data = CFData::from_buffer(&bytes);
    // SAFETY: `kSecGuestAttributeAudit` is a process-lifetime CFString constant.
    // wrap_under_get_rule retains it; Drop releases that retain.
    let key = unsafe { CFString::wrap_under_get_rule(kSecGuestAttributeAudit) };
    let attributes = core_foundation::dictionary::CFDictionary::from_CFType_pairs(&[(key, data)]);
    unsafe {
        let mut guest: SecCodeRef = std::ptr::null_mut();
        let status = SecCodeCopyGuestWithAttributes(
            std::ptr::null_mut(),
            attributes.as_concrete_TypeRef(),
            0,
            &mut guest,
        );
        if status != ERR_SEC_SUCCESS || guest.is_null() {
            if !guest.is_null() {
                core_foundation::base::CFRelease(guest as CFTypeRef);
            }
            return None;
        }
        let signature = copy_signature(guest);
        core_foundation::base::CFRelease(guest as CFTypeRef);
        signature
    }
}

fn token_bytes(token: &AuditToken) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    for (index, value) in token.val.iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// # Safety
/// `code` is a +1 `SecCodeRef`. The function does not release it.
unsafe fn copy_signature(code: SecCodeRef) -> Option<CodeSignature> {
    let mut dict: CFDictionaryRef = std::ptr::null_mut();
    // SAFETY: caller guarantees `code` is live. On success the dictionary is +1
    // and released below. The flag requests signing information only.
    let status =
        unsafe { SecCodeCopySigningInformation(code as _, SIGNING_INFORMATION, &mut dict) };
    if status != ERR_SEC_SUCCESS || dict.is_null() {
        if !dict.is_null() {
            unsafe { core_foundation::base::CFRelease(dict as CFTypeRef) };
        }
        return None;
    }
    let identifier = dict_string(dict, unsafe { kSecCodeInfoIdentifier });
    let team = dict_string(dict, unsafe { kSecCodeInfoTeamIdentifier });
    let cdhash = cdhash_of(dict);
    unsafe { core_foundation::base::CFRelease(dict as CFTypeRef) };
    let identifier = identifier?;
    let cdhash = cdhash?;
    CodeSignature::new(&identifier, team.as_deref(), &cdhash)
}

fn cdhash_of(dict: CFDictionaryRef) -> Option<String> {
    let hashes = dict_type(dict, unsafe { kSecCodeInfoCdHashes });
    if let Some(hash) = first_data_hex(hashes) {
        return Some(hash);
    }
    data_hex(dict_type(dict, unsafe { kSecCodeInfoUnique }))
}

fn dict_string(dict: CFDictionaryRef, key: CFStringRef) -> Option<String> {
    let value = dict_type(dict, key)?;
    if !value.instance_of::<CFString>() {
        return None;
    }
    // SAFETY: the type id matches CFString. The wrapper retains under the get rule.
    let text = unsafe { CFString::wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef) };
    let text = text.to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn dict_type(dict: CFDictionaryRef, key: CFStringRef) -> Option<CFType> {
    // SAFETY: `dict` is a live dictionary and `key` is a live CFString.
    // The returned value is +0; wrap_under_get_rule retains it.
    let value = unsafe { CFDictionaryGetValue(dict, key as *const c_void) };
    if value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_get_rule(value as CFTypeRef) })
    }
}

fn first_data_hex(value: Option<CFType>) -> Option<String> {
    let value = value?;
    if !value.instance_of::<CFArray>() {
        return None;
    }
    // SAFETY: the type id matches CFArray.
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
    // SAFETY: the type id matches CFData.
    let data = unsafe { CFData::wrap_under_get_rule(value.as_CFTypeRef() as CFDataRef) };
    let bytes = data.bytes();
    if bytes.is_empty() {
        None
    } else {
        Some(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

#[link(name = "Security", kind = "framework")]
extern "C" {
    static kSecCodeInfoTeamIdentifier: CFStringRef;
    static kSecCodeInfoIdentifier: CFStringRef;
    static kSecCodeInfoCdHashes: CFStringRef;
    static kSecCodeInfoUnique: CFStringRef;

    fn SecCodeCopySigningInformation(
        code: security_framework_sys::code_signing::SecStaticCodeRef,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryGetValue(dict: CFDictionaryRef, key: *const c_void) -> *const c_void;
}

#[link(name = "bsm")]
extern "C" {
    fn audit_token_to_euid(token: AuditToken) -> u32;
    fn audit_token_to_pid(token: AuditToken) -> i32;
}

pub(crate) async fn run_agent(
    scene: &str,
    fake: bool,
    mode: Option<String>,
) -> Result<u8, Failure> {
    let audit: Arc<dyn AuditSource> =
        Arc::new(MacAudit::load().await.map_err(super::helper_failure)?);
    let uid = current_uid();
    let (host, idle_ms) = load_host(mode).await?;
    let registry = Arc::new(SessionRegistry::new());
    let (driver, control) = if fake {
        (Arc::new(fake_driver(scene)?) as Arc<dyn Driver>, None)
    } else {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let latch = Arc::new(crate::host::macos_wire::ActivityLatch::new(
            host.activity.clone(),
            tx,
        ));
        let native = Arc::new(
            gcu_driver_macos::MacosDriver::with_stop_latch(
                latch,
                gcu_driver_macos::input::hotkey::HotkeyConfig::default(),
                idle_ms,
            )
            .await
            .map_err(|_| Failure::internal("Cannot start the macOS driver"))?,
        );
        host.activity
            .set_physical(Arc::new(crate::host::macos_wire::MacosActivity::new(
                &native,
            )));
        let driver = Arc::clone(&native) as Arc<dyn Driver>;
        let connection = gcu_mcp::Connection::open_host_only(
            Arc::clone(&driver),
            host.host.clone(),
            gcu_mcp::ServerConfig::default(),
        )
        .await
        .map_err(|_| Failure::internal("Cannot open helper control"))?;
        let control_session = connection.session();
        let control = crate::resume::Control::start_with_lifecycle(
            Arc::clone(&control_session),
            host.activity.clone(),
            connection.lifecycle(),
        )
        .await?;
        let release_driver = Arc::clone(&driver);
        let release_activity = host.activity.clone();
        let release_registry = Arc::clone(&registry);
        let release = tokio::spawn(async move {
            let mut rx = rx;
            while rx.recv().await.is_some() {
                let releasing = Arc::clone(&release_driver);
                let release_task = tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(500),
                        releasing.release_all_input(),
                    )
                    .await;
                });
                let mut sessions = release_registry.sessions();
                sessions.push(Arc::clone(&control_session));
                for session in sessions {
                    tokio::spawn(async move {
                        session.handle_stop().await;
                    });
                }
                let persisting = release_activity.clone();
                let persist = tokio::task::spawn_blocking(move || persisting.persist_stop());
                let _ = release_task.await;
                let _ = tokio::time::timeout(Duration::from_secs(5), persist).await;
            }
        });
        (driver, Some((control, connection, release)))
    };
    let running = start_listener(ListenConfig {
        path: socket_path(uid),
        audit,
        version: env!("CARGO_PKG_VERSION").to_string(),
        uid,
        host: host.host,
        driver,
        registry,
        handshake_timeout: HANDSHAKE_TIMEOUT,
        idle_limit: IDLE_LIMIT,
    })
    .await
    .map_err(super::helper_failure)?;
    let result = match running.done.await {
        Ok(ListenStop::Idle | ListenStop::Shutdown | ListenStop::Stopped) => Ok(0),
        Ok(ListenStop::Replaced) => Err(Failure::arguments(
            "Computer-use helper socket was replaced",
        )),
        Ok(ListenStop::Failed) | Err(_) => Err(Failure::internal("Computer-use helper failed")),
    };
    if let Some((control, connection, release)) = control {
        release.abort();
        drop(control);
        drop(connection);
    }
    result
}

async fn load_host(mode: Option<String>) -> Result<(crate::host::LocalHost, u64), Failure> {
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(move || {
            let config = crate::config::load()?;
            let idle_ms = config.user_idle_ms;
            crate::host::LocalHost::new(config, mode).map(|host| (host, idle_ms))
        }),
    )
    .await
    .map_err(|_| Failure::internal("Host configuration IO timed out"))?
    .map_err(|_| Failure::internal("Host configuration failed"))?
    .map_err(|_| Failure::internal("Cannot load host configuration"))
}

pub(crate) async fn run_stdio_proxy(
    profile: gcu_mcp::Profile,
    turn_idle_secs: u64,
) -> Result<u8, Failure> {
    if !valid_idle_secs(turn_idle_secs) {
        return Err(Failure::arguments(
            "Turn idle is outside the supported range",
        ));
    }
    let audit: Arc<dyn AuditSource> =
        Arc::new(MacAudit::load().await.map_err(super::helper_failure)?);
    let uid = current_uid();
    let path = socket_path(uid);
    let control = Arc::new(Sigterm);
    for _ in 0..3 {
        let config = ClientConfig {
            path: path.clone(),
            audit: Arc::clone(&audit),
            uid,
            version: env!("CARGO_PKG_VERSION").to_string(),
            profile: WireProfile::from(profile),
            turn_idle_secs,
            control: Arc::clone(&control) as Arc<dyn super::client::ProcessControl>,
            handshake_timeout: HANDSHAKE_TIMEOUT,
        };
        match client_attach(config).await {
            Ok(ClientAttach::Ready(stream)) => return bridge_stdio(stream).await,
            Ok(ClientAttach::Replaced { .. }) | Err(HelperError::Absent | HelperError::Version) => {
                launch_helper().await?;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(error) => return Err(super::helper_failure(error)),
        }
    }
    Err(Failure::arguments(
        "Computer-use helper did not accept the connection",
    ))
}

async fn bridge_stdio(stream: tokio::net::UnixStream) -> Result<u8, Failure> {
    let (socket_read, socket_write) = stream.into_split();
    super::serve::bridge_ndjson(
        socket_read,
        socket_write,
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await
    .map(|()| 0)
    .map_err(super::helper_failure)
}

pub(crate) async fn launch_helper() -> Result<(), Failure> {
    let bundle = current_bundle_id();
    let argv = launch_command(bundle)
        .ok_or_else(|| Failure::internal("Computer-use helper bundle id is not launchable"))?;
    let mut command = tokio::process::Command::new(&argv[0]);
    command.args(&argv[1..]);
    let status = tokio::time::timeout(HANDSHAKE_TIMEOUT, command.status())
        .await
        .map_err(|_| Failure::internal("Launching the computer-use helper timed out"))?
        .map_err(|_| Failure::internal("Cannot launch the computer-use helper"))?;
    if status.success() {
        Ok(())
    } else {
        Err(Failure::arguments(
            "Launch Services did not start the computer-use helper",
        ))
    }
}

fn fake_driver(scene: &str) -> Result<gcu_driver_fake::FakeDriver, Failure> {
    gcu_driver_fake::FakeDriver::from_scene(scene)
        .and_then(|driver| {
            driver.with_image_digest([0x47; 32]).map_err(|_| {
                gcu_driver_fake::scene::SceneError::Invalid("fake identity unavailable")
            })
        })
        .map_err(|_| Failure::arguments("Unknown or invalid fake scene"))
}

fn socket_path(uid: u32) -> PathBuf {
    super::identity::ordinary_socket(&std::env::temp_dir(), uid)
}

fn current_uid() -> u32 {
    // SAFETY: geteuid reads the process credential and takes no pointer.
    // On this target `uid_t` is `u32`.
    unsafe { libc::geteuid() }
}
