use crate::grant::{parse_stream, Dict};
use crate::input::{InputGate, InputPort, InputWorker, Request as InputRequest};
use crate::transport::Transports;
use crate::{capture::CaptureWorker, frame::FrameStore, CapturedFrame};
use crate::{InputAction, InputState, InputSubmission};
use crate::{ObservedFrame, PresentedFrame};
use crate::{PortalGrant, SourceKind};
use futures_util::StreamExt;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle, time::Instant};
use zbus::{
    message::Type,
    zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Str},
    Connection, MatchRule, MessageStream, Proxy,
};

pub(crate) const SERVICE: &str = "org.freedesktop.portal.Desktop";
pub(crate) const ROOT: &str = "/org/freedesktop/portal/desktop";
pub(crate) const REMOTE: &str = "org.freedesktop.portal.RemoteDesktop";
pub(crate) const SCREEN: &str = "org.freedesktop.portal.ScreenCast";
pub(crate) const SESSION: &str = "org.freedesktop.portal.Session";
pub(crate) const REQUEST: &str = "org.freedesktop.portal.Request";
const CLEANUP_LIMIT: Duration = Duration::from_secs(1);

pub struct PortalOptions {
    pub run_id: String,
    /// Exported native Wayland parent handle, or empty for an unparented dialog.
    pub parent_window: String,
    pub source: SourceKind,
    /// Bounds the whole negotiation, not each successive method independently.
    pub timeout: Duration,
    /// Lifetime of a one-use observation, measured from issue, not pixel delivery.
    /// Default 60s; the Host may choose a shorter/longer model budget up to 300s.
    /// This is not a guarantee of image freshness or compositor responsiveness.
    pub observation_timeout: Duration,
}

