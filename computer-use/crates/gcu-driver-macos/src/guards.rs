//! Driver wiring for event safety and dynamically reported guard capabilities.

use crate::{ax, MacosDriver};
use gcu_core::driver::*;
use gcu_protocol::capabilities::*;
use std::{collections::BTreeMap, sync::Arc};

impl MacosDriver {
    /// Install the event backend and independent Stop release service.
    /// Missing safety sources disable foreground; startup failure latches Stop.
    pub fn inject_events(&mut self, api: Arc<dyn crate::input::EventApi>) {
        crate::input::apply_probe(&mut self.caps, &api.probe());
        self.guarded_caps = std::array::from_fn(|bits| {
            let mut caps = self.caps.clone();
            caps.user_activity_guard = bits & 1 != 0;
            if bits & 2 == 0 {
                caps.delivery.foreground = ForegroundCapability::Unavailable;
            }
            caps
        });
        self.guard_runtime = api.guards().and_then(|guards| {
            match crate::input::watchdog::GuardRuntime::start(
                &api,
                self.ledger.clone(),
                guards.clone(),
            ) {
                Ok(runtime) => Some(runtime),
                Err(_) => {
                    guards.request_stop();
                    tracing::warn!("macos Stop release service failed to start");
                    None
                }
            }
        });
        if let Ok(mut slot) = self.input.lock() {
            *slot = Some(api);
        }
    }

    pub(crate) fn event_api(&self) -> Option<Arc<dyn crate::input::EventApi>> {
        self.input.lock().ok().and_then(|slot| slot.clone())
    }

    /// Host adapter reads the same activity source and Stop latch as the driver.
    /// No model tool or turn/session lifecycle may clear this latch.
    pub fn input_guards(&self) -> Option<Arc<crate::input::hotkey::Guards>> {
        self.event_api().and_then(|api| api.guards())
    }

    pub(crate) fn guard_capabilities(&self) -> &Capabilities {
        let Some(guards) = self.input_guards() else {
            return &self.guarded_caps[0];
        };
        let activity = guards.ms_since_physical_input().is_some();
        let foreground = guards.stop_available() && activity && !guards.is_stopped();
        &self.guarded_caps[usize::from(activity) | (usize::from(foreground) << 1)]
    }

    pub(crate) async fn perform_events(
        &self,
        req: &DriverAction,
        cancel: &Cancel,
    ) -> Result<Delivery> {
        let Some(api) = self.event_api() else {
            let win = action_window(req)?.clone();
            return self
                .call(DRIVER_DEADLINE, Some(cancel), false, move |state| {
                    state.validate_window(&win)?;
                    Err(DriverError::CapabilityUnavailable)
                })
                .await;
        };
        let clock = api
            .guards()
            .map(|guards| guards.clock.clone())
            .unwrap_or_else(crate::input::clock::runtime_clock);
        let started = clock.now();
        let win = action_window(req)?.clone();
        let work = self.call(DRIVER_DEADLINE, Some(cancel), false, move |state| {
            state.prepare_input(&win)
        });
        let target = if let Some(guards) = api.guards() {
            tokio::select! {
                biased;
                _ = guards.stopped() => return Err(DriverError::Cancelled { maybe_executed: false }),
                result = work => result?,
            }
        } else {
            work.await?
        };
        let left = DRIVER_DEADLINE.saturating_sub(clock.now().saturating_sub(started));
        let _armed = crate::input::arm(api.as_ref(), Arc::clone(&self.evidence), target.window_id)?;
        crate::input::deliver(
            Arc::clone(&api),
            self.ledger.clone(),
            &target,
            req,
            cancel,
            left,
        )
        .await
    }
}

fn action_window(action: &DriverAction) -> Result<&WindowRef> {
    match action {
        DriverAction::PointerClick { win, .. }
        | DriverAction::PointerScroll { win, .. }
        | DriverAction::PointerDrag { win, .. }
        | DriverAction::TypeText { win, .. }
        | DriverAction::PressKeys { win, .. }
        | DriverAction::Activate { win } => Ok(win),
        _ => Err(DriverError::CapabilityUnavailable),
    }
}

pub(crate) fn discovery_capabilities() -> Capabilities {
    Capabilities {
        platform: Platform::Macos,
        version: "macOS 14+; AX tree (P1-03); screenshot (P1-04); semantic actions (P1-05); targeted events (P1-06)".into(),
        observe: ObserveCapabilities {
            tree: true,
            screenshot: true,
            events: true,
        },
        delivery: DeliveryCapabilities {
            accessibility: true,
            targeted_pointer: false,
            targeted_keys: false,
            foreground: ForegroundCapability::Unavailable,
        },
        background_operation: BackgroundOperation::Partial,
        user_activity_guard: false,
        overlay_excluded_from_capture: false,
        private_apis: BTreeMap::new(),
        limits: CapabilityLimits {
            max_tree_nodes: ax::MAX_NODES_CEILING as u32,
            max_image_edge: 2560,
            type_text_max: 0,
        },
    }
}
