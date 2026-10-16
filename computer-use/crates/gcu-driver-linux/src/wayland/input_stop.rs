//! Native GlobalShortcuts registration. Interface presence alone never enables input.
use super::input_control::{RegisteredStop, StopControl, StopRegistrar, WATCHDOG_TICK};
use crate::x11::hotkey::StopLatch;
use ashpd::desktop::{
    global_shortcuts::{GlobalShortcuts, NewShortcut},
    CreateSessionOptions, Session,
};
use async_trait::async_trait;
use futures_core::Stream;
use gcu_core::driver::{DriverError, Result, DRIVER_DEADLINE};
use std::{
    future::poll_fn,
    sync::{Arc, Mutex},
    time::Duration,
};

const ID: &str = "gcu-stop";
fn unavailable() -> DriverError {
    DriverError::CapabilityUnavailable
}

/// Registers the default desktop-wide Ctrl+Alt+Shift+Backspace with explicit portal consent.
/// The Host supplies localized copy, owns the persistent latch and retains the returned lease.
pub struct AshpdStopRegistrar;
struct Registration {
    session: Arc<Session<GlobalShortcuts>>,
    control: Arc<StopControl>,
    pump: Mutex<Option<tokio::task::JoinHandle<()>>>,
}
#[async_trait]
impl RegisteredStop for Registration {
    fn control(&self) -> Arc<StopControl> {
        self.control.clone()
    }
    async fn close(&self) -> Result<()> {
        self.control.listener_failed();
        if let Some(pump) = self.pump.lock().unwrap_or_else(|p| p.into_inner()).take() {
            pump.abort();
        }
        tokio::time::timeout(DRIVER_DEADLINE, self.session.close())
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.control.listener_failed();
        if let Some(pump) = self.pump.lock().unwrap_or_else(|p| p.into_inner()).take() {
            pump.abort();
        }
        let session = self.session.clone();
        tokio::spawn(async move {
            if !matches!(
                tokio::time::timeout(DRIVER_DEADLINE, session.close()).await,
                Ok(Ok(()))
            ) {
                tracing::warn!("Stop portal close failed");
            }
        });
    }
}
#[async_trait]
impl StopRegistrar for AshpdStopRegistrar {
    async fn register(
        &self,
        latch: Arc<dyn StopLatch>,
        description: String,
    ) -> Result<Arc<dyn RegisteredStop>> {
        tokio::time::timeout(DRIVER_DEADLINE,async {
            let bus=zbus::Connection::session().await.map_err(|_|unavailable())?;
            let proxy=Arc::new(GlobalShortcuts::with_connection(bus).await.map_err(|_|unavailable())?);
            let session=Arc::new(proxy.create_session(CreateSessionOptions::default()).await.map_err(|_|unavailable())?);
            let registration=Arc::new(Registration {session:session.clone(),control:StopControl::registered(latch),pump:Mutex::new(None)});
            let path=serde_json::to_string(session.as_ref()).map_err(|_|unavailable())?.trim_matches('"').to_owned();
            let monitor_proxy=proxy.clone();let monitor_session=session.clone();let control=registration.control.clone();
            let (ready,subscribed)=tokio::sync::oneshot::channel();
            let (arm,armed)=tokio::sync::oneshot::channel::<String>();
            let pump=tokio::spawn(async move {
                let result=async {
                    let activated=monitor_proxy.receive_activated().await.map_err(|_|unavailable())?;
                    let changed=monitor_proxy.receive_shortcuts_changed().await.map_err(|_|unavailable())?;
                    let closed=monitor_session.receive_closed().await.map_err(|_|unavailable())?;
                    tokio::pin!(activated,changed,closed);
                    ready.send(()).map_err(|_|unavailable())?;
                    let trigger=armed.await.map_err(|_|unavailable())?;
                    loop {
                        // The heartbeat requires a successful portal roundtrip, not just a local
                        // timer. Daemon loss/restart or hung replies invalidate it within 75ms.
                        let probe=async {
                            tokio::time::sleep(WATCHDOG_TICK).await;
                            tokio::time::timeout(Duration::from_millis(50),monitor_proxy.list_shortcuts(&monitor_session,Default::default())).await.map_err(|_|unavailable())?.map_err(|_|unavailable())?.response().map_err(|_|unavailable())
                        };
                        tokio::pin!(probe);
                        loop {
                            tokio::select! {biased;
                                event=poll_fn(|cx|Pin::new(&mut activated).poll_next(cx))=>{
                                    let event=event.ok_or_else(unavailable)?;
                                    if event.session_handle().as_str()==path && event.shortcut_id()==ID {control.stop();return Ok(());}
                                }
                                event=poll_fn(|cx|Pin::new(&mut changed).poll_next(cx))=>{
                                    let event=event.ok_or_else(unavailable)?;
                                    if event.session_handle().as_str()==path && !event.shortcuts().iter().any(|s|s.id()==ID && s.trigger_description()==trigger) {return Err(unavailable());}
                                }
                                _=poll_fn(|cx|Pin::new(&mut closed).poll_next(cx))=>return Err(unavailable()),
                                result=&mut probe=>{
                                    let response=result?;
                                    if response.shortcuts().iter().filter(|s|s.id()==ID && s.trigger_description()==trigger).count()!=1 {return Err(unavailable());}
                                    control.heartbeat();break;
                                }
                            }
                        }
                    }
                }.await;
                if result.is_err() {control.listener_failed();}
            });
            *registration.pump.lock().unwrap_or_else(|p|p.into_inner())=Some(pump);
            subscribed.await.map_err(|_|unavailable())?;
            let response=proxy.bind_shortcuts(&session,&[NewShortcut::new(ID,description).preferred_trigger("CTRL+ALT+SHIFT+BackSpace")],None,Default::default()).await.map_err(|_|unavailable())?.response().map_err(|_|unavailable())?;
            if response.shortcuts().iter().filter(|s|s.id()==ID && !s.trigger_description().is_empty()).count()!=1 {return Err(unavailable());}
            let trigger=response.shortcuts().iter().find(|s|s.id()==ID).ok_or_else(unavailable)?.trigger_description().to_owned();
            arm.send(trigger).map_err(|_|unavailable())?;
            registration.control.heartbeat();
            if !registration.control.available() {return Err(unavailable());}
            Ok(registration as Arc<dyn RegisteredStop>)
        }).await.map_err(|_|DriverError::Timeout {maybe_executed:false})?
    }
}