impl PortalOptions {
    pub fn new(run_id: String, source: SourceKind) -> Self {
        Self {
            run_id,
            parent_window: String::new(),
            source,
            timeout: Duration::from_secs(90),
            observation_timeout: crate::frame::DEFAULT_OBSERVATION_TIMEOUT,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.run_id.is_empty() || self.run_id.len() > 256 || self.run_id.contains('\0') {
            return Err("a bounded nonempty run identity is required".into());
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(300) {
            return Err("portal negotiation timeout must be in (0, 300s]".into());
        }
        if self.observation_timeout.is_zero() || self.observation_timeout > Duration::from_secs(300)
        {
            return Err("observation timeout must be in (0, 300s]".into());
        }
        if self.parent_window.len() > 4096
            || self.parent_window.contains('\0')
            || (!self.parent_window.is_empty()
                && (self
                    .parent_window
                    .strip_prefix("wayland:")
                    .unwrap_or("")
                    .is_empty()))
        {
            return Err("expected an exported Wayland parent handle".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Stopped,
    PortalClosed,
    PortalOwnerLost,
    BusDisconnected,
    TransportClosed(&'static str),
    TimedOut,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Granted(PortalGrant),
    /// Authorization is gone, but native resources are still being retired.
    Closing(CloseReason),
    /// Native capture/input joined; descriptors and private portal connection closed.
    Closed(CloseReason),
}

/// Run-owned portal connection. Dropping a pending/active handle requests
/// teardown; it does not abort the owner task or prematurely declare it idle.
/// Native consumers stay inside this owner; no FD is exposed to callers, and
/// no Notify* fallback is used after EIS. Granted means portal consent, not EI
/// readiness; `frame()` returns None until real capture produces pixels.
pub struct PortalSession {
    cancel: watch::Sender<bool>,
    state: watch::Receiver<SessionState>,
    owner: Option<JoinHandle<()>>,
    frames: Arc<FrameStore>,
    input: InputPort,
}

/// Read/revoke authority only. This cannot send input, issue observations, or
/// certify completion; only awaiting the original PortalSession owner can.
#[derive(Clone)]
pub(crate) struct SessionControl {
    cancel: watch::Sender<bool>,
    state: watch::Receiver<SessionState>,
    frames: Arc<FrameStore>,
    gate: Arc<InputGate>,
}
impl SessionControl {
    pub(crate) fn subscribe(&self) -> watch::Receiver<SessionState> {
        self.state.clone()
    }
    pub(crate) fn state(&self) -> SessionState {
        self.state.borrow().clone()
    }
    pub(crate) fn geometry_revision(&self) -> u64 {
        self.frames.geometry_revision()
    }
    pub(crate) fn request_stop(&self) {
        self.gate.revoke();
        self.frames.revoke();
        let _ = self.cancel.send(true);
    }
}

struct Consumers {
    frames: Arc<FrameStore>,
    capture: bool,
    input: Option<(Arc<InputGate>, std::sync::mpsc::Receiver<InputRequest>)>,
    gate: Arc<InputGate>,
}
impl Drop for Consumers {
    fn drop(&mut self) {
        self.gate.revoke();
        self.frames.revoke();
    }
}

impl PortalSession {
    pub(crate) fn control(&self) -> SessionControl {
        SessionControl {
            cancel: self.cancel.clone(),
            state: self.state.clone(),
            frames: self.frames.clone(),
            gate: self.input.gate.clone(),
        }
    }
    pub fn start(options: PortalOptions) -> Result<Self, String> {
        Self::spawn(options, None, true, true)
    }

    fn spawn(
        options: PortalOptions,
        address: Option<String>,
        capture: bool,
        input_enabled: bool,
    ) -> Result<Self, String> {
        options.validate()?;
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "Wayland portal needs the Host asynchronous runtime")?;
        let (cancel, cancellation) = watch::channel(false);
        let (publish, state) = watch::channel(SessionState::Starting);
        let frames = Arc::new(FrameStore::new(options.observation_timeout));
        let (input, receive) = InputPort::new();
        let owner = runtime.spawn(supervise(
            options,
            address,
            cancellation,
            publish,
            Consumers {
                frames: frames.clone(),
                capture,
                input: input_enabled.then(|| (input.gate.clone(), receive)),
                gate: input.gate.clone(),
            },
        ));
        Ok(Self {
            cancel,
            state,
            owner: Some(owner),
            frames,
            input,
        })
    }

    pub fn state(&self) -> SessionState {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<SessionState> {
        self.state.clone()
    }

    pub fn input_state(&self) -> InputState {
        self.input.gate.state()
    }

    /// Native EI logical-coordinate/evdev primitive, not a screenshot action or
    /// Unicode typing API. Submission does not certify application effects.
    pub async fn input(
        &self,
        generation: u64,
        action: InputAction,
    ) -> Result<InputSubmission, String> {
        if !matches!(self.state(), SessionState::Granted(_)) {
            return Err("portal input is not granted".into());
        }
        self.input.submit(generation, action).await
    }

    /// Last valid delivered image, with its original delivery timestamp. Static
    /// sources need not emit periodic images; this does not attest freshness.
    /// Returns None before capture/after invalidation; revoked runs return errors.
    pub fn frame(&self) -> Result<Option<CapturedFrame>, String> {
        if !matches!(self.state(), SessionState::Granted(_)) {
            return Err("portal capture is not granted".into());
        }
        self.frames.latest()
    }

    /// Capture a non-forgeable, one-use binding to both native owners. A Host
    /// must still validate its own intent/permission/target scope before input.
    /// A new observation supersedes the previous one, even for identical pixels.
    /// Its bounded lifetime starts now; it never rewrites pixel delivery time.
    pub fn observe(&self) -> Result<Option<ObservedFrame>, String> {
        let SessionState::Granted(grant) = self.state() else {
            return Err("portal observation is not granted".into());
        };
        ObservedFrame::capture(&self.frames, &self.input.gate, &grant)
    }

    /// Low-level EI action bound to an observed frame, revalidated on the native
    /// owner immediately before submission, not merely before enqueueing. These
    /// remain EI logical coordinates, NOT screenshot-pixel coordinates.
    pub async fn input_observed(
        &self,
        observation: ObservedFrame,
        action: InputAction,
    ) -> Result<InputSubmission, String> {
        if !matches!(self.state(), SessionState::Granted(_))
            || !observation.belongs_to(&self.frames, &self.input.gate)
        {
            return Err("observation belongs to another or revoked session".into());
        }
        self.input
            .submit_bound(
                observation.input_capabilities().generation,
                action,
                Some(observation.permit),
            )
            .await
    }

    /// Move to the centre of a pixel in the exact presented screenshot. Pixel
    /// indices are zero-based and half-open; no clamping or implicit rounding.
    /// This is a motion submission, NOT a click or application-effect receipt.
    /// Owner/geometry/generation/observation deadline checks also run at dispatch.
    pub async fn move_to_pixel(
        &self,
        presented: PresentedFrame,
        x: u32,
        y: u32,
    ) -> Result<InputSubmission, String> {
        let (observation, action) = presented.into_pixel_input(x, y)?;
        self.input_observed(observation, action).await
    }

    /// One observation authorizes one bounded neutral-to-neutral sequence, not
    /// independent replayable events. Native preflight rejects the entire plan
    /// before any event when capabilities, held state or coordinates disagree.
    pub async fn input_sequence_observed(
        &self,
        observation: ObservedFrame,
        actions: Vec<InputAction>,
        cancellation: grok_computer_use_core::execution::ActionCancellation,
    ) -> Result<InputSubmission, String> {
        crate::sequence::validate_balanced(&actions)?;
        if !matches!(self.state(), SessionState::Granted(_))
            || !observation.belongs_to(&self.frames, &self.input.gate)
        {
            return Err("observation belongs to another or revoked session".into());
        }
        self.input
            .submit_actions_bound(
                observation.input_capabilities().generation,
                actions,
                Some(observation.permit),
                cancellation,
            )
            .await
    }

    /// Validate and consume authority for a known no-op (e.g. zero scroll).
    /// Same lock order/lifetime rules as native input, but never enqueue FFI.
    pub(crate) fn consume_observation(
        &self,
        observation: ObservedFrame,
        cancellation: &grok_computer_use_core::execution::ActionCancellation,
    ) -> Result<(), String> {
        cancellation.check()?;
        if !matches!(self.state(), SessionState::Granted(_))
            || !observation.belongs_to(&self.frames, &self.input.gate)
        {
            return Err("observation belongs to another or revoked session".into());
        }
        let state = self
            .input
            .gate
            .0
            .lock()
            .map_err(|_| "EI authority lock poisoned")?;
        if !matches!(*state, InputState::Ready(ref c) if c.generation == observation.input_capabilities().generation)
        {
            return Err("EI generation changed before no-op validation".into());
        }
        observation.permit.dispatch(|| cancellation.check())
    }

    pub async fn ready(&mut self) -> Result<PortalGrant, CloseReason> {
        loop {
            match self.state() {
                SessionState::Granted(grant) => return Ok(grant),
                SessionState::Closing(reason) | SessionState::Closed(reason) => return Err(reason),
                SessionState::Starting => {}
            }
            if self.state.changed().await.is_err() {
                return Err(CloseReason::Failed("portal owner terminated".into()));
            }
        }
    }

    pub async fn stop(&mut self) -> Result<(), String> {
        self.input.gate.revoke();
        self.frames.revoke();
        let _ = self.cancel.send(true);
        if let Some(owner) = self.owner.as_mut() {
            owner
                .await
                .map_err(|e| format!("portal owner failed: {e}"))?;
            // Retain the exact handle if the caller cancels this wait. A second
            // stop must still await retirement, not claim an already-idle owner.
            self.owner.take();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn on_test_bus(options: PortalOptions, address: String) -> Result<Self, String> {
        // Protocol-only fixture: its fake socket peers do not speak PipeWire.
        Self::spawn(options, Some(address), false, false)
    }

    #[cfg(test)]
    pub(crate) fn on_capture_test_bus(
        options: PortalOptions,
        address: String,
    ) -> Result<Self, String> {
        Self::spawn(options, Some(address), true, false)
    }

    #[cfg(test)]
    pub(crate) fn on_input_test_bus(
        options: PortalOptions,
        address: String,
    ) -> Result<Self, String> {
        Self::spawn(options, Some(address), false, true)
    }

    #[cfg(test)]
    pub(crate) fn on_native_test_bus(
        options: PortalOptions,
        address: String,
    ) -> Result<Self, String> {
        Self::spawn(options, Some(address), true, true)
    }
}

impl Drop for PortalSession {
    fn drop(&mut self) {
        self.input.gate.revoke();
        self.frames.revoke();
        let _ = self.cancel.send(true);
        // Dropping JoinHandle detaches; supervision still closes the session.
    }
}

struct Owner<'a> {
    connection: &'a Connection,
    destination: String,
    sender: String,
    path: OwnedObjectPath,
    pending_request: Mutex<Option<OwnedObjectPath>>,
}

// Native capture and EI consumers share this supervisor lifetime. Their frame
// and input readiness are separate states, never implied by this portal grant.
struct Negotiated {
    transports: Transports,
    grant: PortalGrant,
}

pub(crate) fn string_value(value: String) -> OwnedValue {
    Str::from(value).into()
}

fn token() -> String {
    format!("cu_{}", uuid::Uuid::new_v4().simple())
}

async fn signal(
    connection: &Connection,
    sender: &str,
    path: &str,
    interface: &str,
    member: &str,
) -> Result<MessageStream, String> {
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender(sender)
        .map_err(err)?
        .path(path)
        .map_err(err)?
        .interface(interface)
        .map_err(err)?
        .member(member)
        .map_err(err)?
        .build();
    MessageStream::for_match_rule(rule, connection, Some(16))
        .await
        .map_err(err)
}

async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    while !*cancel.borrow_and_update() {
        if cancel.changed().await.is_err() {
            return;
        }
    }
}

async fn owner_lost(stream: &mut MessageStream, pinned: &str) {
    while let Some(Ok(message)) = stream.next().await {
        let Ok((name, old, _new)) = message.body().deserialize::<(String, String, String)>() else {
            return;
        };
        if name == SERVICE && old == pinned {
            return;
        }
    }
}

async fn supervise(
    options: PortalOptions,
    address: Option<String>,
    mut cancellation: watch::Receiver<bool>,
    publish: watch::Sender<SessionState>,
    mut consumers: Consumers,
) {
    let deadline = Instant::now() + options.timeout;
    let connect = async {
        match address {
            Some(address) => zbus::connection::Builder::address(address.as_str())
                .map_err(err)?
                .build()
                .await
                .map_err(err),
            None => Connection::session().await.map_err(err),
        }
    };
    let connection = tokio::select! {
        biased;
        _ = cancelled(&mut cancellation) => Err(CloseReason::Stopped),
        result = tokio::time::timeout_at(deadline, connect) => match result {
            Ok(Ok(connection)) => Ok(connection),
            Ok(Err(error)) => Err(CloseReason::Failed(error)),
            Err(_) => Err(CloseReason::TimedOut),
        }
    };
    let connection = match connection {
        Ok(connection) => connection,
        Err(reason) => {
            publish.send_replace(SessionState::Closed(reason));
            return;
        }
    };
    let prepared = tokio::select! {
        biased;
        _ = cancelled(&mut cancellation) => Err(CloseReason::Stopped),
        result = tokio::time::timeout_at(deadline, Owner::prepare(&connection)) => match result {
            Ok(Ok(owner)) => Ok(owner),
            Ok(Err(error)) => Err(CloseReason::Failed(error)),
            Err(_) => Err(CloseReason::TimedOut),
        }
    };
    let reason = match prepared {
        Ok(owner) => {
            let reason = owner
                .run(
                    &options,
                    deadline,
                    &mut cancellation,
                    &publish,
                    &mut consumers,
                )
                .await;
            publish.send_replace(SessionState::Closing(reason.clone()));
            owner.cleanup().await;
            reason
        }
        Err(reason) => {
            publish.send_replace(SessionState::Closing(reason.clone()));
            reason
        }
    };
    let _ = tokio::time::timeout(CLEANUP_LIMIT, connection.close()).await;
    consumers.frames.revoke();
    consumers.gate.revoke();
    publish.send_replace(SessionState::Closed(reason));
}

impl<'a> Owner<'a> {
    async fn prepare(connection: &'a Connection) -> Result<Self, String> {
        let proxy = zbus::fdo::DBusProxy::new(connection).await.map_err(err)?;
        // Desktop portals are normally D-Bus activated. This starts only the
        // well-known portal service; it does not create or grant a session.
        // Resolve once. Every subsequent call and signal is bound to this
        // unique owner; a replacement service cannot inherit the old grant.
        let destination = match proxy.get_name_owner(SERVICE.try_into().map_err(err)?).await {
            Ok(owner) => owner,
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => {
                // An already-owned non-activatable service is valid; some buses
                // reject StartServiceByName without a .service file even then.
                let activated = proxy
                    .start_service_by_name(SERVICE.try_into().map_err(err)?, 0)
                    .await
                    .map_err(err)?;
                if !matches!(activated, 1 | 2) {
                    return Err("invalid portal activation response".into());
                }
                proxy
                    .get_name_owner(SERVICE.try_into().map_err(err)?)
                    .await
                    .map_err(err)?
            }
            Err(error) => return Err(err(error)),
        }
        .to_string();
        let sender = connection
            .unique_name()
            .ok_or("missing portal bus identity")?
            .as_str()
            .trim_start_matches(':')
            .replace('.', "_");
        let path = OwnedObjectPath::try_from(format!("{ROOT}/session/{sender}/{}", token()))
            .map_err(err)?;
        Ok(Self {
            connection,
            destination,
            sender,
            path,
            pending_request: Mutex::new(None),
        })
    }

    async fn run(
        &self,
        options: &PortalOptions,
        deadline: Instant,
        cancel: &mut watch::Receiver<bool>,
        publish: &watch::Sender<SessionState>,
        consumers: &mut Consumers,
    ) -> CloseReason {
        let subscriptions = async {
            let closed = signal(
                self.connection,
                &self.destination,
                self.path.as_str(),
                SESSION,
                "Closed",
            )
            .await?;
            let owners = signal(
                self.connection,
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "NameOwnerChanged",
            )
            .await?;
            // Close the resolve/subscribe race without adopting a replacement.
            let current = zbus::fdo::DBusProxy::new(self.connection)
                .await
                .map_err(err)?
                .get_name_owner(SERVICE.try_into().map_err(err)?)
                .await
                .map_err(err)?;
            if current.as_str() != self.destination {
                return Err("portal owner changed during setup".into());
            }
            Ok::<_, String>((closed, owners))
        };
        let (mut closed, mut owners) = tokio::select! {
            biased;
            _ = cancelled(cancel) => return CloseReason::Stopped,
            result = tokio::time::timeout_at(deadline, subscriptions) => match result {
                Ok(Ok(streams)) => streams,
                Ok(Err(error)) => return CloseReason::Failed(error),
                Err(_) => return CloseReason::TimedOut,
            }
        };
        // Both subscriptions exist before CreateSession, including a synchronous
        // Closed/Response emitted before the corresponding method reply.
        let transports = tokio::select! {
            biased;
            _ = cancelled(cancel) => return CloseReason::Stopped,
            _ = self.connection.closed() => return CloseReason::BusDisconnected,
            signal = closed.next() => return closed_reason(signal),
            _ = owner_lost(&mut owners, &self.destination) => return CloseReason::PortalOwnerLost,
            result = tokio::time::timeout_at(deadline, self.negotiate(options)) => match result {
                Ok(Ok(transports)) => transports,
                Ok(Err(error)) => return CloseReason::Failed(error),
                Err(_) => return CloseReason::TimedOut,
            }
        };
        // Complete fallible fd duplication before starting either native owner.
        let fds = (|| -> Result<_, String> {
            Ok((
                if consumers.capture {
                    Some(transports.transports.capture_fd()?)
                } else {
                    None
                },
                if consumers.input.is_some() {
                    Some(transports.transports.input_fd()?)
                } else {
                    None
                },
            ))
        })();
        let (capture_fd, input_fd) = match fds {
            Ok(fds) => fds,
            Err(message) => return CloseReason::Failed(message),
        };
        let mut consumer = capture_fd.map(|fd| {
            CaptureWorker::start(
                fd,
                transports.grant.stream.clone(),
                options.run_id.clone(),
                consumers.frames.clone(),
            )
        });
        let mut input = input_fd.map(|fd| {
            let (gate, receive) = consumers.input.take().expect("input owner checked");
            InputWorker::start(
                fd,
                transports.grant.stream.clone(),
                options.run_id.clone(),
                gate,
                receive,
            )
        });
        publish.send_replace(SessionState::Granted(transports.grant.clone()));
        let mut reason = tokio::select! {
            biased;
            _ = cancelled(cancel) => CloseReason::Stopped,
            _ = self.connection.closed() => CloseReason::BusDisconnected,
            signal = closed.next() => closed_reason(signal),
            _ = owner_lost(&mut owners, &self.destination) => CloseReason::PortalOwnerLost,
            name = transports.transports.until_disconnected() => CloseReason::TransportClosed(name),
            result = async {
                match consumer.as_mut() {
                    Some(worker) => worker.finished().await,
                    None => std::future::pending().await,
                }
            } => match result {
                Ok(()) => CloseReason::TransportClosed("PipeWire"),
                Err(message) => CloseReason::Failed(message),
            },
            result = async {
                match input.as_mut() {
                    Some(worker) => worker.finished().await,
                    None => std::future::pending().await,
                }
            } => match result {
                Ok(()) => CloseReason::TransportClosed("EIS"),
                Err(message) => CloseReason::Failed(message),
            },
        };
        // Invalidate before closing either descriptor. No public fd clones.
        publish.send_replace(SessionState::Closing(reason.clone()));
        consumers.frames.revoke();
        consumers.gate.revoke();
        if let Some(worker) = input.as_mut() {
            let _ = worker.stop().await;
        }
        if let Some(worker) = consumer.as_mut() {
            // Never declare Closed/idle while the native consumer still owns
            // the fd/buffers. Cancellation of the caller's stop cannot detach
            // this wait; the supervisor owns retirement to actual completion.
            if let Err(message) = worker.stop().await {
                // The native thread has joined, but a missing daemon teardown
                // acknowledgement must not be reported as a successful Stop.
                reason = CloseReason::Failed(format!("{reason:?}; {message}"));
            }
        }
        drop(transports);
        reason
    }

    async fn request<B>(
        &self,
        interface: &str,
        method: &str,
        make_body: impl FnOnce(String) -> B,
    ) -> Result<Dict, String>
    where
        B: serde::Serialize + DynamicType,
    {
        let token = token();
        let expected = OwnedObjectPath::try_from(format!("{ROOT}/request/{}/{token}", self.sender))
            .map_err(err)?;
        let mut responses = signal(
            self.connection,
            &self.destination,
            expected.as_str(),
            REQUEST,
            "Response",
        )
        .await?;
        *self.pending_request.lock().map_err(err)? = Some(expected.clone());
        let reply = self
            .connection
            .call_method(
                Some(self.destination.as_str()),
                ROOT,
                Some(interface),
                method,
                &make_body(token),
            )
            .await
            .map_err(err)?;
        let returned: OwnedObjectPath = reply.body().deserialize().map_err(err)?;
        if returned != expected {
            return Err(format!("{method} returned a different request identity"));
        }
        let message = responses
            .next()
            .await
            .ok_or("portal response stream ended")?
            .map_err(err)?;
        let (status, results): (u32, Dict) = message.body().deserialize().map_err(err)?;
        self.pending_request.lock().map_err(err)?.take();
        match status {
            0 => Ok(results),
            1 => Err(format!("{method}: user cancelled portal consent")),
            2 => Err(format!("{method}: portal rejected request")),
            _ => Err(format!("{method}: invalid portal response code")),
        }
    }

    async fn negotiate(&self, options: &PortalOptions) -> Result<Negotiated, String> {
        let remote = Proxy::new(self.connection, self.destination.as_str(), ROOT, REMOTE)
            .await
            .map_err(err)?;
        let screen = Proxy::new(self.connection, self.destination.as_str(), ROOT, SCREEN)
            .await
            .map_err(err)?;
        let remote_version: u32 = remote.get_property("version").await.map_err(err)?;
        let screen_version: u32 = screen.get_property("version").await.map_err(err)?;
        if remote_version < 2 || screen_version < 3 {
            return Err(
                "portal needs RemoteDesktop >= 2 and ScreenCast >= 3; no XWayland fallback".into(),
            );
        }
        let devices: u32 = remote
            .get_property("AvailableDeviceTypes")
            .await
            .map_err(err)?;
        let sources: u32 = screen
            .get_property("AvailableSourceTypes")
            .await
            .map_err(err)?;
        let cursors: u32 = screen
            .get_property("AvailableCursorModes")
            .await
            .map_err(err)?;
        if devices & 3 != 3 || sources & options.source.mask() == 0 || cursors & 1 == 0 {
            return Err("portal lacks requested input/source/cursor capabilities".into());
        }
        let session_token = self
            .path
            .as_str()
            .rsplit('/')
            .next()
            .ok_or("invalid session identity")?
            .to_owned();
        let mut created = self
            .request(REMOTE, "CreateSession", |token| {
                Dict::from([
                    ("handle_token".into(), string_value(token)),
                    ("session_handle_token".into(), string_value(session_token)),
                ])
            })
            .await?;
        // The standard intentionally uses a string here, not D-Bus object-path.
        let created_path = String::try_from(
            created
                .remove("session_handle")
                .ok_or("missing portal session")?,
        )
        .map_err(|_| "portal session handle must be a string")?;
        if created_path != self.path.as_str() {
            return Err("portal returned a different session identity".into());
        }
        self.request(REMOTE, "SelectDevices", |token| {
            (
                self.path.clone(),
                Dict::from([
                    ("handle_token".into(), string_value(token)),
                    ("types".into(), 3u32.into()),
                    // Never persist consent or automatically replay a restore token.
                    ("persist_mode".into(), 0u32.into()),
                ]),
            )
        })
        .await?;
        self.request(SCREEN, "SelectSources", |token| {
            (
                self.path.clone(),
                Dict::from([
                    ("handle_token".into(), string_value(token)),
                    ("types".into(), options.source.mask().into()),
                    ("multiple".into(), false.into()),
                    ("cursor_mode".into(), 1u32.into()),
                    // Combined-session persistence belongs ONLY to RemoteDesktop.
                ]),
            )
        })
        .await?;
        let mut started = self
            .request(REMOTE, "Start", |token| {
                (
                    self.path.clone(),
                    options.parent_window.clone(),
                    Dict::from([("handle_token".into(), string_value(token))]),
                )
            })
            .await?;
        let stream = parse_stream(&mut started, options.source)?;
        let pipewire: zbus::zvariant::OwnedFd = screen
            .call("OpenPipeWireRemote", &(self.path.clone(), Dict::new()))
            .await
            .map_err(err)?;
        let eis: zbus::zvariant::OwnedFd = remote
            .call("ConnectToEIS", &(self.path.clone(), Dict::new()))
            .await
            .map_err(err)?;
        Ok(Negotiated {
            transports: Transports::new(pipewire.into(), eis.into())?,
            grant: PortalGrant {
                run_id: options.run_id.clone(),
                session_path: self.path.to_string(),
                remote_desktop_version: remote_version,
                screen_cast_version: screen_version,
                devices: 3,
                stream,
            },
        })
    }

    async fn cleanup(&self) {
        let request = self
            .pending_request
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        let close = |path: OwnedObjectPath, interface: &'static str| async move {
            let _ = self
                .connection
                .call_method(
                    Some(self.destination.as_str()),
                    path,
                    Some(interface),
                    "Close",
                    &(),
                )
                .await;
        };
        let cleanup = async {
            if let Some(request) = request {
                tokio::join!(close(request, REQUEST), close(self.path.clone(), SESSION));
            } else {
                close(self.path.clone(), SESSION).await;
            }
        };
        let _ = tokio::time::timeout(CLEANUP_LIMIT, cleanup).await;
    }
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn closed_reason(signal: Option<zbus::Result<zbus::Message>>) -> CloseReason {
    match signal {
        Some(Ok(message)) if message.body().deserialize::<Dict>().is_ok() => {
            CloseReason::PortalClosed
        }
        Some(Ok(_)) => CloseReason::Failed("malformed portal Closed signal".into()),
        _ => CloseReason::BusDisconnected,
    }
}
