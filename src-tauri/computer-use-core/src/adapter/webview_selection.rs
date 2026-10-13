//! Host-retained pre-approval receipt. Never an execution target or wire DTO.
use super::{ComputerUseAdapter, ScopeCandidate};
use std::fmt;
use std::sync::Arc;

pub trait WebViewScopeReceipt: Send + Sync {
    fn require_current(&self) -> Result<(), String>;
    /// Only called by the original Host resource reservation, after its run
    /// exists behind the pending consent gate. No observation/input occurs.
    fn bind(
        &self,
        executor: &dyn ComputerUseAdapter,
        session: &str,
        run: &str,
    ) -> Result<ScopeCandidate, String>;
}

#[derive(Clone)]
pub struct WebViewSelectedIdentity {
    pub target_id: String,
    pub native_tab_id: String,
    pub document_generation: u64,
    pub receipt: Arc<dyn WebViewScopeReceipt>,
}

impl fmt::Debug for WebViewSelectedIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebViewSelectedIdentity")
            .field("target_id", &self.target_id)
            .field("native_tab_id", &self.native_tab_id)
            .field("document_generation", &self.document_generation)
            .finish_non_exhaustive()
    }
}
impl PartialEq for WebViewSelectedIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.target_id == other.target_id
            && self.native_tab_id == other.native_tab_id
            && self.document_generation == other.document_generation
            && Arc::ptr_eq(&self.receipt, &other.receipt)
    }
}
impl Eq for WebViewSelectedIdentity {}
