use super::{emit, select};
use gcu_core::driver::{AppQuery, Driver, WindowRef};
use gcu_driver_macos::{diagnostics, MacosDriver};
use serde_json::json;
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub(super) async fn window(
    driver: &MacosDriver,
    path: &Path,
) -> Result<WindowRef, Box<dyn std::error::Error>> {
    let query = AppQuery::from(path.to_str().ok_or("fixture path must be UTF-8")?);
    let app = driver.resolve_app(&query).await?;
    emit("RESOLVE", json!(app.info));
    if app.info.pid.is_none() {
        emit("LAUNCH", json!(driver.launch_app(&app).await?.info));
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reopened = false;
    loop {
        // Refresh the process binding after launch/reopen. Never manufacture
        // a WindowRef from a raw AX index or use another app's window.
        let app = driver.resolve_app(&query).await?;
        let started = Instant::now();
        let mut listed = driver.list_windows(Some(&app)).await;
        emit(
            "WINDOW_LIST",
            match &listed {
                Ok(windows) => json!({"pid": app.info.pid, "windows": windows,
                "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}),
                Err(error) => json!({"pid": app.info.pid, "error": error.to_string(),
                "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}),
            },
        );
        let started = Instant::now();
        let keyed = driver.key_window(&app).await;
        emit(
            "WINDOW_KEY",
            match &keyed {
                Ok(win) => {
                    json!({"window": win, "elapsed_ms": started.elapsed().as_secs_f64()*1000.0})
                }
                Err(error) => {
                    json!({"error": error.to_string(), "elapsed_ms": started.elapsed().as_secs_f64()*1000.0})
                }
            },
        );
        // key_window may have just unhidden/restored the fixture. Refresh the
        // list before considering an activating reopen.
        if keyed.as_ref().is_ok_and(|key| {
            !listed
                .as_ref()
                .is_ok_and(|rows| rows.iter().any(|w| w.id == key.id))
        }) {
            listed = driver.list_windows(Some(&app)).await;
            emit(
                "WINDOW_LIST_AFTER_KEY",
                match &listed {
                    Ok(windows) => json!({"pid": app.info.pid, "windows": windows}),
                    Err(error) => json!({"pid": app.info.pid, "error": error.to_string()}),
                },
            );
        }
        if let Some(pid) = app.info.pid {
            if let Some(info) =
                select::window(listed.as_deref().unwrap_or(&[]), keyed.as_ref().ok(), pid)
            {
                emit(
                    "WINDOW",
                    json!({"window": info, "fixture_reopened": reopened,
                    "source": if keyed.as_ref().is_ok_and(|key| key.id == info.id) { "key_window + list_windows" } else { "list_windows largest fixture window" }}),
                );
                report(driver, pid).await;
                return Ok(WindowRef { app, id: info.id });
            }
            if !reopened {
                report(driver, pid).await;
            }
        }
        // The previous probe only unhid an already-running app. A fixture
        // without an on-screen window needs an explicit reopen, and may need
        // to be foreground once before AccessKit publishes its view.
        if !reopened {
            reopened = true;
            reopen(path).await?;
        }
        if Instant::now() >= deadline {
            if let Some(pid) = app.info.pid {
                report(driver, pid).await;
            }
            return Err(
                "fixture has no listed window after reopen; inspect WINDOW_NATIVE CG/AX facts"
                    .into(),
            );
        }
        emit(
            "WINDOW_WAIT",
            json!({"pid": app.info.pid, "fixture_reopened": reopened}),
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn report(driver: &MacosDriver, pid: u32) {
    match diagnostics::fixture_window_report(driver, pid).await {
        Ok(facts) => println!("{facts}"),
        Err(error) => emit(
            "WINDOW_NATIVE_ERROR",
            json!({"pid": pid, "error": error.to_string()}),
        ),
    }
}

async fn reopen(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    emit(
        "FIXTURE_REOPEN",
        json!({"path": path, "activates": true,
        "reason": "no on-screen fixture window; explicit fixture-only open/reopen"}),
    );
    let started = Instant::now();
    let mut child = std::process::Command::new("/usr/bin/open")
        .arg("-a")
        .arg(path)
        .spawn()?;
    loop {
        if let Some(status) = child.try_wait()? {
            emit(
                "FIXTURE_REOPEN_RESULT",
                json!({"success": status.success(), "code": status.code(),
                "elapsed_ms": started.elapsed().as_secs_f64()*1000.0}),
            );
            return if status.success() {
                Ok(())
            } else {
                Err("fixture reopen failed".into())
            };
        }
        if started.elapsed() >= Duration::from_secs(5) {
            child.kill()?;
            child.wait()?;
            return Err("fixture reopen exceeded 5s".into());
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
