//! Preserve the owned STA's terminal outcome, including concurrent/repeated joins.

use std::thread::JoinHandle;

use parking_lot::Mutex;

pub(super) struct NativeThread(Mutex<State>);

struct State {
    handle: Option<JoinHandle<Result<(), String>>>,
    result: Option<Result<(), String>>,
}

impl NativeThread {
    pub(super) fn new(handle: JoinHandle<Result<(), String>>) -> Self {
        Self(Mutex::new(State {
            handle: Some(handle),
            result: None,
        }))
    }

    pub(super) fn join(&self) -> Result<(), String> {
        // Keep the lock while joining. A second caller must not interpret a
        // taken handle as completion while the first caller still waits on it.
        let mut state = self.0.lock();
        if let Some(handle) = state.handle.take() {
            state.result = Some(handle.join().unwrap_or_else(|_| {
                Err("owned native thread panicked; cleanup unconfirmed".into())
            }));
        }
        state
            .result
            .clone()
            .unwrap_or_else(|| Err("owned native shutdown result unavailable".into()))
    }
}

pub(super) fn combine<T>(
    primary: Result<T, String>,
    cleanup: Result<(), String>,
) -> Result<T, String> {
    match (primary, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(primary), Err(cleanup)) => Err(format!(
            "{primary}; owned native cleanup also failed: {cleanup}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn a_joined_thread_error_is_retained_for_every_later_caller() {
        let owner = NativeThread::new(thread::spawn(|| Err("environment exit timeout".into())));
        assert_eq!(owner.join(), Err("environment exit timeout".into()));
        assert_eq!(owner.join(), Err("environment exit timeout".into()));
    }

    #[test]
    fn native_thread_panic_is_not_successful_cleanup() {
        let owner = NativeThread::new(thread::spawn(|| panic!("owned fixture panic")));
        let error = owner.join().unwrap_err();
        assert!(error.contains("cleanup unconfirmed"));
        assert_eq!(owner.join(), Err(error));
    }

    #[test]
    fn concurrent_joiners_wait_for_the_same_terminal_outcome() {
        let (release, blocked) = mpsc::channel();
        let owner = Arc::new(NativeThread::new(thread::spawn(move || {
            blocked.recv().unwrap();
            Err("native close rejected".into())
        })));
        let (finished, results) = mpsc::channel();
        let joins: Vec<_> = (0..2)
            .map(|_| {
                let owner = owner.clone();
                let finished = finished.clone();
                thread::spawn(move || finished.send(owner.join()).unwrap())
            })
            .collect();
        assert!(matches!(results.try_recv(), Err(mpsc::TryRecvError::Empty)));
        release.send(()).unwrap();
        for _ in 0..2 {
            assert_eq!(
                results.recv_timeout(Duration::from_secs(5)).unwrap(),
                Err("native close rejected".into())
            );
        }
        for join in joins {
            join.join().unwrap();
        }
    }

    #[test]
    fn successful_cleanup_is_cached_without_rejoining() {
        let owner = NativeThread::new(thread::spawn(|| Ok(())));
        assert_eq!(owner.join(), Ok(()));
        assert_eq!(owner.join(), Ok(()));
    }

    #[test]
    fn action_and_cleanup_failures_are_both_preserved() {
        assert_eq!(combine(Ok(7), Ok(())), Ok(7));
        assert_eq!(
            combine::<()>(Err("action".into()), Ok(())),
            Err("action".into())
        );
        assert_eq!(combine(Ok(7), Err("cleanup".into())), Err("cleanup".into()));
        let error = combine::<()>(Err("action".into()), Err("cleanup".into())).unwrap_err();
        assert!(error.starts_with("action;"));
        assert!(error.ends_with("cleanup"));
    }
}
