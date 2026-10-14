//! Route B Host adapters. Physical activity belongs to engine platform drivers.
mod data;
mod remote;
mod session;
mod stop;
pub use remote::RemoteHost;
pub use session::RemoteSession;
pub use stop::{PhysicalActivity, StopMirror, UnavailableActivity};
