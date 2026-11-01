//! Private fake scope engine. No native/helper fallback exists on this route.
mod data;
mod fake;
mod plane;
mod relay;
mod state;

#[cfg(all(test, feature = "eval-autoapprove", debug_assertions))]
mod approval_tests;

#[cfg(all(test, feature = "script"))]
pub(crate) async fn test_evidence(
    driver: gcu_driver_fake::FakeDriver,
) -> impl gcu_core::driver::Driver + gcu_core::scope::InstanceEvidence {
    fake::FakeEvidence::new(driver)
        .await
        .expect("fake test evidence")
}

use crate::{
    args::{DriverArgs, DriverKind, Profile},
    exit::Failure,
};
use gcu_core::driver::Driver;
use gcu_host_protocol::{
    codec,
    scope::{Ack, Control, MAX_CONTROL_FRAME},
    transport::BoxStream,
    Error,
};
pub(crate) use relay::run as relay;
use state::{Clock, Machine, Monotonic, Phase};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[cfg(test)]
pub(crate) use data::admit_data;
pub(crate) use plane::DataPlane;

/// Encodes one control ACK. S3 writes the bare record. The scoped helper adds
/// its process instance. The closure re-reads facts; it does not wait.
pub(crate) type AckEncoder = Arc<dyn Fn(&Ack) -> Result<Vec<u8>, Error> + Send + Sync>;

/// Import before configuration, workers, or any other child can be launched.
pub(crate) async fn run(
    args: &DriverArgs,
    profile: Profile,
    mode: Option<String>,
) -> Result<u8, Failure> {
    // Windows private-pipe runtime proof: not_run (ADR 0017 S3)
    if cfg!(windows) {
        return Err(Failure::arguments(
            "Scoped engine unsupported on this platform (Windows private-pipe runtime: not_run)",
        ));
    }
    let control = gcu_host_protocol::scope::launch::inherited()?;
    if args.driver != Some(DriverKind::Fake) {
        return Err(Failure::arguments(
            "Scoped bootstrap currently requires --driver fake (S2/S4 not_run)",
        ));
    }
    let plan = crate::assemble::decide(crate::assemble::production(
        args,
        crate::assemble::Entry::InProcess,
    ));
    let crate::assemble::Plan::Fake { scene, .. } = plan else {
        return Err(Failure::arguments("Scoped fake driver is unavailable"));
    };
    let driver = gcu_driver_fake::FakeDriver::from_scene(&scene)
        .and_then(|d| {
            d.with_image_digest([0x47; 32]).map_err(|_| {
                gcu_driver_fake::scene::SceneError::Invalid("fake identity unavailable")
            })
        })
        .map_err(|_| Failure::arguments("Unknown fake scope scene"))?;
    let serving = driver.clone();
    let plane = DataPlane::new();
    let data_plane = plane.clone();
    run_linked(
        control,
        driver,
        Machine::new(Monotonic::default()),
        move |lease, children| {
            data::serve(serving, lease, children, profile.into(), mode, data_plane)
        },
        || Ok(()),
        LinkCfg {
            first: First::Read,
            encoder: bare_encoder(),
            plane,
            label: ControlLabel::Engine,
        },
    )
    .await
}

#[cfg(test)]
async fn run_controlled<C, F, D>(
    control: BoxStream,
    driver: gcu_driver_fake::FakeDriver,
    machine: Machine<C>,
    serve: F,
) -> Result<u8, Failure>
where
    C: Clock,
    F: FnOnce(gcu_core::scope::ScopeLease, crate::script_children::Children) -> D,
    D: std::future::Future<Output = Result<(), Failure>>,
{
    run_linked(
        control,
        driver,
        machine,
        serve,
        || Ok(()),
        LinkCfg {
            first: First::Read,
            encoder: bare_encoder(),
            plane: DataPlane::new(),
            label: ControlLabel::Engine,
        },
    )
    .await
}

enum First {
    /// Read the install frame from the control stream.
    Read,
    /// The caller already admitted this install frame.
    Ready(Control),
}

struct LinkCfg {
    first: First,
    encoder: AckEncoder,
    plane: DataPlane,
    /// Which fixed sentence a control failure uses.
    label: ControlLabel,
}

/// S3 keeps its host-auth sentence. The scoped helper says only `ScopeDenied`.
#[derive(Clone, Copy)]
enum ControlLabel {
    Engine,
    Helper,
}

fn control_failure(error: Error, label: ControlLabel) -> Failure {
    match label {
        ControlLabel::Helper => Failure::arguments("ScopeDenied"),
        ControlLabel::Engine => Failure::from(error),
    }
}

fn bare_encoder() -> AckEncoder {
    Arc::new(|ack: &Ack| serde_json::to_vec(ack).map_err(|_| Error::Invalid))
}

/// Install plus the peer check, ACK encoder, and data latch for one helper.
pub(crate) struct ReadyLink<P> {
    /// Frame already admitted by the helper. Not read from the stream again.
    pub(crate) install: Control,
    /// Re-checked before every apply and before the data plane opens.
    pub(crate) peer: P,
    /// Writes the ACK. A scoped encoder re-reads the helper instance.
    pub(crate) encoder: AckEncoder,
    /// Stays closed until the ACK write finishes and the peer check passes.
    pub(crate) plane: DataPlane,
}

