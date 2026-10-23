use super::{
    lock,
    router::{error, ActionRouter},
    wire::CallError,
    ActionState,
};
use crate::{
    driver::{Cancel, CaptureOpts, TreeOpts, WindowRef},
    fingerprint::refresh_tree,
    model::{Fingerprint, Geometry, RawNode, RawTree},
    observe::{CaptureEvidence, TreeQuality},
    redact::{self, CapturedFrame, RedactedImage},
    render::RenderOpts,
    session::Session,
    snapshot::{assemble, ElementIndex, RenderedState, WindowKey},
    tree::{Pipeline, ProcessedTree},
};
use gcu_protocol::{
    tools::ScreenshotMode, types::ScreenshotInfo, AppState, ErrorCode, ToolError, WindowInfo,
};
use serde_json::{json, Value};

pub(crate) struct Fresh {
    pub win: WindowRef,
    pub info: WindowInfo,
    pub raw: RawTree,
    pub tree: ProcessedTree,
    pub signature: String,
    pub image: Option<RedactedImage>,
    pub quality: TreeQuality,
    pub scope_run: Option<String>,
    pub scope_generation: Option<u64>,
}
impl Fresh {
    pub fn geometry(&self) -> Geometry {
        Geometry {
            frame: self.info.frame,
            scale: self.info.scale,
        }
    }
    pub fn key(&self) -> WindowKey {
        WindowKey {
            app_id: self.win.app.info.id.clone(),
            window_id: self.win.id.clone(),
        }
    }
}
impl ActionRouter {
    pub(crate) async fn read(
        &self,
        session: &Session,
        win: &WindowRef,
        screenshot: bool,
        opts: &TreeOpts,
    ) -> Result<Fresh, CallError> {
        self.read_with_mode(session, win, ScreenshotMode::Explicit(screenshot), opts)
            .await
    }
    pub(crate) async fn read_with_mode(
        &self,
        session: &Session,
        win: &WindowRef,
        mode: ScreenshotMode,
        opts: &TreeOpts,
    ) -> Result<Fresh, CallError> {
        let producing = crate::scope::expected_epoch().or_else(|| {
            self.lease.as_ref().and_then(|lease| {
                lease
                    .current()
                    .map(|scope| (scope.run_id().to_owned(), scope.generation()))
            })
        });
        if self.lease.is_some() && producing.is_none() {
            return Err(CallError::ScopeDenied);
        }
        let cancel = Cancel::default();
        let mut raw = self
            .native(
                session,
                &cancel,
                false,
                self.driver.snapshot_tree(win, opts, &cancel),
            )
            .await
            .map_err(|e| CallError::from_driver(&e))?;
        if let Some(root) = &mut raw.root {
            refresh_tree(root, Fingerprint::default(), 0);
        }
        let geometry = self
            .native(session, &cancel, false, self.driver.window_geometry(win))
            .await
            .map_err(|e| CallError::from_driver(&e))?;
        // Screenshot identity is the accessibility tree. DOM fusion runs after
        // that compare so a DOM-only change cannot mark the capture stale, and
        // before redaction so shared password fields are still masked.
        let ax_signature = signature(&raw)?;
        let mut capture = None;
        let captured_at;
        // Auto captures speculatively to detect visual changes that the tree
        // cannot express. Only a synchronized, redacted image can reach policy
        // comparison or delivery; an omitted image never enters SnapshotStore.
        if mode != ScreenshotMode::Explicit(false) {
            if raw.incomplete.is_some() {
                return Err(ErrorCode::HandoffRequired.into());
            }
            let shot = self
                .native(
                    session,
                    &cancel,
                    false,
                    self.driver.capture(win, &CaptureOpts::default(), &cancel),
                )
                .await
                .map_err(|e| CallError::from_driver(&e))?;
            captured_at = self.epoch.elapsed();
            let after = self
                .native(
                    session,
                    &cancel,
                    false,
                    self.driver.snapshot_tree(win, opts, &cancel),
                )
                .await
                .map_err(|e| CallError::from_driver(&e))?;
            if after.incomplete.is_some() {
                return Err(ErrorCode::HandoffRequired.into());
            }
            if ax_signature != signature(&after)? {
                return Err(ErrorCode::StaleScreenshot.into());
            }
            let current = self
                .native(session, &cancel, false, self.driver.window_geometry(win))
                .await
                .map_err(|e| CallError::from_driver(&e))?;
            if geometry != current || shot.geometry != geometry {
                return Err(ErrorCode::StaleGeometry.into());
            }
            capture = Some(shot);
        } else {
            captured_at = self.epoch.elapsed();
        }
        if let Some(hub) = self.browser_hub() {
            let aligned = self
                .native(session, &cancel, false, async {
                    Ok(hub.align(session.id(), win, raw).await)
                })
                .await
                .map_err(|e| CallError::from_driver(&e))?;
            raw = aligned;
        }
        if mode != ScreenshotMode::Explicit(false) && raw.incomplete.is_some() {
            return Err(ErrorCode::HandoffRequired.into());
        }
        let merged_signature = signature(&raw)?;
        let mut tree = Pipeline::default().run(raw.clone(), opts);
        let quality = TreeQuality::assess(&raw, &tree);
        let version = digest(merged_signature.as_bytes());
        tree.state_version = Some(version);
        tree.observed_at = Some(captured_at);
        let image = match capture {
            Some(shot) => Some(
                redact::apply(&tree, CapturedFrame::new(shot, version, captured_at)).map_err(
                    |e| match e {
                        redact::RedactionError::Stale => ErrorCode::StaleScreenshot,
                        _ => ErrorCode::HandoffRequired,
                    },
                )?,
            ),
            None => None,
        };
        let windows = self
            .native(
                session,
                &cancel,
                false,
                self.driver.list_windows(Some(&win.app)),
            )
            .await
            .map_err(|e| CallError::from_driver(&e))?;
        let mut info = windows
            .into_iter()
            .find(|w| w.id == win.id)
            .ok_or(ErrorCode::WindowGone)?;
        if info.frame != geometry.frame || info.scale != geometry.scale {
            return Err(ErrorCode::StaleGeometry.into());
        }
        info.frame = geometry.frame;
        let key = WindowKey {
            app_id: win.app.info.id.clone(),
            window_id: win.id.clone(),
        };
        let resources = lock(&session.actions);
        let previous = resources.snapshots.latest(&key);
        let next = previous
            .and_then(|s| s.index().elements().keys().max())
            .map_or(0, |i| u64::from(*i) + 1);
        ElementIndex::assign(&mut tree, previous.map(|s| s.index()), next)
            .map_err(|_| ErrorCode::StaleElement)?;
        drop(resources);
        if let Some((run, generation)) = &producing {
            if !self.scope_binding_current(Some(run.as_str()), Some(*generation)) {
                return Err(CallError::ScopeDenied);
            }
        }
        Ok(Fresh {
            win: win.clone(),
            info,
            raw,
            tree,
            signature: merged_signature,
            image,
            quality,
            scope_run: producing.as_ref().map(|(run, _)| run.clone()),
            scope_generation: producing.as_ref().map(|(_, generation)| *generation),
        })
    }
    pub(crate) fn rendered_diff_is_empty(
        &self,
        session: &Session,
        fresh: &Fresh,
        text_limit: Option<usize>,
    ) -> bool {
        let mut opts = RenderOpts::new(fresh.win.app.info.clone(), fresh.info.clone());
        opts.text_limit = text_limit;
        // Match SnapshotStore's delivered baseline, including secret-bearing
        // labels/identifiers removed after Pipeline's password-value redaction.
        let mut tree = fresh.tree.clone();
        crate::snapshot::sanitize(&mut tree);
        let current = RenderedState::from_tree(&tree, &opts);
        lock(&session.actions)
            .snapshots
            .latest(&fresh.key())
            .is_some_and(|previous| previous.rendered() == &current)
    }
    pub(crate) fn observation_attachment(
        &self,
        session: &Session,
        fresh: &Fresh,
        mode: ScreenshotMode,
        text_limit: Option<usize>,
    ) -> bool {
        if let ScreenshotMode::Explicit(attach) = mode {
            return attach;
        }
        let diff_is_empty = self.rendered_diff_is_empty(session, fresh, text_limit);
        let pixels_changed = lock(&session.actions)
            .snapshots
            .latest_with_screenshot(&fresh.key())
            .and_then(|previous| previous.screenshot())
            .zip(fresh.image.as_ref())
            .map(|(before, after)| {
                before.geometry() != after.geometry() || before.pixels() != after.pixels()
            });
        self.auto_capture.observation(
            &fresh.quality,
            CaptureEvidence {
                diff_is_empty,
                pixels_changed,
                ..CaptureEvidence::default()
            },
        )
    }
    pub(crate) fn publish(
        &self,
        session: &Session,
        fresh: Fresh,
        attach: bool,
    ) -> Result<ActionState, Box<ToolError>> {
        self.publish_with_limit(session, fresh, attach, self.tree_opts.text_limit)
    }
    pub(crate) fn publish_with_limit(
        &self,
        session: &Session,
        mut fresh: Fresh,
        attach: bool,
        text_limit: Option<usize>,
    ) -> Result<ActionState, Box<ToolError>> {
        let config = self.policy.load();
        // After policy load and before the thumbnail. A revoke here refuses
        // before an id is reserved.
        self.publish_gate.enter();
        let image = if attach {
            fresh
                .image
                .as_ref()
                .map(|i| i.thumbnail(config.max_image_edge))
                .transpose()
                .map_err(|_| Box::new(error(ErrorCode::HandoffRequired)))?
        } else {
            None
        };
        self.publication_refusal(session, &fresh)?;
        let mut opts = RenderOpts::new(fresh.win.app.info.clone(), fresh.info.clone());
        opts.text_limit = text_limit;
        let stored = if attach { fresh.image.take() } else { None };
        let record_window = WindowKey::from_options(&opts);
        let baseline = {
            let resources = lock(&session.actions);
            resources.snapshots.baseline(&record_window)
        };
        let mut assembled = assemble(fresh.tree.clone(), &opts, stored, &baseline)
            .map_err(|_| Box::new(error(ErrorCode::Internal)))?;
        // Match the stored index onto the caller's tree before publication.
        ElementIndex::assign(&mut fresh.tree, Some(&assembled.index), 0)
            .map_err(|_| Box::new(error(ErrorCode::Internal)))?;
        let scope_run = fresh.scope_run.clone();
        let scope_generation = fresh.scope_generation;
        assembled.memory_bytes += scope_run.as_ref().map_or(0, String::len);
        assembled.scope_run = scope_run.clone();
        assembled.scope_generation = scope_generation;
        let id = {
            let mut resources = lock(&session.actions);
            resources
                .snapshots
                .reserve_id()
                .map_err(|_| Box::new(error(ErrorCode::Internal)))?
        };
        let screenshot = image.as_ref().map(|image| {
            assembled.delivery_size = (image.width(), image.height());
            ScreenshotInfo {
                id: format!("i_{}", id.0),
                width: image.width(),
                height: image.height(),
                scale_to_window: Some(image.scale_to_window()),
                mime: "image/png".into(),
            }
        });
        let state = ActionState {
            text: std::mem::take(&mut assembled.text),
            overflow_text: assembled.overflow_text.take(),
            state: AppState {
                app: opts.app,
                window: opts.window,
                snapshot_id: format!("s_{}", id.0),
                screenshot,
                focused: fresh.tree.focused,
                truncated: fresh.tree.truncation(fresh.tree.omitted_nodes),
                elements: None,
                action: None,
            },
            image,
            scope_run,
            scope_generation,
        };
        // All rendering, indexing, state assembly and test pauses precede the
        // snapshot lock and the lease. A denied id remains burned.
        self.commit_gate.enter();
        session.index_gate().enter();
        if let Some(lease) = &self.lease {
            lease.commit_before_gate().enter();
        }
        let mut resources = lock(&session.actions);
        let plan = resources
            .snapshots
            .prepare_install(&assembled)
            .map_err(|_| Box::new(error(ErrorCode::Internal)))?;
        let mut retired = Vec::with_capacity(resources.snapshots.len());
        let install = || {
            let observation = resources
                .snapshots
                .install(id, assembled, plan, &mut retired);
            // Keep the prepared diff alive until after the lease lock, too.
            (observation, state)
        };
        let committed = if let Some(lease) = &self.lease {
            let (Some(run), Some(generation)) =
                (fresh.scope_run.as_deref(), fresh.scope_generation)
            else {
                return Err(self.refusal_after_loss(session));
            };
            lease
                .commit_if_current(&[(run, generation)], install)
                .map_err(|_| self.refusal_after_loss(session))
        } else {
            Ok(install())
        };
        drop(resources);
        let (observation, state) = committed?;
        drop(observation);
        drop(retired);
        if let Some(lease) = &self.lease {
            lease.commit_after_gate().enter();
        }
        Ok(state)
    }

