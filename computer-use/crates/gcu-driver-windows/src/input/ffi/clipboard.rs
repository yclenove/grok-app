//! Thread-affine clipboard owner and cross-process transaction mutex.
mod data;
mod text_formats;
use super::super::clipboard::ClipboardApi;
use crate::uia::{patterns::ActionContext, worker::Worker};
use async_trait::async_trait;
use data::{Format, Open};
use gcu_core::driver::{DriverError, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use windows::{
    core::w,
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{DataExchange::*, Threading::*},
        UI::WindowsAndMessaging::*,
    },
};

fn failure() -> DriverError {
    DriverError::Native {
        message: "Windows clipboard operation failed".into(),
        maybe_executed: false,
    }
}
/// Lazy persistent clipboard thread. Native locks never cross OS threads. The
/// parent owns this instance in Worker mode, so killing the child retains backup.
#[derive(Default)]
pub struct NativeClipboard {
    worker: tokio::sync::OnceCell<Worker<State>>,
    pending: Arc<AtomicBool>,
}
impl NativeClipboard {
    /// Stage synthetic text for native diagnostics, with an explicit deadline
    /// and cancellation token. Callers must perform sensitivity checks first.
    pub async fn stage_text(
        &self,
        text: String,
        cancel: &gcu_core::driver::Cancel,
        deadline: std::time::Instant,
    ) -> Result<u32> {
        self.stage(text, ActionContext::until(cancel, deadline))
            .await
    }

