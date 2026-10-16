//! Shared streaming image digest cache. All callers run this on blocking workers.
use gcu_core::driver::Cancel;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{self, Read},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::Duration,
};

// A 4 GiB image at a conservative 32 MiB/s takes 128s; leave 52s for I/O/scheduling.
// This read-only cold-hash budget is independent of DRIVER_DEADLINE (input/OS calls).
pub(crate) const HASH_TIMEOUT: Duration = Duration::from_secs(180);
pub(crate) const MAX_IMAGE_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const BUFFER_SIZE: usize = 64 * 1024;
const CACHE_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Stamp {
    pub dev: u64,
    pub inode: u64,
    pub size: u64,
    pub mtime: (i64, i64),
    // ctime also changes if content is rewritten and the old mtime is restored.
    pub ctime: (i64, i64),
}
#[cfg(unix)]
impl From<&std::fs::Metadata> for Stamp {
    fn from(meta: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        Self {
            dev: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
        }
    }
}

pub(super) trait ImageReader: Read {
    fn stamp(&self) -> io::Result<Stamp>;
}
#[cfg(unix)]
impl ImageReader for std::fs::File {
    fn stamp(&self) -> io::Result<Stamp> {
        let meta = self.metadata()?;
        if !meta.is_file() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Stamp::from(&meta))
    }
}

// Both production Tokio time and deterministic tests inject a monotonic clock.
pub(super) struct Control {
    pub cancel: Cancel,
    now: Arc<dyn Fn() -> Duration + Send + Sync>,
    deadline: Duration,
}
impl Control {
    pub fn new(cancel: Cancel, now: Arc<dyn Fn() -> Duration + Send + Sync>) -> Self {
        let deadline = now().saturating_add(HASH_TIMEOUT);
        Self {
            cancel,
            now,
            deadline,
        }
    }
    pub fn native(cancel: Cancel) -> Self {
        let epoch = tokio::time::Instant::now();
        Self::new(cancel, Arc::new(move || epoch.elapsed()))
    }
    pub fn check(&self) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            return Err(io::ErrorKind::Interrupted.into());
        }
        if (self.now)() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(())
    }
}

#[derive(Default)]
enum State {
    #[default]
    Empty,
    Working,
    Ready(String),
}
#[derive(Default)]
struct Slot {
    state: Mutex<State>,
    ready: Condvar,
}
#[derive(Default)]
pub(super) struct Cache(Mutex<HashMap<Stamp, Arc<Slot>>>);
pub(super) fn shared_cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(Cache::default)
}
fn poisoned<T>(_: T) -> io::Error {
    io::ErrorKind::InvalidData.into()
}

impl Cache {
    pub fn digest(&self, reader: &mut impl ImageReader, control: &Control) -> io::Result<String> {
        control.check()?;
        let stamp = reader.stamp()?;
        if stamp.size > MAX_IMAGE_SIZE {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let slot = {
            let mut slots = self.0.lock().map_err(poisoned)?;
            if !slots.contains_key(&stamp) && slots.len() >= CACHE_CAPACITY {
                // Only evict idle entries, never split an in-flight single-flight slot.
                let idle = slots
                    .iter()
                    .find(|(_, s)| Arc::strong_count(s) == 1)
                    .map(|(k, _)| *k);
                if let Some(idle) = idle {
                    slots.remove(&idle);
                } else {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
            }
            slots.entry(stamp).or_default().clone()
        };
        let mut state = slot.state.lock().map_err(poisoned)?;
        loop {
            control.check()?;
            match &*state {
                State::Ready(digest) => {
                    if reader.stamp()? != stamp {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    control.check()?;
                    return Ok(digest.clone());
                }
                State::Empty => {
                    *state = State::Working;
                    break;
                }
                State::Working => {
                    // Wake periodically to observe this waiter's cancellation/deadline.
                    state = slot
                        .ready
                        .wait_timeout(state, Duration::from_millis(10))
                        .map_err(poisoned)?
                        .0;
                }
            }
        }
        drop(state);
        let result = stream_digest(reader, stamp, control).and_then(|digest| {
            control.check()?;
            Ok(digest)
        });
        let mut state = slot.state.lock().map_err(poisoned)?;
        // A failed/cancelled computation leaves no digest, and wakes all waiters to retry.
        *state = match &result {
            Ok(digest) => State::Ready(digest.clone()),
            Err(_) => State::Empty,
        };
        slot.ready.notify_all();
        result
    }
}

fn stream_digest(
    reader: &mut impl ImageReader,
    stamp: Stamp,
    control: &Control,
) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; BUFFER_SIZE];
    let mut total = 0u64;
    loop {
        control.check()?;
        let count = match reader.read(&mut buffer) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        control.check()?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > stamp.size {
            return Err(io::ErrorKind::InvalidData.into());
        }
        hash.update(&buffer[..count]);
    }
    if total != stamp.size || reader.stamp()? != stamp {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let digest = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    control.check()?;
    Ok(digest)
}

#[cfg(test)]
#[path = "hash_tests.rs"]
mod tests;
