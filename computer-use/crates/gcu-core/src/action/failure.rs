use super::{
    lock,
    router::{error, outcome, ActionRouter},
    wire::CallError,
    ActionState, ToolOutcome,
};
use crate::{
    driver::{AppQuery, Cancel, WindowRef},
    session::Session,
    snapshot::WindowKey,
};
use gcu_protocol::{
    result::{ActionResult, Outcome},
    AppState, ErrorCode,
};
impl ActionRouter {
    pub(crate) async fn failure(
        &self,
        session: &Session,
        action: ActionResult,
        failure: impl Into<CallError>,
        win: &WindowRef,
    ) -> ToolOutcome {
        let failure = failure.into();
        // Copy the authority that started this failure before any refresh await.
        // Copy once; a later admission must not stamp an old window with a newer epoch.
        let producing = originating_epoch(self);
        let code = match failure {
            CallError::ScopeDenied => {
                return outcome(action, Some(failure.tool_error()), None);
            }
            CallError::Code(code) => code,
        };
        let refresh = matches!(
            code,
            ErrorCode::StaleElement
                | ErrorCode::StaleGeometry
                | ErrorCode::StaleScreenshot
                | ErrorCode::WindowGone
                | ErrorCode::ElementNotActionable
        ) || action.outcome == Outcome::Unknown;
        let mut out = outcome(action, Some(error(code)), None);
        if !refresh || session.is_stopped() || session.is_closed() {
            return out;
        }
        let cancel = Cancel::default();
        let current_app = self
            .native(
                session,
                &cancel,
                false,
                self.driver.resolve_app(&AppQuery(win.app.info.id.clone())),
            )
            .await
            .ok();
        let current_win = WindowRef {
            app: current_app.unwrap_or_else(|| win.app.clone()),
            id: win.id.clone(),
        };
        let fresh = self
            .read(
                session,
                &current_win,
                self.driver.capabilities().observe.screenshot,
                &self.tree_opts,
            )
            .await;
        let fresh = match fresh {
            Ok(f) => Ok(f),
            Err(_) => {
                self.read(session, &current_win, false, &self.tree_opts)
                    .await
            }
        };
        if let Ok(fresh) = fresh {
            out.state = self.publish(session, fresh, true).ok();
        }
        if out.state.is_none() && code == ErrorCode::WindowGone {
            // Test barrier after the refresh awaits and before this epoch is used.
            // The captured epoch is not read again from the lease.
            self.refresh_gate.enter();
            if self.lease.is_some() && !window_list_current(self, session, producing.as_ref()) {
                return out;
            }
            let windows = self
                .native(session, &cancel, false, self.driver.list_windows(None))
                .await
                .unwrap_or_default();
            if self.lease.is_some() && !window_list_current(self, session, producing.as_ref()) {
                return out;
            }
            // After the list await and before the snapshot mutex. A revoke while
            // this waits never attaches the metadata assembled below.
            session.snapshot_gate().enter();
            let prior = lock(&session.actions)
                .snapshots
                .latest(&WindowKey {
                    app_id: win.app.info.id.clone(),
                    window_id: win.id.clone(),
                })
                .map(|s| s.geometry());
            let geometry = prior.unwrap_or(crate::model::Geometry {
                frame: gcu_protocol::Rect {
                    x: 0.,
                    y: 0.,
                    w: 0.,
                    h: 0.,
                },
                scale: 1.,
            });
            let state = ActionState {
                text: format!(
                    "Window {} is gone. Available windows: {}",
                    win.id,
                    serde_json::to_string(&windows).unwrap_or_default()
                ),
                image: None,
                overflow_text: None,
                state: AppState {
                    app: current_win.app.info,
                    window: gcu_protocol::WindowInfo {
                        id: win.id.clone(),
                        title: String::new(),
                        frame: geometry.frame,
                        scale: geometry.scale,
                        on_current_desktop: None,
                    },
                    snapshot_id: String::new(),
                    screenshot: None,
                    focused: None,
                    truncated: None,
                    elements: None,
                    action: None,
                },
                scope_run: producing.as_ref().map(|(run, _)| run.clone()),
                scope_generation: producing.as_ref().map(|(_, generation)| *generation),
            };
            if let Some(lease) = &self.lease {
                let Some((run, generation)) = producing.as_ref() else {
                    return out;
                };
                lease.commit_before_gate().enter();
                // Host activity (including is_stopped) may take a foreign mutex.
                let stopped = session.is_stopped() || session.activity().stop_requested();
                let attached = lease.commit_if_current(&[(run.as_str(), *generation)], || {
                    if stopped {
                        return None;
                    }
                    Some((state, windows))
                });
                if let Ok(Some((state, windows))) = attached {
                    out.state = Some(state);
                    out.windows = windows;
                    lease.commit_after_gate().enter();
                }
            } else {
                out.state = Some(state);
                out.windows = windows;
            }
        }
        out
    }
}

/// Authority copied at the start of [`ActionRouter::failure`].
///
/// Prefers the attempt epoch. Outside an attempt, copies the lease once.
/// Memory only. No blocking, timeout, or cancellation. Callers must not
/// replace this with a later `current()` after an await.
fn originating_epoch(router: &ActionRouter) -> Option<(String, u64)> {
    if let Some(epoch) = crate::scope::expected_epoch() {
        return Some(epoch);
    }
    let scope = router
        .lease
        .as_ref()
        .and_then(|lease| lease.authorized_current())?;
    Some((scope.run_id().to_owned(), scope.generation()))
}

/// Whether a scoped window list may still be published.
///
/// `None` inside a scope is not proof. Unscoped callers do not use this.
/// Memory only: the lease read drops its lock before return. No timeout or
/// cancellation.
fn window_list_current(
    router: &ActionRouter,
    session: &Session,
    producing: Option<&(String, u64)>,
) -> bool {
    if session.is_stopped() || session.activity().stop_requested() {
        return false;
    }
    let Some((run, generation)) = producing else {
        return false;
    };
    router.scope_binding_current(Some(run.as_str()), Some(*generation))
}
