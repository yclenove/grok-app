//! Binding, image index, and navigation plan. Memory only; no WebView calls.

use super::url::{classify, UrlClass, UrlRole};
use super::wire::{window_valid, BrowserSync, BrowserWindow};

const RETIRED_CAP: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImageRow {
    id: String,
    session_id: String,
    instance_id: String,
    document_revision: u64,
}

/// Why a sync was not installed. The previous map is unchanged unless noted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyFault {
    /// A window was offered while the platform is unproven. The map is unchanged.
    Unavailable,
    /// Revision moved backwards, repeated, or named a retired instance.
    Stale,
    /// The window failed a field check.
    Invalid,
    /// The retired-instance table is full. The caller must close the connection.
    ///
    /// A revoke still drops the live window before this is returned.
    Saturated,
}

/// Why navigation must not be treated as a successful dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavFault {
    /// L1 is closed, or no window is bound.
    Unavailable,
    /// The caller does not own the bound window.
    Owner,
    /// The instance was closed or retired.
    Gone,
    /// Revision, document, or policy generation does not match.
    Stale,
    /// The URL is not a public navigation target.
    UrlBlocked,
    /// Stop is latched.
    Stopped,
    /// Delivery may have started. Do not retry automatically.
    Unknown,
    /// Parameters are not a navigate request.
    Invalid,
    /// The queue or window lock refused before any handoff.
    Busy,
    /// Failure that proves the URL was not handed to native navigation.
    Internal,
}

/// Image index failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFault {
    /// The session does not own the image.
    Owner,
    /// The instance is gone.
    Gone,
    /// The document revision no longer matches.
    Stale,
    /// The id is not in this index.
    Missing,
    /// The instance was already retired.
    Late,
}

/// Result of planning one navigation. [`Plan::Ready`] is the only arm that may call native code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Canonical URL. This is not evidence that a WebView ran.
    Ready(String),
    /// Do not dispatch.
    Refuse(NavFault),
}

/// Inputs the engine fills. The model supplies only the URL.
pub struct NavRequest<'a> {
    /// Candidate URL.
    pub url: &'a str,
    /// App session of the tool call.
    pub session_id: &'a str,
    /// Instance the engine believes is current.
    pub instance_id: &'a str,
    /// Binding revision the engine believes is current.
    pub revision: u64,
    /// Document generation the engine believes is current.
    pub document_revision: u64,
    /// Policy generation the engine loaded.
    pub policy_version: u64,
    /// Live stop latch.
    pub stopped: bool,
}

/// One engine's browser map.
#[derive(Debug, Clone)]
pub struct Book {
    revision: u64,
    window: Option<BrowserWindow>,
    retired: Vec<String>,
    images: Vec<ImageRow>,
    revoked_images: Vec<String>,
    policy_version: Option<u64>,
}

