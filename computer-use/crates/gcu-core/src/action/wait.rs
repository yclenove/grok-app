//! `wait_for` on the action router. Reads only; [`crate::driver::Driver::perform`] is never called.
//!
//! The wait horizon (default 5s, maximum 30s) is not wrapped in [`crate::driver::NATIVE_TIMEOUT`].
//! Each snapshot inside the loop stays within [`crate::driver::DRIVER_DEADLINE`].

use super::{
    lock,
    observe::{self, Fresh},
    router::{error, outcome, result, window_lock_key, ActionRouter},
    ToolCall, ToolOutcome,
};
use crate::{
    fingerprint::refresh_tree,
    model::{Fingerprint, RawTree},
    policy::{ActionIntent, Tool},
    session::Session,
    snapshot::{ElementIndex, WindowKey},
    tree::Pipeline,
    wait::{supported, WaitPhase, WaitRequest},
};
use gcu_protocol::{
    result::Outcome,
    tools::{BatchStep, WaitForArgs},
    ErrorCode, WindowInfo,
};
use std::time::Duration;

impl ActionRouter {
    /// Branch `wait_for` before the mutating gate. Other steps use the normal executor.
    pub(crate) async fn dispatch_queued(&self, session: &Session, call: &ToolCall) -> ToolOutcome {
        match &call.action {
            BatchStep::WaitFor(args) => {
                let args = args.clone();
                self.wait_queued(session, call, args).await
            }
            _ => self.execute_queued(session, call).await,
        }
    }

    /// Run one `wait_for` with the same queue, authorization, Stop, and cancellation as
    /// [`ActionRouter::execute`].
    ///
    /// Batch and script reuse this entry. It enqueues once; a second enqueue would deadlock
    /// the session queue. No input is delivered. A match publishes the new observation.
    /// The deadline publishes [`ErrorCode::Timeout`] and the last observation when one exists.
    ///
    /// Blocking: the session queue has no deadline, the window lock waits at most 5s, then
    /// this call waits on the settle clock. Timeout: `timeout_ms` is 1..=30000 (default
    /// 5000). That horizon is not the 5s native ceiling; each snapshot still is.
    /// Cancellation: Stop, session close, turn end, or dropping the future.
    pub async fn wait_for(&self, session: &Session, call: ToolCall) -> ToolOutcome {
        if !matches!(call.action, BatchStep::WaitFor(_)) {
            let action = result(&call);
            return outcome(action, Some(error(ErrorCode::CapabilityUnavailable)), None);
        }
        self.execute(session, call).await
    }

    async fn wait_queued(
        &self,
        session: &Session,
        call: &ToolCall,
        args: WaitForArgs,
    ) -> ToolOutcome {
        let mut action = result(call);
        if args.validate().is_err() || supported(&args.until).is_err() {
            return outcome(action, Some(error(ErrorCode::CapabilityUnavailable)), None);
        }
        if let Err(code) = self.check_boundary(session).await {
            return outcome(action, Some(error(code)), None);
        }
        let win = match self
            .resolve(session, &call.app, call.window.as_deref())
            .await
        {
            Ok(win) => win,
            Err(code) => return outcome(action, Some(error(code)), None),
        };
        let _window = match self.locks.acquire(&window_lock_key(&win)).await {
            Ok(guard) => guard,
            Err(err) => return outcome(action, Some(error(err.code())), None),
        };
        let fresh = match self.read(session, &win, false, &self.tree_opts).await {
            Ok(fresh) => fresh,
            Err(code) => return self.failure(session, action, code, &win).await,
        };
        let mut intent = ActionIntent::new(Tool::Observe);
        intent.parameters = fresh.signature.clone();
        if let Err(code) = self.authorize(session, &mut intent, &fresh).await {
            // The pre-approval read can contain a secret. Do not publish it.
            return outcome(action, Some(error(code)), None);
        }
        drop(fresh);
        if let Err(code) = self.check_boundary(session).await {
            return outcome(action, Some(error(code)), None);
        }
        let events = self.driver.subscribe(&win);
        let report = match self
            .wait_engine
            .wait(WaitRequest {
                driver: self.driver.as_ref(),
                session,
                window: &win,
                tree_opts: &self.tree_opts,
                events,
                until: args.until,
                timeout: Duration::from_millis(u64::from(args.timeout_ms)),
            })
            .await
        {
            Ok(report) => report,
            Err(ErrorCode::WindowGone) => {
                return self
                    .failure(session, action, ErrorCode::WindowGone, &win)
                    .await;
            }
            Err(code) => return outcome(action, Some(error(code)), None),
        };
        action.settle.ms = u64::try_from(report.elapsed.as_millis()).unwrap_or(u64::MAX);
        action.notes.push("No input was delivered.".into());
        match report.phase {
            WaitPhase::Ready => {
                let (Some(raw), Some(info)) = (report.raw, report.info) else {
                    return outcome(action, Some(error(ErrorCode::Internal)), None);
                };
                match self.observation(session, &win, raw, info) {
                    Ok(state) => {
                        action.outcome = Outcome::Done;
                        outcome(action, None, Some(state))
                    }
                    Err(code) => outcome(action, Some(error(code)), None),
                }
            }
            WaitPhase::TimedOut => {
                action.outcome = Outcome::Failed;
                let state = match (report.raw, report.info) {
                    (Some(raw), Some(info)) => self.observation(session, &win, raw, info).ok(),
                    _ => None,
                };
                outcome(action, Some(error(ErrorCode::Timeout)), state)
            }
            WaitPhase::Cancelled => {
                let code = report.code.unwrap_or(ErrorCode::ApprovalDenied);
                outcome(action, Some(error(code)), None)
            }
            WaitPhase::Waiting => outcome(action, Some(error(ErrorCode::Internal)), None),
        }
    }

    /// Publish one already-read sample. Does not snapshot again and does not attach an image.
    fn observation(
        &self,
        session: &Session,
        win: &crate::driver::WindowRef,
        mut raw: RawTree,
        info: WindowInfo,
    ) -> Result<super::ActionState, ErrorCode> {
        if let Some(root) = &mut raw.root {
            refresh_tree(root, Fingerprint::default(), 0);
        }
        let signature = observe::signature(&raw)?;
        let mut tree = Pipeline::default().run(raw.clone(), &self.tree_opts);
        tree.state_version = Some(observe::digest(signature.as_bytes()));
        tree.observed_at = Some(self.epoch.elapsed());
        let key = WindowKey {
            app_id: win.app.info.id.clone(),
            window_id: win.id.clone(),
        };
        {
            let resources = lock(&session.actions);
            let previous = resources.snapshots.latest(&key);
            let next = previous
                .and_then(|snapshot| snapshot.index().elements().keys().max())
                .map_or(0, |index| u64::from(*index) + 1);
            ElementIndex::assign(&mut tree, previous.map(|snapshot| snapshot.index()), next)
                .map_err(|_| ErrorCode::StaleElement)?;
        }
        let fresh = Fresh {
            win: win.clone(),
            info,
            raw,
            tree,
            signature,
            image: None,
        };
        self.publish(session, fresh, false).map_err(|err| err.code)
    }
}
