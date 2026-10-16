//! Portable fixture discovery diagnostics; timing comes from the harness clock.
use super::harness::{progress, Clock};
use gcu_core::driver::{AppQuery, Driver, WindowRef};
use serde_json::Value;
use std::time::Duration;

pub async fn discover<C: Clock>(
    clock: &mut C,
    budget: Duration,
    driver: &dyn Driver,
    executable: &str,
    pid: u32,
    read_oracle: impl Fn() -> Result<Value, String>,
) -> Result<WindowRef, String> {
    let start = clock.now();
    let mut attempt = 0;
    let mut last_resolve_error = None;
    loop {
        attempt += 1;
        let list_start = clock.now();
        let listed = driver.list_apps().await;
        let list_ms = clock.now().saturating_sub(list_start).as_millis();
        let resolve_start = clock.now();
        let resolved = driver.resolve_app(&AppQuery(executable.to_owned())).await;
        let resolve_ms = clock.now().saturating_sub(resolve_start).as_millis();
        if let Err(error) = &resolved {
            last_resolve_error = Some(error.to_string());
        }
        let windows_start = clock.now();
        let window = match resolved {
            Ok(app) if app.info.pid == Some(pid) => match driver.list_windows(Some(&app)).await {
                Ok(windows) => windows
                    .into_iter()
                    .find(|w| w.title == "GCU Fixture")
                    .map(|w| WindowRef { app, id: w.id })
                    .ok_or_else(|| "fixture window missing".to_owned()),
                Err(error) => Err(error.to_string()),
            },
            Ok(app) => Err(format!(
                "unexpected fixture PID: {:?}, expected {pid}",
                app.info.pid
            )),
            Err(error) => Err(error.to_string()),
        };
        let windows_ms = clock.now().saturating_sub(windows_start).as_millis();
        let elapsed = clock.now().saturating_sub(start);
        let diagnostic = format!(
            "attempt={attempt}, list_count={:?}, list_error={:?}, last_resolve_error={last_resolve_error:?}, \
             discovery_status={:?}, list_ms={list_ms}, resolve_ms={resolve_ms}, windows_ms={windows_ms}, \
             elapsed_ms={}, last_window={:?}, last_oracle={:?}",
            listed.as_ref().ok().map(Vec::len), listed.as_ref().err().map(ToString::to_string),
            driver.discovery_status(), elapsed.as_millis(), window.as_ref().err(), read_oracle(),
        );
        progress(format!("FIXTURE DISCOVERY: {diagnostic}"));
        if window.is_ok() {
            return window;
        }
        if elapsed >= budget {
            return Err(format!("fixture discovery: timeout; {diagnostic}"));
        }
        clock.tick().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_driver_windows::{
        testing::{medium_security, signed_image, window, FakeWinApi},
        WindowsDriver,
    };
    use std::sync::Arc;
    #[derive(Default)]
    struct Manual(Duration);
    impl Clock for Manual {
        fn now(&self) -> Duration {
            self.0
        }
        async fn tick(&mut self) {
            self.0 += Duration::from_millis(25);
        }
    }
    #[tokio::test]
    async fn discovery_timeout_reports_counts_errors_status_steps_and_last_oracle() {
        let driver = WindowsDriver::with_api(Arc::new(FakeWinApi::default()))
            .await
            .unwrap();
        let error = discover(
            &mut Manual::default(),
            Duration::ZERO,
            &driver,
            "C:\\missing.exe",
            7,
            || Ok(serde_json::json!({"counter": 3})),
        )
        .await
        .unwrap_err();
        for field in [
            "list_count=Some(0)",
            "last_resolve_error=Some(",
            "app not found",
            "discovery_status=",
            "indexing:",
            "list_ms=0",
            "resolve_ms=0",
            "windows_ms=0",
            "elapsed_ms=0",
            "last_oracle=Ok(",
            "counter",
        ] {
            assert!(error.contains(field), "missing {field}: {error}");
        }
    }
    #[tokio::test]
    async fn discovery_running_fixture_accepts_exact_external_pid_without_clock_ticks() {
        let api = Arc::new(FakeWinApi::default());
        api.update(|s| {
            s.paths.insert(7, "C:\\gcu-fixture.exe".into());
            s.images.insert(
                "C:\\gcu-fixture.exe".into(),
                signed_image("C:\\gcu-fixture.exe"),
            );
            s.processes.insert(7, medium_security());
            let mut w = window(8, 7);
            w.title = "GCU Fixture".into();
            s.windows.push(w);
        });
        let driver = WindowsDriver::with_api(api).await.unwrap();
        let mut clock = Manual::default();
        let window = discover(
            &mut clock,
            Duration::ZERO,
            &driver,
            "C:\\gcu-fixture.exe",
            7,
            || Ok(serde_json::json!({"counter": 0})),
        )
        .await
        .unwrap();
        assert_eq!(window.app.info.pid, Some(7));
        assert_eq!(clock.now(), Duration::ZERO);
    }
}
