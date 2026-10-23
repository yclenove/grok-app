//! Cancellation-safe worker ownership, independent of Tokio runtime liveness.
#[cfg(feature = "script")]
use std::sync::Arc;
#[cfg(feature = "script")]
use std::{
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct Children {
    #[cfg(feature = "script")]
    shared: Arc<Mutex<Registry>>,
}
#[cfg(feature = "script")]
#[derive(Default)]
struct Registry {
    closing: bool,
    children: Vec<Arc<Mutex<Child>>>,
}
impl Children {
    /// Close admission, kill all workers, then reap with one total wall bound.
    /// Called before engine exit and synchronously on future/runtime drop.
    pub(crate) fn close(&self) -> bool {
        #[cfg(feature = "script")]
        {
            let children = {
                let mut registry = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                registry.closing = true;
                registry.children.clone()
            };
            for child in &children {
                let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
            }
            let end = Instant::now() + Duration::from_secs(1);
            children
                .iter()
                .map(|child| reap_until(child, end))
                .fold(true, |all, reaped| all & reaped)
        }
        #[cfg(not(feature = "script"))]
        true
    }

    #[cfg(feature = "script")]
    pub(crate) fn spawn(&self, command: &mut Command) -> std::io::Result<ChildOwner> {
        // Admission and registration are indivisible with respect to close.
        let mut registry = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if registry.closing {
            return Err(std::io::Error::other("worker owner closed"));
        }
        let child = Arc::new(Mutex::new(command.spawn()?));
        registry.children.push(child.clone());
        Ok(ChildOwner {
            child,
            done: Arc::new(AtomicBool::new(false)),
            owner: self.clone(),
        })
    }
}

#[cfg(feature = "script")]
pub(crate) struct ChildOwner {
    pub(crate) child: Arc<Mutex<Child>>,
    pub(crate) done: Arc<AtomicBool>,
    owner: Children,
}
#[cfg(feature = "script")]
impl Drop for ChildOwner {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        let _ = self.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        if reap_until(&self.child, Instant::now() + Duration::from_secs(1)) {
            self.owner
                .shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .children
                .retain(|child| !Arc::ptr_eq(child, &self.child));
        }
        // A failed reap stays registered: the engine barrier must fail closed.
    }
}
#[cfg(feature = "script")]
fn reap_until(child: &Mutex<Child>, end: Instant) -> bool {
    loop {
        match child.lock().unwrap_or_else(|e| e.into_inner()).try_wait() {
            Ok(Some(_)) => return true,
            Err(_) => return false,
            Ok(None) if Instant::now() >= end => return false,
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}
