//! One-shot `expect` on the action router. No input and no settle wait.
//!
//! The check is a single post-approval read. It does not call
//! [`crate::driver::Driver::perform`] and does not use the settle clock.

use super::{
    router::{error, outcome, result, window_lock_key, ActionRouter},
    ToolCall, ToolOutcome,
};
use crate::{
    policy::{ActionIntent, Tool},
    session::Session,
    wait::{met, supported, Stability},
};
use gcu_protocol::{
    result::{Outcome, Verification, VerifyState},
    tools::{Condition, ElementCondition},
    ErrorCode,
};

impl ActionRouter {
    /// Evaluate `condition` once with the same authorization, Stop, and cancellation
    /// path as [`ActionRouter::wait_for`].
    ///
    /// Satisfied is [`Outcome::Done`] with [`VerifyState::Satisfied`]. Unsatisfied is
    /// [`Outcome::Failed`] with [`VerifyState::Unsatisfied`] and no error code: batch
    /// `stop_on_error` decides whether later steps run. A read or policy failure keeps
    /// the real [`ErrorCode`] and sets [`VerifyState::Unknown`].
    ///
    /// Blocking: window-lock waits cap at 5s, then one native read. Timeout: the read
    /// uses the 5s native ceiling. Cancellation: Stop, session close, or turn end.
    pub(super) async fn expect_queued(
        &self,
        session: &Session,
        call: &ToolCall,
        condition: Condition,
    ) -> ToolOutcome {
        let mut action = result(call);
        let label = expect_label(&condition);
        if condition.validate().is_err() || supported(&condition).is_err() {
            action.verify = Some(verify(&label, VerifyState::Unknown));
            return outcome(action, Some(error(ErrorCode::CapabilityUnavailable)), None);
        }
        if let Err(code) = self.check_boundary(session).await {
            action.verify = Some(verify(&label, VerifyState::Unknown));
            return outcome(action, Some(error(code)), None);
        }
        let win = match self
            .resolve(session, &call.app, call.window.as_deref())
            .await
        {
            Ok(win) => win,
            Err(code) => {
                action.verify = Some(verify(&label, VerifyState::Unknown));
                return outcome(action, Some(error(code)), None);
            }
        };
        let _window = match self.locks.acquire(&window_lock_key(&win)).await {
            Ok(guard) => guard,
            Err(err) => {
                action.verify = Some(verify(&label, VerifyState::Unknown));
                return outcome(action, Some(error(err.code())), None);
            }
        };
        let fresh = match self.read(session, &win, false, &self.tree_opts).await {
            Ok(fresh) => fresh,
            Err(code) => {
                action.verify = Some(verify(&label, VerifyState::Unknown));
                return self.failure(session, action, code, &win).await;
            }
        };
        let mut intent = ActionIntent::new(Tool::Observe);
        intent.parameters = fresh.signature.clone();
        if let Err(code) = self.authorize(session, &mut intent, &fresh).await {
            action.verify = Some(verify(&label, VerifyState::Unknown));
            return outcome(action, Some(error(code)), None);
        }
        drop(fresh);
        if let Err(code) = self.check_boundary(session).await {
            action.verify = Some(verify(&label, VerifyState::Unknown));
            return outcome(action, Some(error(code)), None);
        }
        let fresh = match self.read(session, &win, false, &self.tree_opts).await {
            Ok(fresh) => fresh,
            Err(code) => {
                action.verify = Some(verify(&label, VerifyState::Unknown));
                return self.failure(session, action, code, &win).await;
            }
        };
        let mut stability = Stability::default();
        match met(&condition, &fresh.raw, &fresh.info.title, &mut stability) {
            Ok(true) => {
                action.outcome = Outcome::Done;
                action.verify = Some(verify(&label, VerifyState::Satisfied));
                action.notes.push("Expectation met.".into());
                action.notes.push("No input was delivered.".into());
                match self.publish(session, fresh, false) {
                    Ok(state) => outcome(action, None, Some(state)),
                    Err(err) => outcome(action, Some(*err), None),
                }
            }
            Ok(false) => {
                action.outcome = Outcome::Failed;
                action.verify = Some(verify(&label, VerifyState::Unsatisfied));
                action.notes.push("Expectation was not met.".into());
                action.notes.push("No input was delivered.".into());
                match self.publish(session, fresh, false) {
                    Ok(state) => outcome(action, None, Some(state)),
                    Err(err) => outcome(action, Some(*err), None),
                }
            }
            Err(code) => {
                drop(fresh);
                action.verify = Some(verify(&label, VerifyState::Unknown));
                outcome(action, Some(error(code)), None)
            }
        }
    }
}

fn verify(label: &str, state: VerifyState) -> Verification {
    Verification {
        expect: label.to_owned(),
        state,
    }
}

fn expect_label(condition: &Condition) -> String {
    match condition {
        Condition::TextPresent(text) => format!("text_present:{text}"),
        Condition::TextAbsent(text) => format!("text_absent:{text}"),
        Condition::Element(element) => element_label(element),
        Condition::WindowTitle(title) => format!("window_title:{title}"),
        Condition::Stable(value) => format!("stable:{value}"),
    }
}

fn element_label(element: &ElementCondition) -> String {
    match &element.name {
        Some(name) => format!("element:{}:{name}", element.role),
        None => format!("element:{}", element.role),
    }
}
