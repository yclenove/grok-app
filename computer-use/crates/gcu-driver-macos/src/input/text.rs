//! Focus-checked AX append, Unicode events, and explicitly permitted clipboard.

use super::{
    clipboard::{self, Pasteboard},
    clock::Clock,
    timing::Operation,
    EventApi, InputLedger, InputTarget, PostPath,
};
use crate::ax::AxHandle;
use async_trait::async_trait;
use gcu_core::{
    driver::{Cancel, Delivery, DriverError, KeyRoute, Result},
    model::NativeHandle,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use unicode_segmentation::UnicodeSegmentation;

/// Maximum UTF-16 units accepted in one CG Unicode event.
pub const UNICODE_UNITS: usize = 20;

/// Split only at grapheme boundaries, including ZWJ, combining marks and CRLF.
/// An oversized individual grapheme is refused before any input is sent.
pub fn grapheme_chunks(text: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut chunk = String::new();
    let mut units = 0;
    for grapheme in text.graphemes(true) {
        let size = grapheme.encode_utf16().count();
        if size > UNICODE_UNITS {
            return Err(DriverError::InvalidRequest);
        }
        if units + size > UNICODE_UNITS {
            out.push(std::mem::take(&mut chunk));
            units = 0;
        }
        chunk.push_str(grapheme);
        units += size;
    }
    if !chunk.is_empty() {
        out.push(chunk);
    }
    Ok(out)
}

/// Fresh AX focus facts. Payloads deliberately do not implement `Debug`.
#[derive(Clone)]
pub struct TextFocus {
    /// Element identity, bound to the requested pid and window by the adapter.
    pub handle: AxHandle,
    /// Enabled editable control, with terminal/console controls excluded.
    pub editable: bool,
    /// Complete current AXValue only for proven plain, settable text.
    pub plain_value: Option<String>,
    /// Complete pre-paste value for a consumption oracle; never observed/logged.
    pub paste_baseline: Option<String>,
    /// Password or another sensitive field.
    pub sensitive: bool,
    /// Unknown/unsafe target semantics; clipboard must refuse.
    pub high_risk: bool,
    /// The requested window and this field actually own foreground focus.
    pub frontmost: bool,
}

/// AX boundary. Native implementations use the dedicated AX thread.
#[async_trait]
pub trait TextApi: Send + Sync {
    /// Fresh focus in this exact target window; no activation or focus changes.
    async fn focus(&self, target: &InputTarget) -> Result<TextFocus>;
    /// Revalidate focus, identity and complete value, then append once.
    /// Errors after starting a write must preserve `maybe_executed`.
    async fn append(&self, target: &InputTarget, expected: &TextFocus, text: &str) -> Result<()>;
    /// Wait for a positive consumption oracle before restoring the pasteboard.
    /// Must honor cancellation and the caller's runtime deadline.
    async fn paste_observed(
        &self,
        target: &InputTarget,
        expected: &TextFocus,
        text: &str,
        cancel: &Cancel,
    ) -> Result<()>;
}

/// Trusted per-action clipboard permission, never inferred from text content.
/// Unknown risk and sensitivity default to refusal. Does not change KeyRoute.
#[derive(Clone, Copy)]
pub struct TextOptions {
    /// Explicit opt-in; only long text and AllowForeground can use it.
    pub allow_clipboard: bool,
    /// Trusted caller marks credentials or other sensitive payloads.
    pub sensitive_text: bool,
    /// Trusted caller's action/target risk assessment.
    pub high_risk_target: bool,
}
impl Default for TextOptions {
    fn default() -> Self {
        Self {
            allow_clipboard: false,
            sensitive_text: true,
            high_risk_target: true,
        }
    }
}

/// Injectable backends and clock. Shared ledgers are owned by the driver.
#[derive(Clone)]
pub struct TextContext {
    /// Accessibility focus and append backend.
    pub text: Arc<dyn TextApi>,
    /// Event backend from P1-06.
    pub events: Arc<dyn EventApi>,
    /// Pasteboard backend, unused unless explicitly permitted.
    pub pasteboard: Arc<dyn Pasteboard>,
    /// Press-before-send ledger.
    pub ledger: Arc<InputLedger>,
    /// Monotonic runtime clock.
    pub clock: Arc<dyn Clock>,
}

/// Input with a bounded budget, cleanup, and no retries on any uncertain result.
/// AX plain append wins. Otherwise Unicode events are used; >2000 scalars may
/// use clipboard only with explicit permission and a foreground route ceiling.
#[allow(clippy::too_many_arguments)]
pub async fn type_text(
    ctx: &TextContext,
    target: &InputTarget,
    expected: Option<&NativeHandle>,
    text: &str,
    route: KeyRoute,
    options: TextOptions,
    cancel: &Cancel,
    budget: Duration,
) -> Result<Delivery> {
    let budget = budget.min(gcu_core::driver::DRIVER_DEADLINE);
    let guards = ctx.events.guards();
    if let Some(guards) = &guards {
        guards.check(false)?;
    }
    if text.is_empty() || text.chars().count() > 8192 {
        return Err(DriverError::InvalidRequest);
    }
    let sent = AtomicBool::new(false);
    let cleanup = Duration::from_millis(100).min(budget);
    let op = Operation::new(
        cancel,
        &sent,
        ctx.clock.as_ref(),
        budget.saturating_sub(cleanup),
        guards,
    );
    let result = run(ctx, target, expected, text, route, options, &op).await;
    let cleanup_deadline = ctx.clock.now() + cleanup;
    tokio::select! {
        biased;
        _ = super::release_held(ctx.events.as_ref(), &ctx.ledger) => {},
        _ = ctx.clock.wait_until(cleanup_deadline) => {},
    }
    result.map_err(|error| {
        if sent.load(Ordering::Acquire) && !error.maybe_executed() {
            super::foreground::uncertain(error)
        } else {
            error
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn run(
    ctx: &TextContext,
    target: &InputTarget,
    expected: Option<&NativeHandle>,
    text: &str,
    route: KeyRoute,
    options: TextOptions,
    op: &Operation<'_>,
) -> Result<Delivery> {
    op.check()?;
    super::refuse_before_post(&ctx.events.probe(), target, ctx.events.as_ref())?;
    let focus = op.guard(ctx.text.focus(target)).await?;
    validate_focus(&focus, target)?;
    if let Some(expected) = expected {
        if expected.downcast_ref::<AxHandle>() != Some(&focus.handle) {
            return Err(DriverError::StaleElement);
        }
    }
    if focus.plain_value.is_some() && !focus.sensitive {
        op.sent.store(true, Ordering::Release);
        let result = op.guard(ctx.text.append(target, &focus, text)).await;
        if result.as_ref().is_err_and(|error| !error.maybe_executed()) {
            op.sent.store(false, Ordering::Release);
        }
        return result.map(|()| Delivery::Accessibility);
    }
    if text.chars().count() > 2000 && options.allow_clipboard {
        if options.sensitive_text
            || options.high_risk_target
            || focus.sensitive
            || focus.high_risk
            || focus.paste_baseline.is_none()
            || text.contains(['\n', '\r'])
            || route != KeyRoute::AllowForeground
        {
            return Err(DriverError::CapabilityUnavailable);
        }
        return clipboard::paste(ctx, target, &focus, text, op).await;
    }
    let probe = ctx.events.probe();
    let targeted = probe.background_ready()
        && !crate::ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name);
    let path = if targeted {
        PostPath::ToPid
    } else if route == KeyRoute::AllowForeground && probe.foreground {
        PostPath::HidTap
    } else {
        return Err(DriverError::CapabilityUnavailable);
    };
    // Validate every cluster before activation or the first post.
    let events = super::keys::plan_text(target, text, path)?;
    if path == PostPath::HidTap {
        op.foreground().check()?;
        op.guard(super::foreground::activate_verified(
            ctx.events.as_ref(),
            target,
            op.sent,
        ))
        .await?;
    }
    for pair in events.chunks(2) {
        let fresh = op.guard(ctx.text.focus(target)).await?;
        validate_focus(&fresh, target)?;
        if fresh.handle != focus.handle || (path == PostPath::HidTap && !fresh.frontmost) {
            return Err(uncertain(op, "text focus changed"));
        }
        op.guard(super::post_all(
            ctx.events.as_ref(),
            &ctx.ledger,
            pair,
            op.cancel,
            op.sent,
        ))
        .await?;
    }
    if path == PostPath::ToPid {
        op.guard(super::confirm_arrival(ctx.events.as_ref(), target, op.sent))
            .await?;
        Ok(Delivery::TargetedEvent)
    } else {
        Ok(Delivery::Foreground)
    }
}

pub(crate) fn validate_focus(focus: &TextFocus, target: &InputTarget) -> Result<()> {
    if focus.handle.pid != target.pid as u32 {
        return Err(DriverError::StaleElement);
    }
    if !focus.editable {
        return Err(DriverError::ElementNotActionable);
    }
    Ok(())
}

pub(crate) fn uncertain(op: &Operation<'_>, message: &str) -> DriverError {
    DriverError::Native {
        message: message.into(),
        maybe_executed: op.sent.load(Ordering::Acquire),
    }
}