    async fn worker(&self) -> Result<&Worker<State>> {
        self.worker
            .get_or_try_init(|| async {
                let pending = self.pending.clone();
                Worker::start(move || State::new(pending), State::maintain).await
            })
            .await
    }
}
#[async_trait]
impl ClipboardApi for NativeClipboard {
    async fn stage(&self, text: String, context: ActionContext) -> Result<u32> {
        self.stage_with(text, context).await
    }
    async fn matches(&self, sequence: u32, text: String) -> Result<bool> {
        self.worker()
            .await?
            .call(move |s, r| {
                r.check()?;
                let _open = Open::new(s.window)?;
                // SAFETY: nonblocking OS sequence read while our clipboard lock is held.
                if unsafe { GetClipboardSequenceNumber() } != sequence {
                    return Ok(false);
                }
                Ok(data::text()? == text)
            })
            .await
    }
    async fn restore(&self) -> Result<()> {
        if let Some(worker) = self.worker.get() {
            worker.call(|s, _| s.restore()).await
        } else {
            Ok(())
        }
    }
    fn pending(&self) -> bool {
        self.pending.load(Ordering::Acquire)
    }
}
impl NativeClipboard {
    async fn stage_with(&self, text: String, context: ActionContext) -> Result<u32> {
        loop {
            context.check()?;
            let value = text.clone();
            let action = context.clone();
            if let Some(sequence) = self
                .worker()
                .await?
                .call_action(context.clone(), move |s, r| {
                    r.check()?;
                    s.stage(value, action)
                })
                .await?
            {
                return Ok(sequence);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

struct Gate(HANDLE);
impl Gate {
    fn acquire() -> Result<Option<Self>> {
        // SAFETY: constant session-local name; default DACL isolates other users.
        // Local is the interactive Windows session's clipboard namespace, shared
        // by every gcu process in that session, not a per-window or per-engine lock.
        let handle =
            unsafe { CreateMutexW(None, false, w!("Local\\GrokComputerUse.Clipboard.v2")) }
                .map_err(|_| failure())?;
        // SAFETY: owned valid mutex handle; zero wait never blocks the thread.
        let status = unsafe { WaitForSingleObject(handle, 0) };
        if status == WAIT_OBJECT_0 {
            return Ok(Some(Self(handle)));
        }
        // SAFETY: abandoned acquisition still grants ownership; release it, then
        // fail closed for this action rather than adopting a crashed transaction.
        unsafe {
            if status == WAIT_ABANDONED {
                let _ = ReleaseMutex(handle);
            }
            let _ = CloseHandle(handle);
        }
        if status == WAIT_TIMEOUT {
            Ok(None)
        } else {
            Err(failure())
        }
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        // SAFETY: this thread acquired this mutex, and owns its only local handle.
        unsafe {
            if ReleaseMutex(self.0).is_err() {
                tracing::warn!("clipboard mutex release failed");
            }
            if CloseHandle(self.0).is_err() {
                tracing::warn!("clipboard mutex close failed");
            }
        }
    }
}
struct Transaction {
    _gate: Gate,
    saved: Vec<Format>,
    current: Vec<(u32, Vec<u8>)>,
    sequence: u32,
    restoring: bool,
    context: ActionContext,
}
struct State {
    window: HWND,
    transaction: Option<Transaction>,
    pending: Arc<AtomicBool>,
}
impl State {
    fn new(pending: Arc<AtomicBool>) -> Result<Self> {
        // SAFETY: system STATIC class, message-only owner window, no borrowed data
        // or application callbacks. Creation/destruction stay on this thread.
        let window = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("GCU clipboard owner"),
                Default::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .map_err(|_| failure())?;
        Ok(Self {
            window,
            transaction: None,
            pending,
        })
    }
    fn maintain(&mut self) {
        if self
            .transaction
            .as_ref()
            .is_some_and(|t| t.context.check().is_err())
            && self.restore().is_err()
        {
            tracing::warn!("cancelled clipboard recovery pending");
        }
    }
    fn stage(&mut self, text: String, context: ActionContext) -> Result<Option<u32>> {
        if self.transaction.is_some() {
            return Err(DriverError::CapabilityUnavailable);
        }
        let Some(gate) = Gate::acquire()? else {
            return Ok(None);
        };
        let _open = Open::new(self.window)?;
        // Snapshot/preallocate BEFORE clearing. Nonmaterializable/unsupported
        // formats refuse without losing the user's clipboard.
        let saved = data::snapshot()?
            .into_iter()
            .map(|(id, bytes)| Format::new(id, bytes))
            .collect::<Result<Vec<_>>>()?;
        let mut publication = text_formats::prepare(&text)?;
        context.check()?;
        // SAFETY: this thread holds OpenClipboard and owns a non-null HWND.
        unsafe { EmptyClipboard() }.map_err(|_| failure())?;
        self.pending.store(true, Ordering::Release);
        self.transaction = Some(Transaction {
            _gate: gate,
            saved,
            current: Vec::new(),
            sequence: data::sequence(),
            restoring: false,
            context: context.clone(),
        });
        let mut published = Ok(());
        for entry in publication.iter_mut() {
            if let Err(error) = self.write_format(entry) {
                published = Err(error);
                break;
            }
        }
        if let Err(error) = published {
            // Keep the same OS lock through initial rollback; no external writer
            // can slip between partial publication and restoration.
            if self.restore_locked().is_err() {
                return Err(DriverError::Native {
                    message: "clipboard publication recovery pending".into(),
                    maybe_executed: true,
                });
            }
            return Err(error);
        }
        Ok(self.transaction.as_ref().map(|t| t.sequence))
    }
    fn write_format(&mut self, entry: &mut Format) -> Result<()> {
        let published = entry.publish();
        if let Some(transaction) = &mut self.transaction {
            transaction.sequence = data::sequence();
            if published.is_ok() {
                transaction.current.push((entry.id, entry.bytes.clone()));
            }
        }
        published
    }
    fn restore(&mut self) -> Result<()> {
        if self.transaction.is_none() {
            return Ok(());
        }
        let _open = Open::new(self.window)?;
        self.restore_locked()
    }
    fn restore_locked(&mut self) -> Result<()> {
        let Some(t) = &mut self.transaction else {
            return Ok(());
        };
        // Check under the OS lock: no external copy can race the conditional
        // restore. Same text with a new sequence is external too.
        if data::sequence() != t.sequence || data::snapshot()? != t.current {
            self.transaction.take();
            self.pending.store(false, Ordering::Release);
            return Ok(());
        }
        if !t.restoring {
            // SAFETY: caller holds OpenClipboard on this thread.
            unsafe { EmptyClipboard() }.map_err(|_| failure())?;
            t.sequence = data::sequence();
            t.current.clear();
            t.restoring = true;
        }
        for entry in &mut t.saved {
            if entry.published() {
                continue;
            }
            let result = entry.publish();
            t.sequence = data::sequence();
            if result.is_ok() {
                t.current.push((entry.id, entry.bytes.clone()));
            }
            result?;
        }
        self.transaction.take();
        self.pending.store(false, Ordering::Release);
        Ok(())
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if self.restore().is_err() {
            tracing::warn!("clipboard owner final recovery failed");
        }
        // SAFETY: the message-only HWND was created on this same dedicated thread.
        if unsafe { DestroyWindow(self.window) }.is_err() {
            tracing::warn!("clipboard owner destruction failed");
        }
    }
}