/// Drive one already-checked install through the same state machine.
///
/// `peer` runs before every apply and before the data plane opens. A failure
/// revokes and releases input. The data latch opens only after the ACK bytes
/// are flushed. The clock is the process monotonic clock, including paused
/// tests. Dropping the future revokes and schedules release.
pub(crate) async fn run_ready<F, D, P>(
    control: BoxStream,
    driver: gcu_driver_fake::FakeDriver,
    serve: F,
    link: ReadyLink<P>,
) -> Result<u8, Failure>
where
    F: FnOnce(gcu_core::scope::ScopeLease, crate::script_children::Children) -> D,
    D: std::future::Future<Output = Result<(), Failure>>,
    P: FnMut() -> Result<(), Error>,
{
    run_linked(
        control,
        driver,
        Machine::new(Monotonic::default()),
        serve,
        link.peer,
        LinkCfg {
            first: First::Ready(link.install),
            encoder: link.encoder,
            plane: link.plane,
            label: ControlLabel::Helper,
        },
    )
    .await
}

/// Release input when the handshake never became Active.
///
/// The label is fixed. No data plane is opened. The release wait is the
/// existing native cap. Dropping the future does not skip the call once it
/// has started.
pub(crate) async fn release_unbound(
    driver: gcu_driver_fake::FakeDriver,
    plane: DataPlane,
) -> Result<u8, Failure> {
    plane.close();
    let _ = tokio::time::timeout(
        gcu_core::session::NATIVE_CALL_TIMEOUT,
        driver.release_all_input(),
    )
    .await;
    Err(Failure::arguments("ScopeDenied"))
}

async fn run_linked<C, F, D, P>(
    mut control: BoxStream,
    driver: gcu_driver_fake::FakeDriver,
    machine: Machine<C>,
    serve: F,
    mut peer: P,
    cfg: LinkCfg,
) -> Result<u8, Failure>
where
    C: Clock,
    F: FnOnce(gcu_core::scope::ScopeLease, crate::script_children::Children) -> D,
    D: std::future::Future<Output = Result<(), Failure>>,
    P: FnMut() -> Result<(), Error>,
{
    let plane = cfg.plane.clone();
    let label = cfg.label;
    let mut owner = EngineOwner {
        machine,
        children: crate::script_children::Children::default(),
        release: Some(driver.clone()),
        plane: plane.clone(),
    };
    let children = owner.children.clone();
    let machine = &mut owner.machine;
    let deny = move |error: Error| control_failure(error, label);
    let result: Result<(), Failure> = async {
        // No listener or script worker before Install and a flushed ACK.
        peer().map_err(deny)?;
        let install = match cfg.first {
            First::Read => tokio::time::timeout(
                Duration::from_secs(5),
                codec::read_frame_limited(&mut control, MAX_CONTROL_FRAME),
            )
            .await
            .map_err(|_| deny(Error::Timeout))?
            .map_err(deny)?
            .ok_or_else(|| deny(Error::Disconnected))?,
            First::Ready(frame) => frame,
        };
        peer().map_err(deny)?;
        let ack = machine
            .apply(install)
            .map_err(deny)?
            .ok_or_else(|| deny(Error::Invalid))?;
        let lease = machine.lease().ok_or_else(|| deny(Error::Invalid))?;
        let bytes = (cfg.encoder)(&ack).map_err(deny)?;
        // Peer and lease stay watched while this ACK is written. Data stays shut.
        write_acked(&mut control, &bytes, machine, &mut peer)
            .await
            .map_err(deny)?;
        // The latch must still be closed, and a closed request is not an effect.
        if plane.is_open() || plane.effects() != 0 || plane.request().is_ok() {
            return Err(Failure::arguments("ScopeDenied"));
        }
        plane.open_generation(ack.generation);
        let encoder = cfg.encoder;
        // Always poll control before data. Data completion cannot select an
        // unscoped fallback; this process ends on the same revocation.
        tokio::select! {
            biased;
            result = watch(&mut *machine, &mut control, &mut peer, &encoder, &plane) => {
                result.map_err(deny)
            }
            result = serve(lease, children) => result,
        }
    }
    .await;
    plane.close();
    machine.revoke();
    drop(control);
    // Supervisor death closes control; drain children here before engine exit.
    let children_reaped = owner.children.close();
    // Initial parse/ACK failure also reaches the awaited release barrier.
    // Independent of scope and Stop; never take the router action mutex.
    let _ = tokio::time::timeout(
        gcu_core::session::NATIVE_CALL_TIMEOUT,
        driver.release_all_input(),
    )
    .await;
    owner.release = None;
    if !children_reaped {
        return Err(Failure::internal("Scope worker cleanup failed"));
    }
    result.map(|_| 0)
}

/// How often an Active helper re-reads the peer while a control frame is
/// still arriving. This is not the Stop heartbeat. A failure drops the
/// in-flight read and does not resume it.
const PEER_RECHECK: Duration = Duration::from_millis(50);

