//! Router adapter: settle once, compare complete evidence, publish safe state.
use super::{
    gate::Gate,
    observe::Fresh,
    router::{outcome, ActionRouter},
    ToolCall, ToolOutcome,
};
use crate::{
    driver::{BoxStream, UiEvent},
    model::Role,
    session::Session,
    settle::SettleRequest,
};
use gcu_protocol::{
    result::{ActionResult, Delivery, Effect},
    tools::BatchStep,
};
use std::time::Duration;

impl ActionRouter {
    pub(super) async fn settle_and_publish(
        &self,
        session: &Session,
        call: &ToolCall,
        gate: &Gate,
        mut action: ActionResult,
        before: Fresh,
        events: Option<BoxStream<'static, UiEvent>>,
    ) -> ToolOutcome {
        let win = &before.win;
        let config = self.settle_engine.config();
        let budget = call.settle_budget_ms.map(Duration::from_millis).unwrap_or(
            if matches!(call.action, BatchStep::TypeText(_)) {
                config.text_budget
            } else {
                config.action_budget
            },
        );
        let report = match self
            .settle_engine
            .wait(SettleRequest {
                driver: self.driver.as_ref(),
                session,
                window: win,
                tree_opts: &self.tree_opts,
                events,
                before: &before.raw,
                budget,
            })
            .await
        {
            Ok(report) => report,
            Err(code) => return self.failure(session, action, code, win).await,
        };
        action.settle = report.settle.clone();
        let after = match self
            .read(
                session,
                win,
                self.driver.capabilities().observe.screenshot,
                &self.tree_opts,
            )
            .await
        {
            Ok(fresh) => fresh,
            Err(code) => return self.failure(session, action, code, win).await,
        };
        let pixels = before
            .image
            .as_ref()
            .zip(after.image.as_ref())
            .map(|(a, b)| a.geometry() != b.geometry() || a.pixels() != b.pixels());
        action.effect = match report.effect(&before.raw, &after.raw, pixels) {
            Ok(effect) => effect,
            Err(code) => return self.failure(session, action, code, win).await,
        };
        if action.effect == Effect::NoVisibleChange {
            action.notes.push(
                "No visible change after delivery; the action may not have taken effect.".into(),
            );
        }
        if action.delivery != Delivery::Foreground {
            action
                .notes
                .push("Delivered without foreground activation.".into());
        }
        let tree_changed = before.signature != after.signature;
        let canvas = after.raw.root.is_none()
            || after
                .tree
                .nodes()
                .iter()
                .any(|(_, n)| n.node.role == Role::Canvas);
        let attach = !gate.points.is_empty()
            || canvas
            || (!tree_changed && pixels == Some(true))
            || call.attach_image;
        match self.publish(session, after, attach) {
            Ok(state) => outcome(action, None, Some(state)),
            Err(error) => outcome(action, Some(*error), None),
        }
    }
}
