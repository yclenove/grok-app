//! `AtspiSource` trait and testing `FakeSource` implementation.

use async_trait::async_trait;
use gcu_core::{
    driver::DriverError,
    model::{NativeHandle, NodeAction, NodeValue, Rect, TextExtras},
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};

/// Abstraction over an AT-SPI node source, decoupling raw tree construction from live D-Bus.
#[async_trait]
pub trait AtspiSource: Send + Sync {
    /// Opaque node representation within the source.
    type Node: Clone + Send + Sync + 'static;

    /// Get immediate children of a node.
    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>, DriverError>;
    /// Get the AT-SPI role.
    async fn role(&self, node: &Self::Node) -> Result<atspi::Role, DriverError>;
    /// Get the accessible name.
    async fn name(&self, node: &Self::Node) -> Result<Option<String>, DriverError>;
    /// Get the accessible description.
    async fn description(&self, node: &Self::Node) -> Result<Option<String>, DriverError>;
    /// Get help text.
    async fn help(&self, node: &Self::Node) -> Result<Option<String>, DriverError>;
    /// Get native identifier (accessible ID).
    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>, DriverError>;
    /// Get node attributes dictionary.
    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>, DriverError>;
    /// Get accessibility state set.
    async fn states(&self, node: &Self::Node) -> Result<atspi::StateSet, DriverError>;
    /// Get current control value if available.
    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>, DriverError>;
    /// Get physical pixel extents.
    async fn extents(&self, node: &Self::Node) -> Result<Option<Rect>, DriverError>;
    /// Get semantic and advertised native actions.
    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>, DriverError>;
    /// Get text extras (selection, visible range, extended text bounded by limit).
    async fn text_extras(
        &self,
        node: &Self::Node,
        text_limit: Option<usize>,
    ) -> Result<Option<TextExtras>, DriverError>;
    /// Get URL for web/link nodes.
    async fn url(&self, node: &Self::Node) -> Result<Option<String>, DriverError>;
    /// Create a driver-opaque handle for the node.
    fn handle(&self, node: &Self::Node) -> NativeHandle;
}

/// In-memory fake node representation for testing.
#[derive(Debug, Clone)]
pub struct FakeNode {
    /// Node identifier.
    pub id: u64,
    /// AT-SPI role.
    pub role: atspi::Role,
    /// Accessible name.
    pub name: Option<String>,
    /// Accessible description.
    pub description: Option<String>,
    /// Help text.
    pub help: Option<String>,
    /// Native accessible identifier.
    pub identifier: Option<String>,
    /// Key-value attributes.
    pub attributes: HashMap<String, String>,
    /// AT-SPI state set.
    pub states: atspi::StateSet,
    /// Control value.
    pub value: Option<NodeValue>,
    /// Bounding rectangle in physical pixels.
    pub extents: Option<Rect>,
    /// Advertised actions.
    pub actions: Vec<NodeAction>,
    /// Text details.
    pub text_extras: Option<TextExtras>,
    /// Link URL.
    pub url: Option<String>,
    /// Child nodes.
    pub children: Vec<FakeNode>,
}

impl Default for FakeNode {
    fn default() -> Self {
        Self {
            id: 0,
            role: atspi::Role::Unknown,
            name: None,
            description: None,
            help: None,
            identifier: None,
            attributes: HashMap::new(),
            states: atspi::StateSet::default(),
            value: None,
            extents: None,
            actions: Vec::new(),
            text_extras: None,
            url: None,
            children: Vec::new(),
        }
    }
}

/// Pure in-memory `AtspiSource` for logic tests without D-Bus.
#[derive(Debug, Clone)]
pub struct FakeSource {
    /// Simulated per-call latency.
    pub delay: Duration,
    /// Specific node ID to fail with a native driver error.
    pub error_on_id: Option<u64>,
    /// Counts name, description, help, value, text, and url reads.
    ///
    /// Children, role, and geometry are not counted. Tests use this to prove a
    /// scoped tree refusal did not read node content.
    pub content_reads: Arc<AtomicUsize>,
}

impl Default for FakeSource {
    fn default() -> Self {
        Self {
            delay: Duration::ZERO,
            error_on_id: None,
            content_reads: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl AtspiSource for FakeSource {
    type Node = FakeNode;

    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>, DriverError> {
        if self.delay > Duration::ZERO {
            tokio::time::sleep(self.delay).await;
        }
        if self.error_on_id == Some(node.id) {
            return Err(DriverError::Native {
                message: "injected fake error".into(),
                maybe_executed: false,
            });
        }
        Ok(node.children.clone())
    }

    async fn role(&self, node: &Self::Node) -> Result<atspi::Role, DriverError> {
        Ok(node.role)
    }

    async fn name(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        self.note_content();
        Ok(node.name.clone())
    }

    async fn description(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        self.note_content();
        Ok(node.description.clone())
    }

    async fn help(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        self.note_content();
        Ok(node.help.clone())
    }

    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        Ok(node.identifier.clone())
    }

    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>, DriverError> {
        Ok(node.attributes.clone())
    }

    async fn states(&self, node: &Self::Node) -> Result<atspi::StateSet, DriverError> {
        Ok(node.states)
    }

    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>, DriverError> {
        self.note_content();
        Ok(node.value.clone())
    }

    async fn extents(&self, node: &Self::Node) -> Result<Option<Rect>, DriverError> {
        Ok(node.extents)
    }

    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>, DriverError> {
        Ok(node.actions.clone())
    }

    async fn text_extras(
        &self,
        node: &Self::Node,
        _text_limit: Option<usize>,
    ) -> Result<Option<TextExtras>, DriverError> {
        self.note_content();
        Ok(node.text_extras.clone())
    }

    async fn url(&self, node: &Self::Node) -> Result<Option<String>, DriverError> {
        self.note_content();
        Ok(node.url.clone())
    }

    fn handle(&self, node: &Self::Node) -> NativeHandle {
        NativeHandle::new(node.id)
    }
}

impl FakeSource {
    fn note_content(&self) {
        self.content_reads.fetch_add(1, Ordering::Relaxed);
    }
}
