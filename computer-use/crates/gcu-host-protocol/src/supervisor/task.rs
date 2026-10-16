//! Drive [`super::HostState`] with a replaceable launcher.
//!
//! Identity refusal happens before a spawn and is not a crash. A failed
//! handshake is counted and is not immediately retried. An unexpected exit
//! restarts until the breaker opens. The stop latch is copied into every
//! handshake, including the one after a crash.

use std::{future::Future, pin::Pin, sync::Arc};

use crate::messages::{Empty, SessionId, SessionKind, SessionOpen, SessionOpened};
use crate::server::{HostCallbacks, RPC_TIMEOUT};

use super::{
    admit, baseline_policy, closed_callbacks, privilege_allowed, EngineChoice, EngineFacts,
    EngineProcess, EngineSession, Error, HostBuild, HostState, LaunchRequest, Phase,
    ReleaseIdentity, Signal, Step,
};

/// Facts checked again on every spawn. The running child keeps its own copy.
#[derive(Clone, Debug)]
pub struct Admission {
    /// Release or developer build of the App.
    pub mode: HostBuild,
    /// Identity this App package shipped.
    pub expected: ReleaseIdentity,
    /// Bundled sidecar or a developer external path.
    pub choice: EngineChoice,
    /// Probe results. Model text never fills this.
    pub facts: EngineFacts,
}

/// Visible supervisor status. No process handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// Current phase.
    pub phase: Phase,
    /// Feature switch.
    pub enabled: bool,
    /// Persistent user stop.
    pub stop_latched: bool,
    /// Enabled local sessions.
    pub sessions: u32,
}

/// Future returned by [`Launch::spawn`].
pub type SpawnFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Box<dyn EngineProcess>, Error>> + Send + 'a>>;

/// Starts one admitted engine. Tests supply a fake; the App uses [`OsLaunch`].
pub trait Launch: Send {
    /// Handshake one child. The future must kill that child if it is dropped.
    fn spawn<'a>(&'a mut self, request: &'a LaunchRequest) -> SpawnFuture<'a>;
}

/// Production launcher. No `--driver fake` and no approval grants.
pub struct OsLaunch {
    callbacks: Arc<dyn HostCallbacks>,
}

impl OsLaunch {
    /// Closed callbacks and the baseline policy.
    pub fn production() -> Self {
        Self::callbacks(closed_callbacks(baseline_policy()))
    }

    /// Replace callbacks when the App approval card is attached.
    pub fn callbacks(callbacks: Arc<dyn HostCallbacks>) -> Self {
        Self { callbacks }
    }
}

impl Launch for OsLaunch {
    fn spawn<'a>(&'a mut self, request: &'a LaunchRequest) -> SpawnFuture<'a> {
        let request = request.clone();
        let callbacks = Arc::clone(&self.callbacks);
        Box::pin(async move {
            let session = EngineSession::spawn(&request, callbacks).await?;
            Ok(Box::new(session) as Box<dyn EngineProcess>)
        })
    }
}

/// One engine for every session, plus the idle and crash policy around it.
pub struct Control<L: Launch> {
    state: HostState,
    launch: L,
    request: LaunchRequest,
    admission: Admission,
    engine: Option<Box<dyn EngineProcess>>,
}

impl<L: Launch> Control<L> {
    /// Stopped, disabled, and holding the caller's admission snapshot.
    pub fn new(launch: L, request: LaunchRequest, admission: Admission) -> Self {
        Self {
            state: HostState::new(),
            launch,
            request,
            admission,
            engine: None,
        }
    }

    /// Replace the binary used on the next spawn. Does not touch a live child.
    pub fn set_launch(&mut self, request: LaunchRequest, admission: Admission) {
        self.request = request;
        self.admission = admission;
    }

    /// Current phase and latch.
    pub fn status(&self) -> Status {
        Status {
            phase: self.state.phase,
            enabled: self.state.enabled,
            stop_latched: self.state.stop_latched,
            sessions: self.state.sessions,
        }
    }

