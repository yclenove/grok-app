//! Host-owned Playwright worker process. Never looks up Node on PATH.

#[cfg(feature = "computer-use-probe")]
mod handshake;
mod process;
mod process_tree;

#[cfg(all(windows, feature = "computer-use-probe"))]
pub(crate) use process_tree::assign_job as assign_probe_job;

#[cfg(feature = "computer-use-probe")]
mod error_contract;

#[cfg(feature = "computer-use-probe")]
pub use handshake::run_supervisor_gate;
#[cfg(feature = "computer-use-probe")]
pub(crate) use process::test_node_exe;
pub use process::{
    attach_product, ensure_product_attached, product_spawn_request, shutdown_product,
    BrowserSupervisor, SpawnRequest,
};
#[cfg(feature = "computer-use-probe")]
pub(crate) use process_tree::leftover_browser_pids;
#[cfg(feature = "computer-use-probe")]
pub(crate) use process_tree::{
    live_browser_descendant_pids, live_browser_descendants, process_alive, tracked_process_tree,
};

#[cfg(feature = "computer-use-probe")]
pub use error_contract::run_browser_error_contract_gate;