async fn watch<C, P>(
    machine: &mut Machine<C>,
    control: &mut BoxStream,
    peer: &mut P,
    encoder: &AckEncoder,
    plane: &DataPlane,
) -> Result<(), Error>
where
    C: Clock,
    P: FnMut() -> Result<(), Error>,
{
    while machine.phase() == Phase::Active {
        // Check the peer before blocking, again while the same read is in
        // progress, and again on a complete frame. A peer failure or a lease
        // timeout drops that read. It is never resumed.
        if let Err(error) = peer() {
            machine.revoke();
            return Err(error);
        }
        if machine.remaining().is_zero() {
            machine.revoke();
            return Err(Error::Disconnected);
        }
        // Drop the read before any later write. A cancelled read is not resumed.
        let frame = {
            let pending = codec::read_frame_limited::<_, Control>(control, MAX_CONTROL_FRAME);
            tokio::pin!(pending);
            loop {
                let lease_wait = tokio::time::sleep(machine.remaining());
                let peer_wait = tokio::time::sleep(PEER_RECHECK);
                tokio::select! {
                    biased;
                    result = &mut pending => break result,
                    _ = lease_wait => {
                        machine.revoke();
                        return Err(Error::Disconnected);
                    }
                    _ = peer_wait => {
                        if let Err(error) = peer() {
                            machine.revoke();
                            return Err(error);
                        }
                        if machine.remaining().is_zero() {
                            machine.revoke();
                            return Err(Error::Disconnected);
                        }
                    }
                }
            }
        };
        let frame = match frame {
            Ok(Some(frame)) => frame,
            _ => {
                machine.revoke();
                return Err(Error::Disconnected);
            }
        };
        if let Err(error) = peer() {
            machine.revoke();
            return Err(error);
        }
        let before = machine.generation();
        if let Some(ack) = machine.apply(frame)? {
            // Rebind publishes the new generation inside apply. Suspend data
            // before the first await so a fresh connection cannot arm it.
            let rebound = before != Some(ack.generation);
            if rebound {
                plane.suspend();
            }
            let bytes = match encoder(&ack) {
                Ok(bytes) => bytes,
                Err(error) => {
                    machine.revoke();
                    return Err(error);
                }
            };
            if write_acked(control, &bytes, machine, peer).await.is_err() {
                machine.revoke();
                return Err(Error::Disconnected);
            }
            if rebound {
                plane.open_generation(ack.generation);
            }
        }
    }
    Ok(())
}

/// Write one ACK while the peer and the lease deadline stay in force.
///
/// The write future is pinned so a pending flush is not restarted. A peer
/// failure or a zero remaining lease returns without opening data. Memory for
/// the check itself. The write keeps the caller's budget.
async fn write_acked<C, P>(
    control: &mut BoxStream,
    bytes: &[u8],
    machine: &Machine<C>,
    peer: &mut P,
) -> Result<(), Error>
where
    C: Clock,
    P: FnMut() -> Result<(), Error>,
{
    peer()?;
    let write = write_payload(control, bytes, machine.remaining());
    tokio::pin!(write);
    loop {
        if machine.remaining().is_zero() {
            return Err(Error::Disconnected);
        }
        let lease_wait = tokio::time::sleep(machine.remaining());
        let peer_wait = tokio::time::sleep(PEER_RECHECK);
        tokio::select! {
            biased;
            result = &mut write => {
                result?;
                peer()?;
                if machine.remaining().is_zero() {
                    return Err(Error::Disconnected);
                }
                return Ok(());
            }
            _ = lease_wait => return Err(Error::Disconnected),
            _ = peer_wait => {
                peer()?;
                if machine.remaining().is_zero() {
                    return Err(Error::Disconnected);
                }
            }
        }
    }
}

async fn write_payload(
    control: &mut BoxStream,
    bytes: &[u8],
    budget: Duration,
) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() > MAX_CONTROL_FRAME {
        return Err(Error::Invalid);
    }
    let write = async {
        control
            .write_all(&(bytes.len() as u32).to_be_bytes())
            .await?;
        control.write_all(bytes).await?;
        control.flush().await?;
        Ok(())
    };
    match tokio::time::timeout(budget, write).await {
        Ok(result) => result,
        Err(_) => Err(Error::Timeout),
    }
}

// Cancellation uses the same sole state writer, then schedules release.
// The ordinary path awaits release and disarms this fallback.
struct EngineOwner<C: Clock> {
    machine: Machine<C>,
    children: crate::script_children::Children,
    release: Option<gcu_driver_fake::FakeDriver>,
    plane: DataPlane,
}
impl<C: Clock> Drop for EngineOwner<C> {
    fn drop(&mut self) {
        self.machine.revoke();
        self.plane.close();
        self.children.close();
        if let (Some(driver), Ok(runtime)) =
            (self.release.take(), tokio::runtime::Handle::try_current())
        {
            runtime.spawn(async move {
                let _ = tokio::time::timeout(
                    gcu_core::session::NATIVE_CALL_TIMEOUT,
                    driver.release_all_input(),
                )
                .await;
            });
        }
    }
}

#[cfg(test)]
mod runtime_tests;
