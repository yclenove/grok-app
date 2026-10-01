//! Host-only native dialog-parent ownership, never a model-supplied string.
use crate::session_grants::AuthorizationTicket;

/// Implementations retain the original UI owner. Revocation is synchronous;
/// completion may only become true after that owner's export was released.
/// No deadline, dropped waiter, or remote portal Closed signal proves completion.
/// A preparation returning Err must have created no export, or have already
/// released it on its original owner. Returning Ok transfers that obligation.
pub trait NativeParent: Send + Sync {
    fn authorization(&self) -> &AuthorizationTicket;
    fn handle(&self) -> Result<Option<String>, String>;
    fn is_revoked(&self) -> bool;
    fn request_close(&self);
    fn is_closed(&self) -> bool;
}

/// A dropped reply is not proof that the queued UI callback created nothing.
/// Keep the original selection occupied if its receipt is lost. The registry
/// may revoke/expire admission, but cannot certify retirement from this signal.
pub async fn retain_parent_dispatch<P: NativeParent>(
    receive: tokio::sync::oneshot::Receiver<Result<P, String>>,
) -> Result<P, String> {
    match receive.await {
        Ok(result) => result,
        Err(_) => std::future::pending().await,
    }
}
