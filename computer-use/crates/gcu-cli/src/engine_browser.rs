//! `engine.browser.sync`. Does not call back into the Host.

use gcu_host_protocol::{messages as w, server::Peer};
use gcu_protocol::builtin_browser::{
    current_platform, l1_open, ApplyFault, BrowserAck, BrowserSync,
};
use serde_json::Value;

pub(crate) async fn sync(request: &w::Request, peer: &Peer) -> Result<Value, w::RpcError> {
    let params: BrowserSync = serde_json::from_value(request.params.clone())
        .map_err(|_| w::RpcError::new(-32602, "invalid parameters"))?;
    if params
        .browser
        .as_ref()
        .is_some_and(|window| !gcu_protocol::builtin_browser::window_valid(window))
    {
        return Err(w::RpcError::new(-32602, "invalid parameters"));
    }
    let proven = l1_open(current_platform());
    match gcu_mcp::browser_tool::apply_sync(params, proven).await {
        Ok(revision) => serde_json::to_value(BrowserAck { revision })
            .map_err(|_| w::RpcError::new(-32603, "internal error")),
        Err(ApplyFault::Unavailable) => Err(w::RpcError::new(-32010, "browser_unavailable")),
        Err(ApplyFault::Stale) => Err(w::RpcError::new(-32013, "browser_state_stale")),
        Err(ApplyFault::Invalid) => Err(w::RpcError::new(-32602, "invalid parameters")),
        Err(ApplyFault::Saturated) => {
            peer.close();
            Err(w::RpcError::new(-32603, "internal error"))
        }
    }
}
