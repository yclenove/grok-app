//! `AxSource` and the in-memory [`FakeAxSource`].
//!
//! Live reads go through a dedicated thread and are cancellable only between
//! calls. Each fake call honors [`FakeAxSource::delay`] so tests can force a
//! timeout or a cancel without a macOS session. A stored error is returned
//! as-is, including `PermissionMissing`.

use async_trait::async_trait;
use gcu_core::{
    driver::DriverError,
    model::{NativeHandle, NodeAction, NodeValue, Rect, TextRange},
};
use std::{collections::HashMap, time::Duration};

use gcu_core::driver::Result;

/// Process-local accessibility identity. It is not an authorization token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxHandle {
    /// Owning process.
    pub pid: u32,
    /// Id assigned by the source for this walk.
    pub id: u64,
}

/// Boolean attributes. `None` means the attribute was absent.
///
/// Missing `enabled` is treated as enabled: the renderer prints "disabled"
/// when [`gcu_core::model::States::ENABLED`] is unset, and AX omits the
/// attribute for ordinary controls. Missing `focused` means not focusable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AxStateBits {
    /// `AXEnabled`.
    pub enabled: Option<bool>,
    /// `AXFocused`. `Some` means the attribute exists.
    pub focused: Option<bool>,
    /// `AXSelected`.
    pub selected: Option<bool>,
    /// `AXExpanded`.
    pub expanded: Option<bool>,
    /// Checked, when the control exposes it.
    pub checked: Option<bool>,
    /// Mixed check state.
    pub mixed: bool,
    /// `AXIsEditable` or a text role.
    pub editable: bool,
    /// `AXValue` is settable.
    pub settable: bool,
    /// The source knows the node is off the visible tree.
    pub offscreen: bool,
    /// `AXModal`.
    pub modal: bool,
    /// `AXElementBusy`.
    pub busy: bool,
    /// Required field, when the source knows it. AX has no standard attribute.
    pub required: bool,
}

/// Node source. Implementations must not run AX calls on the async worker.
///
/// Each method is one unit of work. A live source may answer the whole set
/// from one `AXUIElementCopyMultipleAttributeValues` batch. Cancellation is
/// checked by the caller around the future; a call already inside AX runs
/// until its messaging timeout and then reports `maybe_executed = false`.
#[async_trait]
pub trait AxSource: Send + Sync {
    /// Opaque node. Live sources use an id into a thread-local element map.
    type Node: Clone + Send + Sync + 'static;

    /// Children. Prefer visible children, then contents, then `AXChildren`.
    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>>;
    /// String attributes already read for this node. May be empty.
    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>>;
    /// State bits. Absence is `None`, not an error.
    async fn states(&self, node: &Self::Node) -> Result<AxStateBits>;
    /// `AXRole` string. Empty becomes `AXUnknown` in the mapper.
    async fn role(&self, node: &Self::Node) -> Result<String>;
    /// `AXSubrole`.
    async fn subrole(&self, node: &Self::Node) -> Result<Option<String>>;
    /// `AXTitle`, used as the accessible name.
    async fn title(&self, node: &Self::Node) -> Result<Option<String>>;
    /// Bounded value. Password sources must not return the secret.
    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>>;
    /// `AXDescription`.
    async fn description(&self, node: &Self::Node) -> Result<Option<String>>;
    /// `AXPlaceholderValue`.
    async fn placeholder(&self, node: &Self::Node) -> Result<Option<String>>;
    /// `AXHelp`.
    async fn help(&self, node: &Self::Node) -> Result<Option<String>>;
    /// `AXIdentifier`.
    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>>;
    /// Frame in AX points, screen coordinates, top-left origin. Not pixels.
    async fn frame(&self, node: &Self::Node) -> Result<Option<Rect>>;
    /// Advertised actions, already normalized.
    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>>;
    /// `AXSelectedTextRange` in Unicode scalars.
    async fn selected_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>>;
    /// `AXVisibleCharacterRange`.
    async fn visible_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>>;
    /// `AXNumberOfCharacters`, the full length before truncation.
    async fn number_of_characters(&self, node: &Self::Node) -> Result<Option<usize>>;
    /// `AXURL`.
    async fn url(&self, node: &Self::Node) -> Result<Option<String>>;
    /// Opaque handle. No IO.
    fn handle(&self, node: &Self::Node) -> NativeHandle;
}

/// Choose the child list AX actually wants walked.
///
/// `Some` wins even when it is empty: an empty visible-child list means the
/// attribute was present. `None` means unsupported, so the next list is used.
pub fn select_children<T>(
    visible: Option<Vec<T>>,
    contents: Option<Vec<T>>,
    children: Vec<T>,
) -> Vec<T> {
    if let Some(visible) = visible {
        return visible;
    }
    if let Some(contents) = contents {
        return contents;
    }
    children
}