    /// Apply one signal and perform at most the spawn it asks for.
    pub async fn handle(&mut self, signal: Signal, now_ms: u64) -> Result<(), Error> {
        if matches!(signal, Signal::SetEnabled(true)) && self.state.phase == Phase::Tripped {
            return Err(Error::Tripped);
        }
        let step = self.state.apply(signal, now_ms);
        self.act(step, now_ms).await
    }

    /// Observe an exit, then apply the idle clock.
    pub async fn poll(&mut self, now_ms: u64) -> Result<(), Error> {
        if let Some(engine) = self.engine.as_mut() {
            let exit = engine.poll_exit();
            if !matches!(exit, super::PollExit::Running) {
                self.engine = None;
                let step = self.state.apply(
                    Signal::ChildExited {
                        clean: matches!(exit, super::PollExit::Clean),
                    },
                    now_ms,
                );
                self.act(step, now_ms).await?;
            }
        }
        let step = self.state.apply(Signal::Tick, now_ms);
        self.act(step, now_ms).await
    }

    async fn act(&mut self, mut step: Step, now_ms: u64) -> Result<(), Error> {
        loop {
            step = match step {
                Step::None => return Ok(()),
                Step::Spawn | Step::Restart => return self.start(now_ms).await,
                Step::Shutdown => {
                    if let Some(engine) = self.engine.as_mut() {
                        engine.begin_shutdown();
                    }
                    return Ok(());
                }
                Step::Kill => {
                    if let Some(mut engine) = self.engine.take() {
                        engine.kill();
                    }
                    self.state
                        .apply(Signal::ChildExited { clean: true }, now_ms)
                }
            };
        }
    }

    /// Open one local MCP session. The engine mints the bearer; this call does not log it.
    ///
    /// Five-second RPC budget. Missing peer is [`Error::Disconnected`].
    pub async fn open_local_endpoint(&self, session_id: &str) -> Result<SessionOpened, Error> {
        let peer = self
            .engine
            .as_ref()
            .and_then(|engine| engine.peer())
            .ok_or(Error::Disconnected)?;
        peer.call(
            "engine.session.open",
            &SessionOpen {
                session_id: session_id.to_owned(),
                kind: SessionKind::Local,
            },
            RPC_TIMEOUT,
        )
        .await
        .map_err(Error::Handshake)
    }

    /// Close one engine session so its HTTP bearer stops working.
    ///
    /// Five-second RPC budget. Missing peer is [`Error::Disconnected`].
    pub async fn close_endpoint(&self, session_id: &str) -> Result<(), Error> {
        let peer = self
            .engine
            .as_ref()
            .and_then(|engine| engine.peer())
            .ok_or(Error::Disconnected)?;
        let _: Empty = peer
            .call(
                "engine.session.close",
                &SessionId {
                    session_id: session_id.to_owned(),
                },
                RPC_TIMEOUT,
            )
            .await
            .map_err(Error::Handshake)?;
        Ok(())
    }

    async fn start(&mut self, now_ms: u64) -> Result<(), Error> {
        if let Err(reject) = admit(
            self.admission.mode,
            &self.admission.expected,
            &self.admission.choice,
            &self.admission.facts,
        ) {
            let _ = self
                .state
                .apply(Signal::ChildExited { clean: true }, now_ms);
            return Err(Error::Refused(reject));
        }
        let mut request = self.request.clone();
        request.stopped = self.state.stop_latched;
        match self.launch.spawn(&request).await {
            Ok(engine) => {
                let privilege = engine.privilege();
                if let Err(reject) = privilege_allowed(
                    self.admission.mode,
                    privilege.elevated,
                    privilege.ui_access,
                    privilege.unknown,
                ) {
                    let mut engine = engine;
                    engine.kill();
                    drop(engine);
                    let _ = self
                        .state
                        .apply(Signal::ChildExited { clean: true }, now_ms);
                    return Err(Error::Refused(reject));
                }
                self.engine = Some(engine);
                let _ = self.state.apply(Signal::HandshakeOk, now_ms);
                Ok(())
            }
            Err(error) => {
                let _ = self.state.apply(Signal::HandshakeFailed, now_ms);
                Err(error)
            }
        }
    }
}
