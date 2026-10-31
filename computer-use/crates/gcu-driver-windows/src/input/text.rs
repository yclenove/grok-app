//! Bounded long text and one-shot, explicitly foreground-authorized paste.
use super::{api::*, clipboard::ClipboardApi, hotkey::SafetyGuard, ledger::InputLedger, messages};
use crate::uia::patterns::ActionContext;
use gcu_core::{driver::*, model::NativeHandle};
use std::sync::Arc;

/// Total UTF-16 ceiling; larger requests refuse before activation or mutation.
pub const TEXT_MAX_UNITS: usize = 65_536;
/// Plan Unicode packets in batches of at most 8192 UTF-16 units. Preserve scalar
/// and CRLF boundaries both between batches and inside native packets.
pub fn chunks(text: &str) -> Result<Vec<Vec<Event>>> {
    if text.encode_utf16().count() > TEXT_MAX_UNITS || text.contains('\0') {
        return Err(DriverError::InvalidRequest);
    }
    let mut packets = Vec::new();
    let mut start = 0;
    let mut units = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((offset, c)) = chars.next() {
        let mut size = c.len_utf16();
        if c == '\r' && chars.peek().is_some_and(|(_, c)| *c == '\n') {
            chars.next();
            size += 1;
        }
        if units + size > messages::TEXT_MAX_UNITS {
            packets.extend(messages::text_chunks(&text[start..offset])?);
            start = offset;
            units = 0;
        }
        units += size;
    }
    packets.extend(messages::text_chunks(&text[start..])?);
    Ok(packets)
}

pub(super) struct Paste<'a> {
    pub api: &'a dyn InputApi,
    pub clipboard: Arc<dyn ClipboardApi>,
    pub tracker: Option<Arc<dyn InputTracker>>,
    pub ledger: Arc<InputLedger>,
    pub target: Target,
    pub focus: Option<NativeHandle>,
    pub text: String,
    pub context: ActionContext,
    pub safety: Option<Arc<SafetyGuard>>,
}
impl Paste<'_> {
    fn check_stop(&self) -> Result<()> {
        if let Some(guard) = &self.safety {
            guard.check(true)?;
        }
        self.context.check()
    }
    async fn stage(&self) -> Result<u32> {
        if let Some(tracker) = &self.tracker {
            tracker
                .clipboard_stage(self.target.clone(), self.text.clone())
                .await
        } else {
            self.clipboard
                .stage(self.text.clone(), self.context.clone())
                .await
        }
    }
    async fn before_paste(&self) -> Result<()> {
        self.check_stop()?;
        let result = self
            .api
            .before_paste(
                self.target.clone(),
                self.focus.clone(),
                self.context.clone(),
            )
            .await;
        // A live parent latch is authoritative even when the focus read blocks.
        // In Worker mode InputPrepare checks that latch in the parent and the
        // independent Stop channel kills the child throughout these reads.
        self.check_stop()?;
        result
    }
    pub async fn run(self) -> Result<()> {
        self.check_stop()?;
        let recovery = Recovery {
            clipboard: self.clipboard.clone(),
            tracker: self.tracker.clone(),
        };
        let result = async {
            let sequence = self.stage().await?;
            self.check_stop()?;
            let packet = messages::chord_events(
                &KeyChord::parse("ctrl+v").map_err(|_| DriverError::InvalidRequest)?,
                Some((0x56, Vec::new())),
            )?;
            // The accounting handshake may await parent IO. Do it BEFORE the
            // final content/focus/Stop checks, not between checks and dispatch.
            if let Some(tracker) = &self.tracker {
                tracker
                    .prepare(self.target.clone(), packet.clone(), Delivery::Foreground)
                    .await?;
            }
            self.api.validate(self.target.clone(), true).await?;
            self.before_paste().await?;
            let matches = self.clipboard.matches(sequence, self.text.clone()).await?;
            self.check_stop()?;
            if !matches {
                return Err(DriverError::CapabilityUnavailable);
            }
            // Recheck focus/Stop after the clipboard read as well.
            self.before_paste().await?;
            crate::scope_identity::require_delivery_scalars(
                self.target.window.hwnd,
                self.target.window.pid,
                self.target.window.process_created,
                self.target.window.created,
                self.target.hwnd,
            )?;
            let result = self
                .api
                .send(
                    self.target.clone(),
                    packet,
                    Delivery::Foreground,
                    self.ledger.clone(),
                    self.context.clone(),
                    None,
                )
                .await;
            if let Some(tracker) = &self.tracker {
                if result.is_ok() || result.as_ref().is_err_and(|e| !e.maybe_executed()) {
                    tracker.complete(result.is_ok()).await?;
                }
            }
            result?;
            // Ctrl+V is asynchronous; do not restore before the target has had
            // a chance to request the data. This is not proof of consumption.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            self.check_stop()
        }
        .await;
        let cleanup = recovery.restore().await;
        match result {
            Err(error) => {
                if cleanup.is_err() {
                    tracing::warn!("clipboard recovery failed after interrupted paste");
                    return Err(match error {
                        DriverError::Cancelled { .. } => DriverError::Cancelled {
                            maybe_executed: true,
                        },
                        DriverError::Timeout { .. } => DriverError::Timeout {
                            maybe_executed: true,
                        },
                        DriverError::Crashed { .. } => DriverError::Crashed {
                            maybe_executed: true,
                        },
                        _ => DriverError::Native {
                            message: "paste interrupted; clipboard recovery pending".into(),
                            maybe_executed: true,
                        },
                    });
                }
                Err(self.context.error(error))
            }
            Ok(()) => cleanup.map_err(|e| self.context.error(e)),
        }
    }
}
struct Recovery {
    clipboard: Arc<dyn ClipboardApi>,
    tracker: Option<Arc<dyn InputTracker>>,
}
impl Recovery {
    async fn restore(&self) -> Result<()> {
        if let Some(tracker) = &self.tracker {
            tracker.clipboard_restore().await
        } else {
            self.clipboard.restore().await
        }
    }
}