/// In-memory node. Child lists follow [`select_children`].
#[derive(Clone, Debug)]
pub struct FakeAxNode {
    /// Stable id inside this fixture.
    pub id: u64,
    /// AX role string.
    pub role: String,
    /// AX subrole.
    pub subrole: Option<String>,
    /// Accessible title.
    pub title: Option<String>,
    /// Value. Tests that want redaction still put the secret here.
    pub value: Option<NodeValue>,
    /// Description.
    pub description: Option<String>,
    /// Placeholder.
    pub placeholder: Option<String>,
    /// Help.
    pub help: Option<String>,
    /// Identifier.
    pub identifier: Option<String>,
    /// Extra string attributes.
    pub attributes: HashMap<String, String>,
    /// Frame in points.
    pub frame: Option<Rect>,
    /// State bits.
    pub states: AxStateBits,
    /// Actions.
    pub actions: Vec<NodeAction>,
    /// Selected text range.
    pub selected_text_range: Option<TextRange>,
    /// Visible text range.
    pub visible_text_range: Option<TextRange>,
    /// Full character count, which may exceed `value`.
    pub number_of_characters: Option<usize>,
    /// URL.
    pub url: Option<String>,
    /// `AXVisibleChildren`. `None` if unsupported.
    pub visible_children: Option<Vec<FakeAxNode>>,
    /// `AXContents`. `None` if unsupported.
    pub contents: Option<Vec<FakeAxNode>>,
    /// `AXChildren`.
    pub children: Vec<FakeAxNode>,
}

impl Default for FakeAxNode {
    fn default() -> Self {
        Self {
            id: 0,
            role: "AXGroup".into(),
            subrole: None,
            title: None,
            value: None,
            description: None,
            placeholder: None,
            help: None,
            identifier: None,
            attributes: HashMap::new(),
            frame: None,
            states: AxStateBits::default(),
            actions: Vec::new(),
            selected_text_range: None,
            visible_text_range: None,
            number_of_characters: None,
            url: None,
            visible_children: None,
            contents: None,
            children: Vec::new(),
        }
    }
}

/// Pure source for tests. No AX calls.
#[derive(Clone, Debug)]
pub struct FakeAxSource {
    /// Sleep at the start of every method. Zero in ordinary tests.
    pub delay: Duration,
    /// Node id that fails.
    pub error_on_id: Option<u64>,
    /// Error returned for [`FakeAxSource::error_on_id`].
    pub error: DriverError,
    /// Pid stamped into handles.
    pub pid: u32,
}

impl Default for FakeAxSource {
    fn default() -> Self {
        Self {
            delay: Duration::ZERO,
            error_on_id: None,
            error: DriverError::Native {
                message: "injected fake error".into(),
                maybe_executed: false,
            },
            pid: 0,
        }
    }
}

impl FakeAxSource {
    async fn gate(&self, id: u64) -> Result<()> {
        if self.delay > Duration::ZERO {
            tokio::time::sleep(self.delay).await;
        }
        if self.error_on_id == Some(id) {
            return Err(self.error.clone());
        }
        Ok(())
    }
}

#[async_trait]
impl AxSource for FakeAxSource {
    type Node = FakeAxNode;

    async fn children(&self, node: &Self::Node) -> Result<Vec<Self::Node>> {
        self.gate(node.id).await?;
        Ok(select_children(
            node.visible_children.clone(),
            node.contents.clone(),
            node.children.clone(),
        ))
    }

    async fn attributes(&self, node: &Self::Node) -> Result<HashMap<String, String>> {
        self.gate(node.id).await?;
        Ok(node.attributes.clone())
    }

    async fn states(&self, node: &Self::Node) -> Result<AxStateBits> {
        self.gate(node.id).await?;
        Ok(node.states.clone())
    }

    async fn role(&self, node: &Self::Node) -> Result<String> {
        self.gate(node.id).await?;
        Ok(node.role.clone())
    }

    async fn subrole(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.subrole.clone())
    }

    async fn title(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.title.clone())
    }

    async fn value(&self, node: &Self::Node) -> Result<Option<NodeValue>> {
        self.gate(node.id).await?;
        Ok(node.value.clone())
    }

    async fn description(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.description.clone())
    }

    async fn placeholder(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.placeholder.clone())
    }

    async fn help(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.help.clone())
    }

    async fn identifier(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.identifier.clone())
    }

    async fn frame(&self, node: &Self::Node) -> Result<Option<Rect>> {
        self.gate(node.id).await?;
        Ok(node.frame)
    }

    async fn actions(&self, node: &Self::Node) -> Result<Vec<NodeAction>> {
        self.gate(node.id).await?;
        Ok(node.actions.clone())
    }

    async fn selected_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>> {
        self.gate(node.id).await?;
        Ok(node.selected_text_range)
    }

    async fn visible_text_range(&self, node: &Self::Node) -> Result<Option<TextRange>> {
        self.gate(node.id).await?;
        Ok(node.visible_text_range)
    }

    async fn number_of_characters(&self, node: &Self::Node) -> Result<Option<usize>> {
        self.gate(node.id).await?;
        Ok(node.number_of_characters)
    }

    async fn url(&self, node: &Self::Node) -> Result<Option<String>> {
        self.gate(node.id).await?;
        Ok(node.url.clone())
    }

    fn handle(&self, node: &Self::Node) -> NativeHandle {
        NativeHandle::new(AxHandle {
            pid: self.pid,
            id: node.id,
        })
    }
}
