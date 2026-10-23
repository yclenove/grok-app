//! Injected script runner. This crate does not link the JS engine.

use gcu_core::{action::ActionRouter, session::Session};
use gcu_protocol::{ComputerScriptParams, ToolError};
use std::{future::Future, pin::Pin};

/// Text returned to the model, plus an optional tool error.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptBody {
    /// Script output. No handles, paths, or tokens.
    pub output: String,
    /// Script failure and any independent parent-side delivery evidence.
    pub error: Option<ScriptError>,
}

/// Error origin retained in the trusted parent, never decoded from worker text.
// Built only on failure paths and moved once into the reply. Boxing one variant would not
// help (`Untrusted` is already 600+ bytes) and every variant is matched by value in the worker.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptError {
    /// Worker exceptions, timeouts and failures without native-delivery evidence.
    Untrusted(ToolError),
    /// A parent-dispatched router reply proved uncertain native delivery.
    /// Only the trusted runner may construct this; worker Finish frames cannot.
    NativeDelivery {
        /// Worker error for a still-authorized publication, if any.
        script: Option<ToolError>,
        /// Parent router's error, retained independently of script exceptions.
        native: ToolError,
    },
}

impl ScriptError {
    pub(crate) fn error(&self) -> Option<&ToolError> {
        match self {
            Self::Untrusted(error) => Some(error),
            Self::NativeDelivery { script, .. } => script.as_ref(),
        }
    }

    pub(crate) fn native(&self) -> Option<&ToolError> {
        match self {
            Self::NativeDelivery { native, .. } => Some(native),
            Self::Untrusted(_) => None,
        }
    }
}

impl From<ToolError> for ScriptError {
    fn from(error: ToolError) -> Self {
        Self::Untrusted(error)
    }
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
