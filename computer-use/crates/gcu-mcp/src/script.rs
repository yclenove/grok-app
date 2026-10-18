//! Injected script runner. This crate does not link the JS engine.

use gcu_core::{action::ActionRouter, session::Session};
use gcu_protocol::{ComputerScriptParams, ToolError};
use std::{future::Future, pin::Pin};

/// Text returned to the model, plus an optional tool error.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptBody {
    /// Script output. No handles, paths, or tokens.
    pub output: String,
    /// Failure. `None` when the script finished cleanly.
    pub error: Option<ToolError>,
}

/// Runs one script through the same router entry as a single tool and batch.
///
/// Blocking: the returned future holds the session queue when it calls the
/// router. Timeout: implementations cap the script at 30s; each native call
/// stays at 5s. Cancellation: Stop, session close, and dropping the future.
/// Unknown router results are not replayed.
pub trait ScriptRunner: Send + Sync {
    /// Execute `params.code`.
    fn run<'a>(
        &'a self,
        router: &'a ActionRouter,
        session: &'a Session,
        call_id: &'a str,
        params: ComputerScriptParams,
    ) -> Pin<Box<dyn Future<Output = ScriptBody> + Send + 'a>>;
}