impl Book {
    /// Empty map. The first accepted sync must use revision 1.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            revision: 0,
            window: None,
            retired: Vec::new(),
            images: Vec::new(),
            revoked_images: Vec::new(),
            policy_version: None,
        }
    }

    /// Record the policy generation loaded from the Host. Unset stays stale.
    pub fn note_policy(&mut self, version: u64) {
        self.policy_version = Some(version);
    }

    /// Last accepted revision. Zero means no sync has been accepted.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Owner session of the live window, if one is installed.
    pub fn owner(&self) -> Option<&str> {
        self.window
            .as_ref()
            .map(|window| window.session_id.as_str())
    }

    /// Instance, binding revision, document revision, and policy generation.
    pub fn nav_context(&self) -> Option<(String, u64, u64, Option<u64>)> {
        let window = self.window.as_ref()?;
        Some((
            window.instance_id.clone(),
            self.revision,
            window.document_revision,
            self.policy_version,
        ))
    }

    /// Install `sync` when `proven` allows a window. A null browser is always a revoke.
    pub fn apply(&mut self, sync: &BrowserSync, proven: bool) -> Result<(), ApplyFault> {
        if sync.revision != self.revision.saturating_add(1) {
            return Err(ApplyFault::Stale);
        }
        match &sync.browser {
            Some(window) if !proven => Err(ApplyFault::Unavailable),
            Some(window) => self.install(sync.revision, window),
            None => {
                let fault = self.revoke_live();
                self.revision = sync.revision;
                fault
            }
        }
    }

    /// Drop the live window when `session` owns it. The revision counter stays put
    /// so the App's next sync remains current+1. The instance id cannot return.
    ///
    /// [`ApplyFault::Saturated`] means the window was still dropped and the
    /// connection must close.
    pub fn clear_owner(&mut self, session: &str) -> Result<bool, ApplyFault> {
        let owns = self
            .window
            .as_ref()
            .is_some_and(|window| window.session_id == session);
        if !owns {
            return Ok(false);
        }
        self.revoke_live()?;
        Ok(true)
    }

    /// Bind an image to the live document. Late writes after retire are refused.
    pub fn bind_image(
        &mut self,
        session: &str,
        id: &str,
        instance: &str,
        document_revision: u64,
    ) -> Result<(), ImageFault> {
        if self.revoked_images.iter().any(|row| row == id)
            || self.retired.iter().any(|row| row == instance)
        {
            return Err(ImageFault::Late);
        }
        let Some(window) = &self.window else {
            return Err(ImageFault::Gone);
        };
        if window.session_id != session || window.instance_id != instance {
            return Err(ImageFault::Owner);
        }
        if window.document_revision != document_revision {
            return Err(ImageFault::Stale);
        }
        self.images.push(ImageRow {
            id: id.to_owned(),
            session_id: session.to_owned(),
            instance_id: instance.to_owned(),
            document_revision,
        });
        Ok(())
    }

    /// Look up an image for `session`. A mismatched session, instance, or document fails.
    pub fn lookup_image(&self, session: &str, id: &str) -> Result<(), ImageFault> {
        if self.revoked_images.iter().any(|row| row == id) {
            return Err(ImageFault::Gone);
        }
        let Some(row) = self.images.iter().find(|row| row.id == id) else {
            return Err(ImageFault::Missing);
        };
        if row.session_id != session {
            return Err(ImageFault::Owner);
        }
        if self.retired.iter().any(|id| id == &row.instance_id) {
            return Err(ImageFault::Gone);
        }
        let Some(window) = &self.window else {
            return Err(ImageFault::Gone);
        };
        if window.instance_id != row.instance_id || window.session_id != session {
            return Err(ImageFault::Gone);
        }
        if window.document_revision != row.document_revision {
            Err(ImageFault::Stale)
        } else {
            Ok(())
        }
    }

    /// True when `id` is a browser image this session must not read.
    #[must_use]
    pub fn image_blocked(&self, session: &str, id: &str) -> bool {
        let known = self.images.iter().any(|row| row.id == id)
            || self.revoked_images.iter().any(|row| row == id);
        known && self.lookup_image(session, id).is_err()
    }

    /// Ids removed from the index. Delete files only after this returns.
    #[must_use]
    pub fn revoked_image_ids(&self) -> &[String] {
        &self.revoked_images
    }

    /// Plan one navigation. [`Plan::Ready`] is the only arm a navigator may run.
    #[must_use]
    pub fn plan(&self, request: NavRequest<'_>, proven: bool) -> Plan {
        if !matches!(
            classify(request.url, UrlRole::Navigation),
            UrlClass::Public(_)
        ) {
            return Plan::Refuse(NavFault::UrlBlocked);
        }
        if !proven {
            return Plan::Refuse(NavFault::Unavailable);
        }
        let Some(window) = &self.window else {
            return Plan::Refuse(NavFault::Unavailable);
        };
        if window.session_id != request.session_id {
            return Plan::Refuse(NavFault::Owner);
        }
        if self.retired.iter().any(|id| id == request.instance_id) {
            return Plan::Refuse(NavFault::Gone);
        }
        if window.instance_id != request.instance_id || request.revision != self.revision {
            return Plan::Refuse(NavFault::Stale);
        }
        if window.document_revision != request.document_revision {
            return Plan::Refuse(NavFault::Stale);
        }
        if request.stopped {
            return Plan::Refuse(NavFault::Stopped);
        }
        if self.policy_version != Some(request.policy_version) {
            return Plan::Refuse(NavFault::Stale);
        }
        match classify(request.url, UrlRole::Navigation) {
            UrlClass::Public(canonical) => Plan::Ready(canonical),
            UrlClass::Denied => Plan::Refuse(NavFault::UrlBlocked),
        }
    }

    fn install(&mut self, revision: u64, window: &BrowserWindow) -> Result<(), ApplyFault> {
        if !window_valid(window) {
            return Err(ApplyFault::Invalid);
        }
        if self.retired.iter().any(|id| id == &window.instance_id) {
            return Err(ApplyFault::Stale);
        }
        let same = self
            .window
            .as_ref()
            .is_some_and(|live| live.instance_id == window.instance_id);
        if !same {
            self.revoke_live()?;
        }
        self.window = Some(window.clone());
        self.revision = revision;
        Ok(())
    }

    fn revoke_live(&mut self) -> Result<(), ApplyFault> {
        let Some(window) = self.window.take() else {
            return Ok(());
        };
        let instance = window.instance_id;
        self.revoke_images(&instance);
        if self.retired.iter().any(|id| id == &instance) {
            return Ok(());
        }
        if self.retired.len() >= RETIRED_CAP {
            return Err(ApplyFault::Saturated);
        }
        self.retired.push(instance);
        Ok(())
    }

    fn revoke_images(&mut self, instance: &str) {
        let mut kept = Vec::new();
        for row in self.images.drain(..) {
            if row.instance_id == instance {
                self.revoked_images.push(row.id);
            } else {
                kept.push(row);
            }
        }
        self.images = kept;
    }
}
