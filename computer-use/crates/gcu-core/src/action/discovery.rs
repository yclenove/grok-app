//! Discovery and app launch reuse the router's session, policy, and native bounds.
use super::{
    lock,
    observe::Fresh,
    router::{error, outcome},
    wire::CallError,
    ActionRouter, ToolOutcome,
};
use crate::{
    driver::{AppQuery, Cancel, WindowRef},
    model::RawTree,
    policy::{ActionIntent, Tool},
    session::Session,
    tree::Pipeline,
};
use gcu_protocol::{
    result::{ActionResult, Delivery, Effect, Outcome, Settle, SettleReason},
    AppInfo, ErrorCode, LaunchAppParams, ToolError, ToolName, WindowInfo,
};
use std::sync::atomic::Ordering;

impl ActionRouter {
    /// Discover apps on the session queue, without granting access to their UI.
    /// Native work caps at 5s; Stop/close cancel the wait. No automatic retry.
    pub async fn list_apps(&self, session: &Session) -> Result<Vec<AppInfo>, Box<ToolError>> {
        session
            .enqueue(async {
                self.check_boundary(session)
                    .await
                    .map_err(|e| Box::new(error(e)))?;
                self.native(session, &Cancel::default(), false, self.driver.list_apps())
                    .await
                    .map_err(|e| Box::new(CallError::from_driver(&e).tool_error()))
            })
            .await
            .map_err(|_| Box::new(error(ErrorCode::ApprovalDenied)))?
    }
    /// Discover windows, optionally resolving one app query. No app grant is
    /// created. Serialized, Stop/close cancellable, each native wait caps at 5s.
    pub async fn list_windows(
        &self,
        session: &Session,
        query: Option<&str>,
    ) -> Result<Vec<WindowInfo>, Box<ToolError>> {
        session
            .enqueue(async {
                self.check_boundary(session)
                    .await
                    .map_err(|e| Box::new(error(e)))?;
                let cancel = Cancel::default();
                let app = if let Some(query) = query {
                    Some(
                        self.native(
                            session,
                            &cancel,
                            false,
                            self.driver.resolve_app(&AppQuery::from(query)),
                        )
                        .await
                        .map_err(|e| Box::new(CallError::from_driver(&e).tool_error()))?,
                    )
                } else {
                    None
                };
                self.native(
                    session,
                    &cancel,
                    false,
                    self.driver.list_windows(app.as_ref()),
                )
                .await
                .map_err(|e| Box::new(CallError::from_driver(&e).tool_error()))
            })
            .await
            .map_err(|_| Box::new(error(ErrorCode::ApprovalDenied)))?
    }
    /// Launch once after identity authorization and foreground/sensitive policy.
    /// Uses the same authorizer as input actions. Approval binding and identity
    /// are rechecked before launch. Session queue/Host timeout/native 5s bounds
    /// apply; uncertain launch is returned as unknown and never replayed.
    pub async fn launch_app(
        &self,
        session: &Session,
        call_id: &str,
        params: LaunchAppParams,
    ) -> ToolOutcome {
        let started = tokio::time::Instant::now();
        let initial = ActionResult {
            tool: ToolName::LaunchApp,
            target: None,
            delivery: Delivery::None,
            outcome: Outcome::Refused,
            effect: Effect::NotChecked,
            verify: None,
            duration_ms: 0,
            settle: Settle {
                ms: 0,
                reason: SettleReason::Budget,
            },
            notes: Vec::new(),
        };
        let queued = session
            .enqueue(async {
                let mut action = initial.clone();
                let result = self.launch_queued(session, &params).await;
                match result {
                    Ok(state) => {
                        action.outcome = Outcome::Done;
                        action.delivery = Delivery::Foreground;
                        outcome(action, None, state)
                    }
                    Err((err, maybe)) => {
                        if maybe {
                            action.outcome = Outcome::Unknown;
                            action.delivery = Delivery::Foreground;
                            action
                                .notes
                                .push("Launch is uncertain; do not automatically retry.".into());
                        }
                        outcome(action, Some(err.tool_error()), None)
                    }
                }
            })
            .await;
        let mut out = queued
            .unwrap_or_else(|_| outcome(initial, Some(error(ErrorCode::ApprovalDenied)), None));
        out.action.duration_ms = started.elapsed().as_millis() as u64;
        if !session.is_closed() {
            lock(&session.actions)
                .ledger
                .record(call_id, &out.action, self.epoch.elapsed());
        }
        out
    }
    async fn launch_queued(
        &self,
        session: &Session,
        params: &LaunchAppParams,
    ) -> Result<Option<super::ActionState>, (CallError, bool)> {
        self.check_boundary(session)
            .await
            .map_err(|code| (CallError::from(code), false))?;
        // Scope never permits launching, even an already admitted fixture.
        // Refuse before resolution or Host approval; keep the driver's own
        // refusal as an independent boundary for direct driver callers.
        if self.fixture_scoped {
            return Err((CallError::ScopeDenied, false));
        }
        let cancel = Cancel::default();
        let app = self
            .native(
                session,
                &cancel,
                false,
                self.driver.resolve_app(&AppQuery(params.app.clone())),
            )
            .await
            .map_err(|e| (CallError::from_driver(&e), false))?;
        // Authorization-only context: there is no window/tree to observe before
        // starting a process. It is never published as a fabricated observation.
        let raw = RawTree {
            root: None,
            incomplete: None,
        };
        let tree = Pipeline::default().run(raw.clone(), &self.tree_opts);
        let quality = crate::observe::TreeQuality::assess(&raw, &tree);
        let fresh = Fresh {
            win: WindowRef {
                app: app.clone(),
                id: String::new(),
            },
            info: WindowInfo {
                id: String::new(),
                title: String::new(),
                frame: gcu_protocol::Rect {
                    x: 0.,
                    y: 0.,
                    w: 0.,
                    h: 0.,
                },
                scale: 1.,
                on_current_desktop: None,
            },
            tree,
            quality,
            raw,
            image: None,
            signature: format!("{:?}", app),
            scope_run: self.scope_stamp().0,
            scope_generation: self.scope_stamp().1,
        };
        let mut intent = ActionIntent::new(Tool::LaunchApp);
        intent.foreground = true;
        intent.parameters = fresh.signature.clone();
        intent.reason = params.reason.clone().map(gcu_host_api::ModelReason::new);
        let approved = self
            .authorize(session, &mut intent, &fresh)
            .await
            .map_err(|err| (err, false))?;
        let current = self
            .native(
                session,
                &cancel,
                false,
                self.driver.resolve_app(&AppQuery(app.info.id.clone())),
            )
            .await
            .map_err(|e| (CallError::from_driver(&e), false))?;
        if current != app
            || self.policy.load() != approved.config
            || self.policy_version.load(Ordering::Acquire) != approved.binding.policy_version
        {
            return Err((CallError::Code(ErrorCode::StaleElement), false));
        }
        let host = super::authorize::host_app(&fresh, self.driver.platform())
            .map_err(|code| (CallError::from(code), false))?;
        match self.evaluate(
            session,
            &intent,
            &fresh,
            &host,
            &super::authorize::policy_window(&fresh),
            &approved,
        ) {
            crate::policy::PolicyDecision::Allow {
                route_limit: crate::policy::RouteLimit::ForegroundAllowed,
            } => {}
            crate::policy::PolicyDecision::Refuse(code, _) => {
                return Err((CallError::Code(code), false))
            }
            _ => return Err((CallError::Code(ErrorCode::ApprovalDenied), false)),
        }
        let launched = self
            .native(session, &cancel, true, self.driver.launch_app(&app))
            .await
            .map_err(|e| (CallError::from_driver(&e), e.maybe_executed()))?;
        if launched.info.identity != app.info.identity {
            return Err((CallError::Code(ErrorCode::HandoffRequired), true));
        }
        // An app need not create a window. A successful launch remains done.
        let window = self
            .native(session, &cancel, false, self.driver.key_window(&launched))
            .await;
        let Ok(window) = window else {
            return Ok(None);
        };
        let win = WindowRef {
            app: launched,
            id: window.id,
        };
        let _guard = self
            .locks
            .acquire(&format!(
                "{}:{}:{}",
                win.app.info.id.len(),
                win.app.info.id,
                win.id
            ))
            .await
            .map_err(|e| (CallError::from(e.code()), true))?;
        match self.read(session, &win, false, &self.tree_opts).await {
            Ok(fresh) => self.publish(session, fresh, false).map(Some).map_err(|e| {
                let mapped = if e.code == ErrorCode::AppDenied
                    && e.message == crate::scope::SCOPE_DENIED_MESSAGE
                {
                    CallError::ScopeDenied
                } else {
                    CallError::Code(e.code)
                };
                (mapped, true)
            }),
            Err(_) => Ok(None),
        }
    }
}
