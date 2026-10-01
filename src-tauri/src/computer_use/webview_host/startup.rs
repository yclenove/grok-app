//! Bounded fixture startup. A caller timeout must not leave a late live window.

use super::*;

#[derive(Clone)]
pub(super) struct Startup {
    alive: Arc<AtomicBool>,
    phase: Arc<Mutex<&'static str>>,
    deadline: Instant,
}

impl Startup {
    pub(super) fn new(alive: Arc<AtomicBool>) -> Self {
        Self {
            alive,
            phase: Arc::new(Mutex::new("native thread")),
            deadline: Instant::now() + Duration::from_secs(25),
        }
    }

    pub(super) fn stage(&self, phase: &'static str) -> Result<(), String> {
        *self.phase.lock() = phase;
        self.check()
    }

    pub(super) fn check(&self) -> Result<(), String> {
        if !self.alive.load(Ordering::SeqCst) || Instant::now() >= self.deadline {
            Err(format!(
                "owned WebView startup retired at {}",
                *self.phase.lock()
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn stop(&self) -> String {
        self.alive.store(false, Ordering::SeqCst);
        format!("webview bind host did not start at {}", *self.phase.lock())
    }

    pub(super) fn receive<T>(&self, rx: Receiver<T>) -> Result<T, String> {
        receive(rx, self.deadline, || self.check())
    }

    pub(super) fn wait_flag(&self, flag: &AtomicBool, budget: Duration) -> Result<(), String> {
        let deadline = self.deadline.min(Instant::now() + budget);
        loop {
            self.check()?;
            if Instant::now() >= deadline {
                return Err(format!("owned WebView {} timed out", *self.phase.lock()));
            }
            if flag.load(Ordering::SeqCst) {
                return Ok(());
            }
            pump_once();
        }
    }
}

/// Pump only the owning STA. Timeout/disconnection is never script completion.
pub(super) fn receive<T>(
    rx: Receiver<T>,
    deadline: Instant,
    current: impl Fn() -> Result<(), String>,
) -> Result<T, String> {
    loop {
        current()?;
        if Instant::now() >= deadline {
            return Err("owned native reply timed out; outcome unknown".into());
        }
        match rx.try_recv() {
            Ok(value) => return Ok(value),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("owned native reply channel closed; outcome unknown".into());
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        pump_once();
    }
}

fn pump_once() {
    let mut msg = MSG::default();
    unsafe {
        if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        } else {
            thread::sleep(Duration::from_millis(8));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_start_fences_even_an_already_arrived_native_reply() {
        let alive = Arc::new(AtomicBool::new(true));
        let startup = Startup::new(alive.clone());
        startup.stage("environment").unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(42).unwrap();
        assert!(startup.stop().contains("environment"));
        assert!(!alive.load(Ordering::SeqCst));
        assert!(startup.receive(rx).is_err());
        assert!(startup.stage("controller").is_err());
        assert!(startup
            .wait_flag(&AtomicBool::new(true), Duration::from_secs(1))
            .is_err());
    }

    #[test]
    fn callback_disconnect_and_elapsed_deadline_are_reported_without_waiting() {
        let (tx, rx) = mpsc::channel::<()>();
        drop(tx);
        assert!(
            receive(rx, Instant::now() + Duration::from_secs(1), || Ok(()))
                .unwrap_err()
                .contains("channel closed")
        );
        let (_tx, rx) = mpsc::channel::<()>();
        assert!(receive(rx, Instant::now(), || Ok(()))
            .unwrap_err()
            .contains("timed out"));
    }

    #[test]
    fn current_start_receives_its_native_reply() {
        let startup = Startup::new(Arc::new(AtomicBool::new(true)));
        let (tx, rx) = mpsc::channel();
        tx.send(7).unwrap();
        assert_eq!(startup.receive(rx).unwrap(), 7);
        startup
            .wait_flag(&AtomicBool::new(true), Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn a_shorter_stage_budget_is_not_extended_by_the_overall_startup_deadline() {
        let startup = Startup::new(Arc::new(AtomicBool::new(true)));
        startup.stage("navigation").unwrap();
        let error = startup
            .wait_flag(&AtomicBool::new(true), Duration::ZERO)
            .unwrap_err();
        assert!(error.contains("navigation timed out"));
    }
}
