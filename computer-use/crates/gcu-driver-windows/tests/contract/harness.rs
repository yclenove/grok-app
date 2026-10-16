#![cfg_attr(not(windows), allow(dead_code))]
use serde::Serialize;
use serde_json::Value;
use std::{future::Future, path::Path, time::Duration};

pub const CASES: &[(&str, &str)] = &[
    ("C01", "discovery and exact resolution"),
    ("C02", "launch without activation"),
    ("C03", "visible controls and roles"),
    ("C04", "physical capture and occluded marker pixels"),
    ("C05", "semantic press exactly once"),
    ("C06", "physical grid click"),
    ("C07", "text SetValue"),
    ("C08", "Unicode, ZWJ and newline input"),
    ("C09", "Ctrl+A and BackSpace"),
    ("C10", "checkbox toggle"),
    ("C11", "third dropdown item"),
    ("C12", "slider SetValue"),
    ("C13", "two-page list scroll"),
    ("C14", "modal lifecycle and priority"),
    ("C15", "drag to target"),
    ("C16", "background press and SetValue"),
    ("C17", "old screenshot geometry gate"),
    ("C18", "deleted element never substituted"),
    ("C19", "closed window never substituted"),
    ("C20", "password and all image derivatives redacted"),
    ("C21", "overlay exclusion pixels"),
    ("C22", "worker timeout is unknown without replay"),
    ("C23", "interrupted Shift release"),
    ("C24", "virtual list range and bounded read"),
    ("C25", "same-geometry screenshot content gate"),
    ("C26", "execution surfaces denied"),
    ("C27", "independent Stop and listener failure"),
    ("W-UIPI", "higher integrity refused on every route"),
    ("W-HOTKEY", "native hotkey conflict disables foreground"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "reason", rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Skip(String),
    NotRun(String),
    Failed(String),
}
pub type CaseResult = Result<Outcome, String>;

// Preserve the final table report even when a test assertion/provider panics.
// The panic hook retains the libtest diagnostic; the row also receives its last
// external fixture state from the caller. No native operation is retried.
pub async fn attempt<T>(work: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    let mut work = Box::pin(work);
    std::future::poll_fn(move |cx| {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work.as_mut().poll(cx))) {
            Ok(result) => result,
            Err(_) => {
                std::task::Poll::Ready(Err("contract panicked; see libtest diagnostic".into()))
            }
        }
    })
    .await
}

// A row has its own deadline, independent of the outer process-tree watchdog.
// Dropping timed-out work never restarts it. Callers append the external oracle.
pub async fn attempt_bounded<T, C: Clock>(
    clock: &mut C,
    budget: Duration,
    label: &str,
    work: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let start = clock.now();
    let mut work = Box::pin(attempt(work));
    loop {
        tokio::select! {
            biased;
            result = &mut work => return result,
            () = clock.tick() => {}
        }
        if clock.now().saturating_sub(start) >= budget {
            return Err(format!(
                "{label}: timeout after {}ms; not replayed",
                budget.as_millis()
            ));
        }
    }
}

pub fn progress(message: impl std::fmt::Display) {
    use std::io::Write;
    println!("{message}");
    std::io::stdout().flush().unwrap();
}

