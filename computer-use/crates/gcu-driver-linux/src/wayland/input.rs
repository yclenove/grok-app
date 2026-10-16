//! Window-confined portal input, write-ahead ledger and independent Stop/watchdog.
//!
//! Only DriverAction (already resolved by ActionRouter) reaches this module. There is
//! no MCP/raw-input surface, monitor fallback, implicit activation or automatic replay.

use super::{
    capabilities::EnvironmentCapabilities,
    ei::{EiSnapshot, HeldInput},
    input_control::{
        controlled_capabilities, unavailable, InputAuthority, InputClock, RemoteDesktopConnection,
        RemoteDesktopPortal, StopControl, WATCHDOG_TICK,
    },
    input_plan,
    portal::PortalSourceKind,
    restore::{OriginBinding, RestoreError, RestoreRecord, RestoreTokenStore},
    windows::{StreamIdentity, WindowBinding},
};
use gcu_core::driver::{
    Cancel, Delivery, DriverAction, DriverError, Result, WindowRef, DRIVER_DEADLINE,
};
use gcu_protocol::Capabilities;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::time::Duration;

/// Trusted assembly data, retained on the server and never parsed from model arguments.
pub struct InputConfig {
    /// Exact Router window, paired with the compositor binding below.
    pub window: WindowRef,
    /// P7-04 OS-inspected window/process/image identity.
    pub binding: WindowBinding,
    /// P7-01 capability table before runtime control promotion.
    pub capabilities: EnvironmentCapabilities,
    /// P7-02 private credential store.
    pub tokens: Arc<RestoreTokenStore>,
    /// Unique credential key, scoped to this logical session and target.
    pub token_key: String,
    /// Trusted authorization version.
    pub auth_version: String,
    /// Absolute authorization expiry from the Host clock.
    pub auth_expires_secs: u64,
    /// Injectable authorization clock.
    pub clock: Arc<dyn InputClock>,
    /// Full normalized authorized-window physical dimensions, matching the latest capture.
    pub physical_size: (u32, u32),
}
impl InputConfig {
    /// Full P7-02 binding including compositor/window lifetimes and image digest.
    pub fn origin(&self) -> OriginBinding {
        let image = &self.binding.image;
        OriginBinding {
            compositor: format!("{}:{}", self.binding.compositor.slug(), self.binding.epoch),
            source_kind: "window".into(),
            window_identity: format!(
                "{}:{}",
                self.binding.record.id, self.binding.record.generation
            ),
            process_instance: format!(
                "{:?}:{}:{}:{}:{}:{}",
                image.exe_path, image.dev, image.inode, image.sha256, image.pid, image.start_time
            ),
            auth_version: self.auth_version.clone(),
        }
    }
}

/// One user-approved portal/EI session. Drop, cancellation and every uncertain error
/// retire it permanently. Only a new explicit consent transaction can create another.
pub struct PortalInput {
    config: InputConfig,
    connection: RemoteDesktopConnection,
    authority: Arc<dyn InputAuthority>,
    stop: Arc<StopControl>,
    caps: Capabilities,
    caps_table: EnvironmentCapabilities,
    snapshot: Mutex<Option<EiSnapshot>>,
    proof: Mutex<Option<StreamIdentity>>,
    held: Mutex<Vec<HeldInput>>,
    active: Arc<AtomicBool>,
    token_lock: Arc<Mutex<()>>,
    cleanup_started: AtomicBool,
    retired: Cancel,
    closed: Cancel,
    close_error: Arc<Mutex<Option<DriverError>>>,
    action_lock: tokio::sync::Mutex<()>,
}

struct Retire<'a>(&'a PortalInput, bool);
impl Drop for Retire<'_> {
    fn drop(&mut self) {
        if self.1 {
            self.0.shutdown();
        }
    }
}
struct TokenLease {
    store: Arc<RestoreTokenStore>,
    key: String,
    armed: bool,
}
impl Drop for TokenLease {
    fn drop(&mut self) {
        if self.armed {
            let store = self.store.clone();
            let key = self.key.clone();
            tokio::spawn(async move {
                if token_io(move || store.revoke(&key)).await.is_err() {
                    tracing::warn!("portal credential cleanup failed");
                }
            });
        }
    }
}
async fn token_io<T: Send + 'static>(
    work: impl FnOnce() -> std::result::Result<T, RestoreError> + Send + 'static,
) -> Result<T> {
    tokio::time::timeout(DRIVER_DEADLINE, tokio::task::spawn_blocking(work))
        .await
        .map_err(|_| DriverError::Timeout {
            maybe_executed: false,
        })?
        .map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?
        .map_err(|_| DriverError::Native {
            message: "portal credential rejected".into(),
            maybe_executed: false,
        })
}

