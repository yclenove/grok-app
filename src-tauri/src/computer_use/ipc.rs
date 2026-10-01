//! Process-owned endpoint. Transport and authority live in the portable core.
use super::ComputerUseBroker;
pub use grok_computer_use_core::ipc::{spawn, IpcServer};
use grok_computer_use_core::tools::ModelStopHandler;
use parking_lot::Mutex;
use std::sync::{Arc, OnceLock, Weak};

use crate::session_manager::SessionManager;

static ENDPOINT: OnceLock<IpcServer> = OnceLock::new();
static SESSION_MANAGER: OnceLock<Mutex<Weak<SessionManager>>> = OnceLock::new();

fn session_manager_slot() -> &'static Mutex<Weak<SessionManager>> {
    SESSION_MANAGER.get_or_init(|| Mutex::new(Weak::new()))
}

pub(crate) fn register_session_manager(manager: &Arc<SessionManager>) {
    *session_manager_slot().lock() = Arc::downgrade(manager);
}

pub fn endpoint() -> Option<&'static IpcServer> {
    ENDPOINT.get()
}

pub fn ensure(broker: Arc<ComputerUseBroker>) -> Result<&'static IpcServer, String> {
    if let Some(endpoint) = ENDPOINT.get() {
        return Ok(endpoint);
    }
    let stop_handler = session_manager_slot().lock().upgrade().map(|manager| {
        let stop_broker = Arc::clone(&broker);
        Arc::new(
            move |binding: &grok_computer_use_core::ipc::SessionBinding| {
                stop_broker
                    .require_owner(&binding.session_id, &binding.run_id)
                    .map_err(|error| error.to_string())?;
                let plan = manager.fence_computer_use_stop_with_broker(
                    Arc::clone(&stop_broker),
                    &binding.session_id,
                );
                let state = stop_broker
                    .stop_state(&binding.run_id)
                    .map_err(|error| error.to_string())?;
                manager.spawn_fenced_computer_use_stop(binding.session_id.clone(), plan, None);
                Ok(state)
            },
        ) as Arc<ModelStopHandler>
    });
    let server = grok_computer_use_core::ipc::spawn_with_stop_handler(broker, stop_handler)
        .map_err(|e| {
            format!(
                "port_failed: {e}. Repair Grok App loopback IPC. System Node on PATH is not used."
            )
        })?;
    ENDPOINT
        .set(server)
        .map_err(|_| "computer-use endpoint already initialized")?;
    ENDPOINT
        .get()
        .ok_or_else(|| "computer-use endpoint unavailable".into())
}
