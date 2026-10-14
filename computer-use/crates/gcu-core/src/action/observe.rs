use super::{
    lock,
    router::{error, ActionRouter},
    ActionState,
};
use crate::{
    driver::{Cancel, CaptureOpts, TreeOpts, WindowRef},
    fingerprint::refresh_tree,
    model::{Fingerprint, Geometry, RawNode, RawTree},
    redact::{self, CapturedFrame, RedactedImage},
    render::RenderOpts,
    session::Session,
    snapshot::{ElementIndex, WindowKey},
    tree::{Pipeline, ProcessedTree},
};
use gcu_protocol::{types::ScreenshotInfo, AppState, ErrorCode, ToolError, WindowInfo};
use serde_json::{json, Value};

pub(crate) struct Fresh {
    pub win: WindowRef,
    pub info: WindowInfo,
    pub raw: RawTree,
    pub tree: ProcessedTree,
    pub signature: String,
    pub image: Option<RedactedImage>,
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
    ) -> Result<Fresh, ErrorCode> {
        let cancel = Cancel::default();
        let mut raw = self
            .native(
                session,
                &cancel,
                false,
                self.driver.snapshot_tree(win, opts, &cancel),
            )
            .await
            .map_err(|e| e.error_code())?;
        if let Some(root) = &mut raw.root {
            refresh_tree(root, Fingerprint::default(), 0);
        }
        let geometry = self
            .native(session, &cancel, false, self.driver.window_geometry(win))
            .await
            .map_err(|e| e.error_code())?;
        let before = signature(&raw)?;
        let mut tree = Pipeline::default().run(raw.clone(), opts);
        let mut image = None;
        let captured_at;
        if screenshot {
            if raw.incomplete.is_some() {
                return Err(ErrorCode::HandoffRequired);
            }
            let capture = self
                .native(
                    session,
                    &cancel,
                    false,
                    self.driver.capture(win, &CaptureOpts::default(), &cancel),
                )
                .await
                .map_err(|e| e.error_code())?;
            captured_at = self.epoch.elapsed();
            let after = self
                .native(
                    session,
                    &cancel,
                    false,
                    self.driver.snapshot_tree(win, opts, &cancel),
                )
                .await
                .map_err(|e| e.error_code())?;
            if after.incomplete.is_some() {
                return Err(ErrorCode::HandoffRequired);
            }
            if before != signature(&after)? {
                return Err(ErrorCode::StaleScreenshot);
            }
            let current = self
                .native(session, &cancel, false, self.driver.window_geometry(win))
                .await
                .map_err(|e| e.error_code())?;
            if geometry != current || capture.geometry != geometry {
                return Err(ErrorCode::StaleGeometry);
            }
            let version = digest(before.as_bytes());
            tree.state_version = Some(version);
            tree.observed_at = Some(captured_at);
            image = Some(
                redact::apply(&tree, CapturedFrame::new(capture, version, captured_at)).map_err(
                    |e| match e {
                        redact::RedactionError::Stale => ErrorCode::StaleScreenshot,
                        _ => ErrorCode::HandoffRequired,
                    },
                )?,
            );
        } else {
            captured_at = self.epoch.elapsed();
            tree.state_version = Some(digest(before.as_bytes()));
            tree.observed_at = Some(captured_at);
        }
        let windows = self
            .native(
                session,
                &cancel,
                false,
                self.driver.list_windows(Some(&win.app)),
            )
            .await
            .map_err(|e| e.error_code())?;
        let mut info = windows
            .into_iter()
            .find(|w| w.id == win.id)
            .ok_or(ErrorCode::WindowGone)?;
        if info.frame != geometry.frame || info.scale != geometry.scale {
            return Err(ErrorCode::StaleGeometry);
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
        Ok(Fresh {
            win: win.clone(),
            info,
            raw,
            tree,
            signature: before,
            image,
        })
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
        let key = fresh.key();
        let mut opts = RenderOpts::new(fresh.win.app.info.clone(), fresh.info.clone());
        opts.text_limit = text_limit;
        let mut resources = lock(&session.actions);
        // Retain only images actually handed to the model. A speculative image
        // must not replace the model's latest coordinate observation.
        let observation = resources
            .snapshots
            .record(
                fresh.tree.clone(),
                &opts,
                if attach { fresh.image.take() } else { None },
            )
            .map_err(|_| Box::new(error(ErrorCode::Internal)))?;
        let snapshot = resources
            .snapshots
            .get(&key, observation.snapshot_id)
            .ok_or_else(|| Box::new(error(ErrorCode::Internal)))?;
        ElementIndex::assign(&mut fresh.tree, Some(snapshot.index()), 0)
            .map_err(|_| Box::new(error(ErrorCode::Internal)))?;
        let screenshot = image.as_ref().map(|i| ScreenshotInfo {
            id: format!("i_{}", observation.snapshot_id.0),
            width: i.width(),
            height: i.height(),
            scale_to_window: Some(i.scale_to_window()),
            mime: "image/png".into(),
        });
        if let Some(i) = &image {
            resources.snapshots.set_delivery_size(
                &key,
                observation.snapshot_id,
                i.width(),
                i.height(),
            );
        }
        Ok(ActionState {
            text: observation.text,
            overflow_text: observation.overflow_text,
            state: AppState {
                app: opts.app,
                window: opts.window,
                snapshot_id: format!("s_{}", observation.snapshot_id.0),
                screenshot,
                focused: fresh.tree.focused,
                truncated: fresh.tree.truncation(fresh.tree.omitted_nodes),
                elements: None,
                action: None,
            },
            image,
        })
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
