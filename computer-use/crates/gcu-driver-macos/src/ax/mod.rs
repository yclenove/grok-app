//! Accessibility tree for macOS.
//!
//! [`source::AxSource`] is the node boundary. [`tree::build_raw_tree`] is pure
//! and runs on every platform against [`source::FakeAxSource`]. The live
//! implementation uses `accessibility-sys` on a dedicated thread; see
//! `crate::real` when this crate is built for macOS.
//!
//! Reads are bounded by a 2s walk, 5000 nodes, and a 20KB text cap. Cancellation
//! is observed between node calls. An in-flight native call can still finish
//! after the caller has returned; reads report `maybe_executed = false`.

pub mod events;
pub mod owned;
pub mod roles;
pub mod sensitive;
pub mod source;
pub mod toggles;
pub mod tree;

#[cfg(target_os = "macos")]
pub use events::ObserverPool;
pub use events::{parse_window_pid, AxEventSource, EventHub, FakeAxEvents, RunLoopControl};
pub use owned::{CountRelease, Owned, Releaser};
pub use roles::{map_ax_role, role_table, MappedAxRole, RoleRow};
pub use sensitive::is_terminal_like;
pub use source::{select_children, AxHandle, AxSource, AxStateBits, FakeAxNode, FakeAxSource};
pub use toggles::{
    needs_enhanced_accessibility, AccessibilityToggles, ErasedToggle, FakeToggleStore, ToggleName,
    ToggleStore,
};
pub use tree::{
    build_raw_tree, map_ax_states, MAX_NODES_CEILING, RENDER_BYTE_CAP, TREE_BUILD_TIMEOUT,
};
