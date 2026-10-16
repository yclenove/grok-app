//! Metadata is always safe to log; fixture identities require an explicit debug opt-in.
use super::{cache::Properties, source::NodeId};

pub(crate) fn field(node: NodeId, props: &Properties) {
    if let Some(failure) = &props.read_failure {
        tracing::warn!(target: "gcu_windows_uia", ?node, failure, partial = props.partial, "UIA_FIELD_FAILURE");
        identity(props);
    }
}

pub(crate) fn identity(props: &Properties) {
    // Only the disposable contract launcher enables this. Production logging
    // remains payload-free even if a release process inherits this variable.
    if cfg!(debug_assertions) && std::env::var("GCU_UIA_DIAGNOSTICS").as_deref() == Ok("1") {
        let label = |text: &Option<String>| {
            if props.password {
                "<protected>".into()
            } else {
                text.as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(96)
                    .collect::<String>()
            }
        };
        tracing::info!(target: "gcu_windows_uia", control_type = props.control_type,
            role = ?super::roles::map_role(props.control_type, props.aria_role.as_deref(), props.modal),
            name = ?label(&props.name), automation_id = ?label(&props.identifier),
            runtime_id = ?props.runtime_id, "UIA_NODE_IDENTITY");
    }
}

#[cfg(windows)]
pub(super) fn native_call(
    call: &str,
    started: tokio::time::Instant,
    error: windows::core::Error,
    props: &Properties,
) -> gcu_core::driver::DriverError {
    let message = format!(
        "{call}: HRESULT {:08x}; elapsed_ms={}",
        error.code().0 as u32,
        started.elapsed().as_millis()
    );
    tracing::warn!(target: "gcu_windows_uia", failure = message, "UIA_NATIVE_FAILURE");
    identity(props);
    match super::ffi::native(error) {
        gcu_core::driver::DriverError::Native { maybe_executed, .. } => {
            gcu_core::driver::DriverError::Native {
                message,
                maybe_executed,
            }
        }
        other => other,
    }
}
