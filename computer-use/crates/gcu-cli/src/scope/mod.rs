//! Private fake scope engine. No native/helper fallback exists on this route.
mod data;
mod fake;
mod relay;
mod state;

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
use std::time::Duration;

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
    run_controlled(
        control,
        driver,
        Machine::new(Monotonic::default()),
        move |lease, children| data::serve(serving, lease, children, profile.into(), mode),
    )
    .await
}

async fn run_controlled<C, F, D>(
    mut control: BoxStream,
    driver: gcu_driver_fake::FakeDriver,
    machine: Machine<C>,
    serve: F,
) -> Result<u8, Failure>
where
    C: Clock,
    F: FnOnce(gcu_core::scope::ScopeLease, crate::script_children::Children) -> D,
    D: std::future::Future<Output = Result<(), Failure>>,
{
    let mut owner = EngineOwner {
        machine,
        children: crate::script_children::Children::default(),
        release: Some(driver.clone()),
    };
    let children = owner.children.clone();
    let machine = &mut owner.machine;
    let result: Result<(), Failure> = async {
        // No LocalHost, listener or script worker before Install and flushed ACK.
        let install: Control = tokio::time::timeout(
            Duration::from_secs(5),
            codec::read_frame_limited(&mut control, MAX_CONTROL_FRAME),
        )
        .await
        .map_err(|_| Error::Timeout)??
        .ok_or(Error::Disconnected)?;
        let ack = machine.apply(install)?.ok_or(Error::Invalid)?;
        let lease = machine.lease().ok_or(Error::Invalid)?;
        tokio::time::timeout(machine.remaining(), codec::write_frame(&mut control, &ack))
            .await
            .map_err(|_| Error::Timeout)??;
        // Always poll control before data. Data completion closes control and
        // terminates this process; it cannot select an unscoped fallback.
        tokio::select! {
            biased;
            result = watch(&mut *machine, &mut control) => result.map_err(Failure::from),
            result = serve(lease, children) => result,
        }
    }
    .await;
    machine.revoke();
    drop(control);
    // No existing parent-death signal mechanism is used in this repository.
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

async fn watch<C: Clock>(machine: &mut Machine<C>, control: &mut BoxStream) -> Result<(), Error> {
    while machine.phase() == Phase::Active {
        // Never cancel and retry a partially read control frame. Timeout is a
        // terminal lease loss; EOF/parse/IO failure use the same transition.
        let next = tokio::time::timeout(
            machine.remaining(),
            codec::read_frame_limited::<_, Control>(control, MAX_CONTROL_FRAME),
        )
        .await;
        let frame = match next {
            Ok(Ok(Some(frame))) => frame,
            _ => {
                machine.revoke();
                return Err(Error::Disconnected);
            }
        };
        if let Some(ack) = machine.apply(frame)? {
            if !matches!(
                tokio::time::timeout(
                    machine.remaining(),
                    codec::write_frame::<_, Ack>(control, &ack)
                )
                .await,
                Ok(Ok(()))
            ) {
                machine.revoke();
                return Err(Error::Disconnected);
            }
        }
    }
    Ok(())
}

// Cancellation uses the same sole state writer, then schedules release.
// The ordinary path awaits release and disarms this fallback.
struct EngineOwner<C: Clock> {
    machine: Machine<C>,
    children: crate::script_children::Children,
    release: Option<gcu_driver_fake::FakeDriver>,
}
impl<C: Clock> Drop for EngineOwner<C> {
    fn drop(&mut self) {
        self.machine.revoke();
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