    /// Refuse a sample whose producing epoch is missing or no longer current.
    ///
    /// Runs before indexing, without the snapshot lock. The closing check is
    /// [`crate::scope::ScopeLease::commit_if_current`], which holds the lease
    /// read lock across the install. Stop wins only when the epoch is already
    /// gone. Memory only; no await, timeout, or cancellation.
    fn publication_refusal(&self, session: &Session, fresh: &Fresh) -> Result<(), Box<ToolError>> {
        if self.scope_binding_current(fresh.scope_run.as_deref(), fresh.scope_generation) {
            return Ok(());
        }
        Err(self.refusal_after_loss(session))
    }

    fn refusal_after_loss(&self, session: &Session) -> Box<ToolError> {
        if session.is_stopped() || session.activity().stop_requested() {
            Box::new(error(ErrorCode::StoppedByUser))
        } else {
            Box::new(CallError::ScopeDenied.tool_error())
        }
    }
}
// Full raw state is compared in memory, not just rendered/budgeted text. This
// includes recipient/content, offscreen nodes and password changes; never logged.
pub(crate) fn signature(raw: &RawTree) -> Result<String, ErrorCode> {
    fn node(n: &RawNode) -> Value {
        json!([
            n.role,
            n.native_role,
            n.subrole,
            n.name,
            n.value,
            n.description,
            n.placeholder,
            n.help,
            n.identifier,
            n.states,
            n.actions,
            n.frame,
            n.text_extras,
            n.url,
            n.children.iter().map(node).collect::<Vec<_>>()
        ])
    }
    serde_json::to_string(&(raw.incomplete, raw.root.as_ref().map(node)))
        .map_err(|_| ErrorCode::Internal)
}
pub(crate) fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