impl PortalInput {
    /// First use goes through portal consent. Restores are bound/expiry checked before
    /// being supplied, then scope and device mapping are proven afresh. All awaits are
    /// cancellable and bounded by DRIVER_DEADLINE; Stop has its own independent task.
    pub async fn open(
        config: InputConfig,
        portal: &dyn RemoteDesktopPortal,
        authority: Arc<dyn InputAuthority>,
        stop: Arc<StopControl>,
        cancel: &Cancel,
    ) -> Result<Arc<Self>> {
        let mut lease = TokenLease {
            store: config.tokens.clone(),
            key: config.token_key.clone(),
            armed: true,
        };
        let caps = controlled_capabilities(config.capabilities.clone(), &stop, authority.as_ref());
        if !caps.global_stop_hotkey.status.is_available()
            || !caps.foreground_input.status.is_available()
            || !stop.available()
            || !authority.confined()
            || config.binding.record.id != config.window.id
            || config.capabilities.compositor != config.binding.compositor
            || !config.binding.record.peer_pid
            || config.binding.record.pid != Some(config.binding.image.pid)
            || config.binding.image.start_time == 0
            || config.binding.record.pid != config.window.app.info.pid
            || config.auth_version.is_empty()
            || config.clock.now_secs() >= config.auth_expires_secs
            || config.physical_size.0 == 0
            || config.physical_size.1 == 0
        {
            return Err(unavailable());
        }
        let origin = config.origin();
        let store = config.tokens.clone();
        let key = config.token_key.clone();
        let now = config.clock.now_secs();
        let restore = token_io(move || match store.load_and_validate(&key, &origin, now) {
            Ok(token) => Ok(Some(token)),
            Err(RestoreError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        })
        .await?;
        let connection = tokio::select! {
            biased;
            _=cancel.cancelled()=>return Err(DriverError::Cancelled {maybe_executed:false}),
            _=stop.stopped()=>return Err(unavailable()),
            result=tokio::time::timeout(DRIVER_DEADLINE,portal.open(restore.as_deref()))=>result.map_err(|_|DriverError::Timeout {maybe_executed:false})??,
        };
        let input = Arc::new(Self {
            config,
            connection,
            authority,
            stop,
            caps: caps.to_protocol_capabilities(),
            caps_table: caps,
            snapshot: Mutex::new(None),
            proof: Mutex::new(None),
            held: Mutex::new(Vec::new()),
            active: Arc::new(AtomicBool::new(true)),
            token_lock: Arc::new(Mutex::new(())),
            cleanup_started: AtomicBool::new(false),
            retired: Cancel::default(),
            closed: Cancel::default(),
            close_error: Arc::new(Mutex::new(None)),
            action_lock: tokio::sync::Mutex::new(()),
        });
        lease.armed = false;
        let weak = Arc::downgrade(&input);
        tokio::spawn(async move {
            loop {
                let Some(input) = weak.upgrade() else { break };
                if !input.stop.available() {
                    input.stop.listener_failed();
                }
                if !input.live() {
                    input.shutdown();
                    break;
                }
                let stop = input.stop.clone();
                drop(input);
                tokio::select! {biased; _=stop.stopped()=>{}, _=tokio::time::sleep(WATCHDOG_TICK)=>{} }
            }
        });
        let mut retire = Retire(&input, true);
        let proof = input.checked_proof(cancel).await?;
        let snap = input.connection.ei.snapshot().ok_or_else(unavailable)?;
        let stream = &input.connection.stream;
        stream
            .validate(PortalSourceKind::Window)
            .map_err(|_| unavailable())?;
        if stream.mapping_id.as_deref() != Some(snap.region.mapping_id.as_str())
            || stream.size != Some((snap.region.width as i32, snap.region.height as i32))
            || input.connection.session.handle() != proof.session
            || snap.generation == 0
        {
            return Err(unavailable());
        }
        snap.region.map(
            input.config.physical_size,
            gcu_core::model::WindowPoint { x: 0.0, y: 0.0 },
        )?;
        *input.snapshot.lock().unwrap_or_else(|p| p.into_inner()) = Some(snap);
        *input.proof.lock().unwrap_or_else(|p| p.into_inner()) = Some(proof);
        if let Some(token) = &input.connection.restore_token {
            let store = input.config.tokens.clone();
            let key = input.config.token_key.clone();
            let record = RestoreRecord::new(
                token,
                input.config.origin(),
                input.config.clock.now_secs(),
                input.config.auth_expires_secs,
            );
            let live = input.active.clone();
            let serial = input.token_lock.clone();
            token_io(move || {
                let _guard = serial.lock().unwrap_or_else(|p| p.into_inner());
                if live.load(Ordering::Acquire) {
                    store.save(&key, &record)?;
                }
                if !live.load(Ordering::Acquire) {
                    store.revoke(&key)?;
                }
                Ok(())
            })
            .await?;
        }
        if !input.live() {
            return Err(unavailable());
        }
        retire.1 = false;
        drop(retire);
        Ok(input)
    }
    fn live(&self) -> bool {
        let generation_ok = self
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_none_or(|snap| {
                self.connection
                    .ei
                    .snapshot()
                    .is_some_and(|now| now.generation == snap.generation)
            });
        generation_ok
            && self.active.load(Ordering::Acquire)
            && self.connection.session.is_active()
            && self.stop.available()
            && self.authority.confined()
            && self.config.clock.now_secs() < self.config.auth_expires_secs
    }
    /// Nonblocking runtime capabilities. A failed listener/source/session disables input.
    pub fn available(&self) -> bool {
        self.live() && self.connection.ei.snapshot().is_some()
    }
    /// P7-01 declaration after consent and verified controls; callers check available().
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }
    /// Detailed P7-01 table shared with doctor; only meaningful while available().
    pub fn environment_capabilities(&self) -> &EnvironmentCapabilities {
        &self.caps_table
    }
    /// Exact bound Router window; no approximate title/ID resolution.
    pub fn window(&self) -> &WindowRef {
        &self.config.window
    }
    async fn checked_proof(&self, cancel: &Cancel) -> Result<StreamIdentity> {
        let proof = tokio::select! {
            biased;
            _=cancel.cancelled()=>return Err(DriverError::Cancelled {maybe_executed:false}),
            _=self.stop.stopped()=>return Err(unavailable()),
            _=self.retired.cancelled()=>return Err(unavailable()),
            result=tokio::time::timeout(DRIVER_DEADLINE,self.authority.prove(self.connection.session.handle(),&self.connection.stream,&self.config.binding))=>result.map_err(|_|DriverError::Timeout {maybe_executed:false})??,
        };
        if proof.window != self.config.binding
            || proof.session != self.connection.session.handle()
            || proof.source.is_empty()
            || proof.node_id != self.connection.stream.node_id
            || proof.serial == 0
            || self
                .connection
                .stream
                .pipewire_serial
                .is_some_and(|s| s != proof.serial)
        {
            return Err(unavailable());
        }
        let previous = self.proof.lock().unwrap_or_else(|p| p.into_inner());
        if previous.as_ref().is_some_and(|p| p != &proof) {
            return Err(unavailable());
        }
        Ok(proof)
    }
    /// Deliver an ActionRouter-resolved action once. Auto/targeted routes never escalate.
    /// Cancellation (including dropping this future), error or deadline releases held
    /// input and closes the portal. No release waits behind action_lock or source proof.
    pub async fn perform(&self, action: &DriverAction, cancel: &Cancel) -> Result<Delivery> {
        if cancel.is_cancelled() {
            self.shutdown();
            return Err(DriverError::Cancelled {
                maybe_executed: false,
            });
        }
        if input_plan::window(action) != Some(&self.config.window) {
            return Err(DriverError::WindowGone);
        }
        let started = AtomicBool::new(false);
        let mut retire = Retire(self, true);
        let work = async {
            let _serial = self.action_lock.lock().await;
            if !self.available() {
                return Err(unavailable());
            }
            self.checked_proof(cancel).await?;
            let snap = self
                .snapshot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .clone()
                .ok_or_else(unavailable)?;
            let events = input_plan::plan(action, &snap, self.config.physical_size)?;
            for event in events {
                {
                    // Lock order is ledger -> transport. Only bounded nonblocking calls inside.
                    // Stop acquires the same ledger so no down can race after its release sweep.
                    let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
                    if !self.live() || cancel.is_cancelled() {
                        return Err(DriverError::Cancelled {
                            maybe_executed: started.load(Ordering::Acquire),
                        });
                    }
                    if let Some((key, true)) = event.held() {
                        if !held.contains(&key) {
                            held.push(key);
                        }
                    }
                    started.store(true, Ordering::Release);
                    self.connection.ei.send(&event, Some(snap.generation))?;
                    if let Some((key, false)) = event.held() {
                        held.retain(|k| *k != key);
                    }
                }
                tokio::task::yield_now().await;
            }
            Ok(Delivery::Foreground)
        };
        let result = tokio::select! {biased;
            _=cancel.cancelled()=>Err(DriverError::Cancelled {maybe_executed:started.load(Ordering::Acquire)}),
            _=self.stop.stopped()=>Err(DriverError::Cancelled {maybe_executed:started.load(Ordering::Acquire)}),
            _=self.retired.cancelled()=>Err(DriverError::Cancelled {maybe_executed:started.load(Ordering::Acquire)}),
            result=tokio::time::timeout(DRIVER_DEADLINE,work)=>result.unwrap_or_else(|_|Err(DriverError::Timeout {maybe_executed:started.load(Ordering::Acquire)})),
        };
        if result.is_ok() {
            retire.1 = false;
        }
        result.map_err(|e| {
            if started.load(Ordering::Acquire) && !e.maybe_executed() {
                DriverError::Native {
                    message: "portal input interrupted after dispatch".into(),
                    maybe_executed: true,
                }
            } else {
                e
            }
        })
    }
    /// Independently send every outstanding up, even if an earlier release fails.
    /// A failed up remains in the ledger until the session is disconnected.
    pub fn release_all_input(&self) -> Result<()> {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        let mut failed = None;
        held.retain(|key| match self.connection.ei.send(&key.up(), None) {
            Ok(()) => false,
            Err(e) => {
                failed.get_or_insert(e);
                true
            }
        });
        failed.map_or(Ok(()), Err)
    }
    /// Synchronously latch Stop; cleanup/ups are independent of action and approval awaits.
    pub fn stop(&self) {
        self.stop.stop();
        self.shutdown();
    }
    pub(crate) fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
        self.retired.cancel();
        if self.cleanup_started.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Err(e) = self.release_all_input() {
            *self.close_error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
        }
        self.connection.ei.disconnect();
        let session = self.connection.session.clone();
        let tokens = self.config.tokens.clone();
        let key = self.config.token_key.clone();
        let closed = self.closed.clone();
        let error = self.close_error.clone();
        let token_serial = self.token_lock.clone();
        tokio::spawn(async move {
            let portal = async {
                tokio::time::timeout(DRIVER_DEADLINE, session.close())
                    .await
                    .map_err(|_| DriverError::Timeout {
                        maybe_executed: false,
                    })?
                    .map_err(|_| DriverError::Native {
                        message: "portal close failed".into(),
                        maybe_executed: false,
                    })
            };
            let (portal, token) = tokio::join!(
                portal,
                token_io(move || {
                    let _guard = token_serial.lock().unwrap_or_else(|p| p.into_inner());
                    tokens.revoke(&key)
                })
            );
            if let Err(e) = portal.and(token) {
                error
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get_or_insert(e);
            }
            closed.cancel();
        });
    }
    /// Revoke/disable/session-close path: release first, close portal, erase this token.
    /// Idempotent, independent of the caller's cancelled action token, bounded to five seconds.
    pub async fn close(&self) -> Result<()> {
        self.shutdown();
        tokio::time::timeout(Duration::from_secs(5), self.closed.cancelled())
            .await
            .map_err(|_| DriverError::Timeout {
                maybe_executed: false,
            })?;
        self.close_error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .map_or(Ok(()), Err)
    }
}
impl Drop for PortalInput {
    fn drop(&mut self) {
        self.shutdown();
    }
}
