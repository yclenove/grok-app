use super::{
    observe::{digest, signature},
    router::ActionRouter,
};
use crate::{
    driver::{BoxStream, Cancel, UiEvent, WindowRef},
    session::Session,
};
use gcu_protocol::{
    result::{Settle, SettleReason},
    ErrorCode,
};
use std::{future::poll_fn, time::Duration};
use tokio::time::Instant;

impl ActionRouter {
    pub(crate) async fn settle(
        &self,
        session: &Session,
        win: &WindowRef,
        mut events: Option<BoxStream<'static, UiEvent>>,
        budget: Duration,
    ) -> Result<Settle, ErrorCode> {
        let start = Instant::now();
        let deadline = start + budget;
        let mut quiet = start + Duration::from_millis(120);
        let mut poll = start;
        let mut previous = None;
        let cancel = Cancel::default();
        loop {
            self.check_boundary(session).await?;
            let reason = tokio::select! {
                biased;
                _ = tokio::time::sleep_until(deadline) => Some(SettleReason::Budget),
                event = async { match &mut events { Some(stream) => poll_fn(|cx| stream.as_mut().poll_next(cx)).await, None => std::future::pending().await } } => {
                    match event { Some(UiEvent::WindowGone) => return Err(ErrorCode::WindowGone), Some(_) => quiet = Instant::now() + Duration::from_millis(120), None => events = None }
                    None
                }
                _ = tokio::time::sleep_until(quiet), if events.is_some() => Some(SettleReason::EventQuiet),
                _ = tokio::time::sleep_until(poll) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let raw = tokio::time::timeout(remaining, self.native(session, &cancel, false, self.driver.snapshot_tree(win, &self.tree_opts, &cancel))).await;
                    match raw {
                        Err(_) => Some(SettleReason::Budget),
                        Ok(Err(e)) => return Err(e.error_code()),
                        Ok(Ok(raw)) => {
                            let hash = digest(signature(&raw)?.as_bytes());
                            let stable = previous == Some(hash);
                            previous = Some(hash);
                            poll = Instant::now() + Duration::from_millis(150);
                            stable.then_some(SettleReason::TreeStable)
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => None,
            };
            if let Some(reason) = reason {
                return Ok(Settle {
                    ms: start.elapsed().as_millis() as u64,
                    reason,
                });
            }
        }
    }
}
