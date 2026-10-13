use super::{
    lock,
    router::{error, outcome, ActionRouter},
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
        code: ErrorCode,
        win: &WindowRef,
    ) -> ToolOutcome {
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
            out.windows = self
                .native(session, &cancel, false, self.driver.list_windows(None))
                .await
                .unwrap_or_default();
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
            out.state = Some(ActionState {
                text: format!(
                    "Window {} is gone. Available windows: {}",
                    win.id,
                    serde_json::to_string(&out.windows).unwrap_or_default()
                ),
                image: None,
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
            });
        }
        out
    }
}
