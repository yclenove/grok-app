//! Driver integration without changing the architecture's DriverAction.
use crate::{
    input::{
        clipboard::Pasteboard,
        text::{TextApi, TextContext, TextOptions},
        timing::InputClock,
    },
    MacosDriver,
};
use gcu_core::driver::{Cancel, Delivery, DriverAction, DriverError, Result, DRIVER_DEADLINE};
use std::sync::Arc;

pub(crate) const CLIPBOARD_NOTE: &str = "Text input checks editable AX focus. Clipboard is disabled by default; trusted per-action permission and AllowForeground are required for long text. Sensitive text and high-risk targets cannot use it. Clipboard history or synchronization may retain text. Sequence/content/focus checks cannot eliminate last-instant replacement by another process.";

#[derive(Clone)]
pub(crate) struct TextBackends {
    text: Arc<dyn TextApi>,
    pasteboard: Arc<dyn Pasteboard>,
    clock: Arc<dyn InputClock>,
}

impl MacosDriver {
    /// Install portable text/pasteboard adapters and a runtime-domain clock.
    /// Real adapters are installed by `new`; this never grants clipboard access.
    pub fn inject_text(
        &self,
        text: Arc<dyn TextApi>,
        pasteboard: Arc<dyn Pasteboard>,
        clock: Arc<dyn InputClock>,
    ) {
        if let Ok(mut slot) = self.text.lock() {
            *slot = Some(TextBackends {
                text,
                pasteboard,
                clock,
            });
        }
    }

    /// Trusted per-action text entry point; Driver::perform uses refusing defaults.
    /// Clipboard requires explicit non-sensitive, low-risk permission, long text,
    /// and AllowForeground. The complete action includes the usual window gate,
    /// runtime deadline, cancellation and conditional cleanup; it never retries.
    pub async fn perform_text(
        &self,
        req: &DriverAction,
        options: TextOptions,
        cancel: &Cancel,
    ) -> Result<Delivery> {
        let DriverAction::TypeText {
            win,
            focus,
            text,
            route,
        } = req
        else {
            return Err(DriverError::InvalidRequest);
        };
        let backend = self
            .text
            .lock()
            .ok()
            .and_then(|s| s.clone())
            .ok_or(DriverError::CapabilityUnavailable)?;
        let started = backend.clock.now();
        let win = win.clone();
        let target = self
            .call(DRIVER_DEADLINE, Some(cancel), false, move |state| {
                state.prepare_input(&win)
            })
            .await?;
        let context = TextContext {
            text: backend.text,
            events: self.event_api().ok_or(DriverError::CapabilityUnavailable)?,
            pasteboard: backend.pasteboard,
            ledger: Arc::clone(&self.ledger),
            clock: backend.clock,
        };
        let budget =
            DRIVER_DEADLINE.saturating_sub(context.clock.now().saturating_duration_since(started));
        crate::input::text::type_text(
            &context,
            &target,
            focus.as_ref(),
            text,
            *route,
            options,
            cancel,
            budget,
        )
        .await
    }
}
