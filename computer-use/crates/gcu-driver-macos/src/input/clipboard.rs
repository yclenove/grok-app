//! Process-global pasteboard transaction; never part of observation.
//!
//! Lock and restore guard stay on a blocking worker if the waiter times out.
//! Late writes cannot escape cleanup or race another session. Foreign processes
//! do not take our lock: checks cannot prevent last-instant replacement.

use super::{
    text::{uncertain, validate_focus, TextContext, TextFocus},
    timing::Operation,
    InputTarget, PostPath,
};
use gcu_core::driver::{Cancel, Delivery, DriverError, KeyChord, Result};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

static CLIPBOARD: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct CancelOnDrop(Cancel);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// All advertised types and owned bytes for every item, in order. No Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct PasteboardContent(pub Vec<Vec<(String, Vec<u8>)>>);
impl PasteboardContent {
    /// A single public UTF-8 text item.
    pub fn text(text: &str) -> Self {
        Self(vec![vec![(
            "public.utf8-plain-text".into(),
            text.as_bytes().to_vec(),
        )]])
    }
}

/// Complete content snapshot with native changeCount. Never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct PasteboardSnapshot {
    /// Native changeCount, not a timestamp.
    pub change_count: i64,
    /// All materialized items and formats.
    pub content: PasteboardContent,
}

/// Synchronous OS boundary, called on a blocking worker under global lock.
/// Refuse partial/promise snapshots. Compare count AND content before writes.
pub trait Pasteboard: Send + Sync {
    /// Copy all formats, rejecting concurrent modification.
    fn read(&self) -> Result<PasteboardSnapshot>;
    /// Replace only while the complete snapshot matches. None means changed.
    /// Failures after clearing must conditionally restore `expected`.
    fn replace_if(
        &self,
        expected: &PasteboardSnapshot,
        content: &PasteboardContent,
    ) -> Result<Option<PasteboardSnapshot>>;
}

struct Restore<'a> {
    board: &'a dyn Pasteboard,
    original: PasteboardContent,
    owned: PasteboardSnapshot,
    finished: bool,
}
impl Restore<'_> {
    fn finish(&mut self) -> Result<()> {
        self.finished = true;
        self.board
            .replace_if(&self.owned, &self.original)
            .map(|_| ())
    }
}
impl Drop for Restore<'_> {
    fn drop(&mut self) {
        if !self.finished && self.finish().is_err() {
            tracing::warn!("pasteboard cleanup failed");
        }
    }
}

pub(crate) async fn paste(
    ctx: &TextContext,
    target: &InputTarget,
    focus: &TextFocus,
    text: &str,
    op: &Operation<'_>,
) -> Result<Delivery> {
    let lock = op.guard(async { Ok(CLIPBOARD.lock().await) }).await?;
    let ctx = ctx.clone();
    let target = target.clone();
    let focus = focus.clone();
    let text = text.to_owned();
    let deadline = op.deadline;
    let stopped = Cancel::default();
    let _cancel_on_drop = CancelOnDrop(stopped.clone());
    let sent = Arc::new(AtomicBool::new(false));
    let sent_worker = Arc::clone(&sent);
    let stop_worker = stopped.clone();
    let runtime = tokio::runtime::Handle::current();
    let mut job = tokio::task::spawn_blocking(move || {
        let _lock = lock;
        let work = Operation {
            cancel: &stop_worker,
            sent: &sent_worker,
            clock: ctx.clock.as_ref(),
            deadline,
        };
        work.check()?;
        let fresh = runtime.block_on(work.guard(ctx.text.focus(&target)))?;
        validate_focus(&fresh, &target)?;
        if !fresh.frontmost || fresh.handle != focus.handle || fresh.sensitive || fresh.high_risk {
            return Err(uncertain(&work, "paste focus changed"));
        }
        let original = ctx.pasteboard.read()?;
        work.check()?;
        let payload = PasteboardContent::text(&text);
        let owned = ctx
            .pasteboard
            .replace_if(&original, &payload)?
            .ok_or_else(|| uncertain(&work, "pasteboard changed before write"))?;
        let mut restore = Restore {
            board: ctx.pasteboard.as_ref(),
            original: original.content,
            owned,
            finished: false,
        };
        let result = runtime.block_on(paste_once(
            &ctx,
            &target,
            &focus,
            &text,
            &restore.owned,
            &work,
        ));
        let cleanup = runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_millis(100),
                super::release_held(ctx.events.as_ref(), &ctx.ledger),
            )
            .await
        });
        let restored = restore.finish();
        result?;
        cleanup.map_err(|_| DriverError::Timeout {
            maybe_executed: true,
        })?;
        restored.map_err(|_| DriverError::Native {
            message: "pasteboard restoration failed".into(),
            maybe_executed: true,
        })?;
        Ok(Delivery::Clipboard)
    });
    let result = tokio::select! {
        biased;
        _ = op.cancel.cancelled() => { stopped.cancel(); Err(DriverError::Cancelled { maybe_executed: sent.load(Ordering::Acquire) }) },
        result = tokio::time::timeout_at(op.deadline, &mut job) => match result {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(DriverError::Crashed { maybe_executed: sent.load(Ordering::Acquire) }),
            Err(_) => { stopped.cancel(); Err(DriverError::Timeout { maybe_executed: sent.load(Ordering::Acquire) }) },
        },
    };
    op.sent
        .store(sent.load(Ordering::Acquire), Ordering::Release);
    result
}

async fn paste_once(
    ctx: &TextContext,
    target: &InputTarget,
    focus: &TextFocus,
    text: &str,
    owned: &PasteboardSnapshot,
    op: &Operation<'_>,
) -> Result<()> {
    op.check()?;
    if ctx.pasteboard.read()? != *owned {
        return Err(uncertain(op, "pasteboard changed before paste"));
    }
    let fresh = op.guard(ctx.text.focus(target)).await?;
    validate_focus(&fresh, target)?;
    if fresh.handle != focus.handle || !fresh.frontmost || fresh.sensitive || fresh.high_risk {
        return Err(uncertain(op, "paste focus changed"));
    }
    if ctx.pasteboard.read()? != *owned {
        return Err(uncertain(op, "pasteboard changed before paste"));
    }
    super::refuse_before_post(&ctx.events.probe(), target, ctx.events.as_ref())?;
    let events = super::keys::plan_chord(
        target,
        &KeyChord::parse("cmd+v").map_err(|_| DriverError::InvalidRequest)?,
        PostPath::HidTap,
    )?;
    for event in events {
        // The modifier post can yield. Revalidate immediately before V-down.
        if event.down && event.keycode == Some(0x09) {
            let fresh = op.guard(ctx.text.focus(target)).await?;
            if fresh.handle != focus.handle
                || !fresh.frontmost
                || !fresh.editable
                || fresh.sensitive
                || fresh.high_risk
                || ctx.pasteboard.read()? != *owned
            {
                return Err(uncertain(op, "pasteboard or focus changed before V"));
            }
            super::refuse_before_post(&ctx.events.probe(), target, ctx.events.as_ref())?;
        }
        op.guard(super::post_all(
            ctx.events.as_ref(),
            &ctx.ledger,
            &[event],
            op.cancel,
            op.sent,
        ))
        .await?;
    }
    op.guard(ctx.text.paste_observed(target, focus, text, op.cancel))
        .await
}
