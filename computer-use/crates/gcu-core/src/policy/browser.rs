//! Verified-window exception for the Grok denylist entry. Titles are never read.

use super::identity;
use super::PolicyCtx;

/// True only for a fully verified native window in the same session.
///
/// Appended denies still refuse. An empty generation, a zero pid, or a zero
/// start token is unproven and does not match.
pub(crate) fn exception_applies(ctx: &PolicyCtx<'_>) -> bool {
    let Some(grant) = ctx.browser else {
        return false;
    };
    let Some(window) = ctx.window else {
        return false;
    };
    if grant.pid == 0 || grant.started == 0 {
        return false;
    }
    if !bounded(&grant.session_id) || !bounded(&grant.window_id) || !bounded(&grant.generation) {
        return false;
    }
    if grant.session_id != ctx.session_id || grant.window_id != window.id {
        return false;
    }
    if ctx.app.pid.is_some_and(|pid| pid != grant.pid) {
        return false;
    }
    !identity::extra_denies(&ctx.app.identity, &ctx.config.extra_denied_apps)
}

fn bounded(value: &str) -> bool {
    (1..=256).contains(&value.len()) && !value.chars().any(|ch| ch.is_control())
}