#[derive(Default, Serialize)]
pub struct Report {
    pub passed: usize,
    pub skipped: usize,
    pub not_run: usize,
    pub failed: usize,
    pub rows: Vec<(&'static str, Outcome)>,
}
impl Report {
    pub fn record(&mut self, id: &'static str, result: CaseResult) {
        let outcome = result.unwrap_or_else(Outcome::Failed);
        match &outcome {
            Outcome::Passed => self.passed += 1,
            Outcome::Skip(reason) => {
                assert!(!reason.trim().is_empty(), "Skip requires a reason");
                self.skipped += 1;
            }
            Outcome::NotRun(_) => self.not_run += 1,
            Outcome::Failed(_) => self.failed += 1,
        }
        let status = match &outcome {
            Outcome::Passed => "PASS",
            Outcome::Skip(_) => "SKIP",
            Outcome::NotRun(_) => "NOT_RUN",
            Outcome::Failed(_) => "FAIL",
        };
        progress(format!("[{id}] {status}: {outcome:?}"));
        self.rows.push((id, outcome));
        self.pending_snapshot().save();
    }
    pub fn first_failure(&self) -> Option<String> {
        self.rows.iter().find_map(|(id, outcome)| match outcome {
            Outcome::Failed(reason) => Some(format!("{id}: {reason}")),
            _ => None,
        })
    }
    // A killed suite retains completed rows and marks unreached rows honestly.
    pub fn pending_snapshot(&self) -> Self {
        let mut snapshot = Self {
            passed: self.passed,
            skipped: self.skipped,
            not_run: self.not_run,
            failed: self.failed,
            rows: self.rows.clone(),
        };
        for (id, _) in CASES {
            if !self.rows.iter().any(|(done, _)| done == id) {
                snapshot.not_run += 1;
                snapshot.rows.push((
                    id,
                    Outcome::NotRun("case did not finish; suite may have been interrupted".into()),
                ));
            }
        }
        snapshot
    }
    pub fn save(&self) {
        if let Some(path) = std::env::var_os("GCU_CONTRACT_REPORT") {
            let path = std::path::PathBuf::from(path);
            let temporary = path.with_extension("pending");
            std::fs::write(&temporary, serde_json::to_vec_pretty(self).unwrap()).unwrap();
            std::fs::rename(temporary, path).unwrap();
        }
    }
    pub fn finish(&self) {
        progress(format!(
            "SUMMARY: {} passed, {} skipped, {} not_run, {} failed",
            self.passed, self.skipped, self.not_run, self.failed
        ));
        if let Some(reason) = self.first_failure() {
            progress(format!("FIRST FAILURE: {reason}"));
        }
        self.save();
    }
}

/// Polling uses an injected monotonic clock, including the timeout boundary.
/// Native callers use Tokio; unit tests advance a logical clock without sleeping.
pub trait Clock {
    fn now(&self) -> Duration;
    fn tick(&mut self) -> impl Future<Output = ()>;
    async fn wait(&mut self, duration: Duration) {
        let start = self.now();
        while self.now().saturating_sub(start) < duration {
            self.tick().await;
        }
    }
}
#[cfg(windows)]
pub struct DesktopClock(tokio::time::Instant);
#[cfg(windows)]
impl Default for DesktopClock {
    fn default() -> Self {
        Self(tokio::time::Instant::now())
    }
}
#[cfg(windows)]
impl Clock for DesktopClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
    async fn tick(&mut self) {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    async fn wait(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

pub async fn poll<T: std::fmt::Debug, C: Clock>(
    clock: &mut C,
    budget: Duration,
    label: &str,
    mut read: impl FnMut() -> Result<T, String>,
    accept: impl Fn(&T) -> bool,
) -> Result<T, String> {
    let start = clock.now();
    loop {
        let last = read();
        if let Ok(value) = &last {
            if accept(value) {
                return last;
            }
        }
        if clock.now().saturating_sub(start) >= budget {
            return Err(format!("{label}: timeout; last oracle={last:?}"));
        }
        clock.tick().await;
    }
}
pub async fn poll_async<T: std::fmt::Debug, C: Clock>(
    clock: &mut C,
    budget: Duration,
    label: &str,
    mut read: impl AsyncFnMut() -> Result<T, String>,
    accept: impl Fn(&T) -> bool,
) -> Result<T, String> {
    let start = clock.now();
    loop {
        let last = read().await;
        if let Ok(value) = &last {
            if accept(value) {
                return last;
            }
        }
        if clock.now().saturating_sub(start) >= budget {
            return Err(format!("{label}: timeout; last state={last:?}"));
        }
        clock.tick().await;
    }
}
pub fn read_oracle(path: &Path) -> Result<Value, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn require(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Manual(Duration);
    impl Clock for Manual {
        fn now(&self) -> Duration {
            self.0
        }
        async fn tick(&mut self) {
            self.0 += Duration::from_millis(25);
        }
    }
    #[test]
    fn table_unique_ids_covers_all_contracts_and_uipi() {
        assert_eq!(CASES.len(), 29);
        for i in 1..=27 {
            assert_eq!(
                CASES.iter().filter(|r| r.0 == format!("C{i:02}")).count(),
                1
            );
        }
    }
    #[test]
    fn report_keeps_skip_not_run_and_failure_distinct() {
        let mut report = Report::default();
        report.record("C01", Ok(Outcome::Passed));
        report.record("C02", Ok(Outcome::Skip("scope: no launch fixture".into())));
        report.record("C03", Ok(Outcome::NotRun("no Windows desktop".into())));
        report.record("C04", Err("incorrect pixels".into()));
        assert_eq!(
            (report.passed, report.skipped, report.not_run, report.failed),
            (1, 1, 1, 1)
        );
    }
    #[tokio::test]
    async fn panic_records_failed_row_without_losing_report_totals() {
        let result = attempt::<Outcome>(async { panic!("synthetic assertion failure") }).await;
        let mut report = Report::default();
        report.record("C05", result);
        report.record("C06", Ok(Outcome::NotRun("desktop lost".into())));
        assert_eq!((report.failed, report.not_run, report.passed), (1, 1, 0));
    }
    #[tokio::test]
    async fn case_deadline_injected_clock_drops_pending_work_without_replay() {
        let mut clock = Manual(Duration::ZERO);
        let polls = std::cell::Cell::new(0);
        let dropped = std::cell::Cell::new(false);
        struct DropFlag<'a>(&'a std::cell::Cell<bool>);
        impl Drop for DropFlag<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let flag = DropFlag(&dropped);
        let work = async {
            let _flag = flag;
            std::future::poll_fn(|_| {
                polls.set(polls.get() + 1);
                std::task::Poll::<Result<Outcome, String>>::Pending
            })
            .await
        };
        let error = attempt_bounded(&mut clock, Duration::from_millis(50), "C05", work)
            .await
            .unwrap_err();
        assert_eq!(clock.0, Duration::from_millis(50));
        assert_eq!(polls.get(), 2, "one future is polled, never restarted");
        assert!(dropped.get());
        let mut report = Report::default();
        report.record("C05", Err(format!("{error}; last fixture=counter:0")));
        assert_eq!(report.failed, 1);
        assert!(report.first_failure().unwrap().contains("C05: timeout"));
        assert!(report.first_failure().unwrap().contains("counter:0"));
        assert_eq!(report.pending_snapshot().not_run, 28);
    }
    #[tokio::test]
    async fn case_deadline_preserves_ready_error_and_panic_before_timeout() {
        let mut clock = Manual(Duration::ZERO);
        let result = attempt_bounded(&mut clock, Duration::ZERO, "ready", async { Ok(7) }).await;
        assert_eq!(result, Ok(7));
        let result = attempt_bounded::<(), _>(&mut clock, Duration::ZERO, "failed", async {
            Err("first oracle mismatch".into())
        })
        .await;
        assert_eq!(result, Err("first oracle mismatch".into()));
        let result = attempt_bounded::<(), _>(&mut clock, Duration::ZERO, "panic", async {
            panic!("synthetic provider failure")
        })
        .await;
        assert!(result.unwrap_err().contains("contract panicked"));
        assert_eq!(clock.0, Duration::ZERO);
        let mut report = Report::default();
        report.record("C01", Err("first oracle mismatch".into()));
        report.record("C02", Err("later failure".into()));
        assert_eq!(
            report.first_failure().as_deref(),
            Some("C01: first oracle mismatch")
        );
    }
    #[tokio::test]
    async fn asynchronous_poll_injected_clock_bounds_pending_observations() {
        let mut clock = Manual(Duration::ZERO);
        let error = poll_async(
            &mut clock,
            Duration::from_millis(50),
            "UIA ready",
            async || Ok(serde_json::json!({"ready":false})),
            |v| v["ready"] == true,
        )
        .await
        .unwrap_err();
        assert_eq!(clock.0, Duration::from_millis(50));
        assert!(error.contains("last state=Ok(Object"));
        assert!(error.contains("Bool(false)"));
    }
    #[tokio::test]
    async fn poll_injected_clock_reports_last_wrong_value_and_read_error() {
        let mut clock = Manual(Duration::ZERO);
        let error = poll(
            &mut clock,
            Duration::from_millis(50),
            "counter",
            || Ok(7),
            |v| *v == 8,
        )
        .await
        .unwrap_err();
        assert!(error.contains("last oracle=Ok(7)"));
        assert_eq!(clock.0, Duration::from_millis(50));
        let error = poll::<Value, _>(
            &mut clock,
            Duration::ZERO,
            "json",
            || Err("torn JSON".into()),
            |_| true,
        )
        .await
        .unwrap_err();
        assert!(error.contains("torn JSON"));
    }
    #[tokio::test]
    async fn oracle_external_file_rejects_false_success_and_accepts_exact_unicode() {
        let dir = std::env::temp_dir().join(format!("wc{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, br#"{"text":"wrong","counter":0}"#).unwrap();
        let mut clock = Manual(Duration::ZERO);
        assert!(poll(
            &mut clock,
            Duration::ZERO,
            "counter",
            || read_oracle(&path),
            |v| v["counter"] == 1
        )
        .await
        .is_err());
        let text = "中文 👩‍💻\nsecond line";
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"text":text})).unwrap(),
        )
        .unwrap();
        assert_eq!(read_oracle(&path).unwrap()["text"], text);
        std::fs::write(&path, b"{").unwrap();
        assert!(read_oracle(&path).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
