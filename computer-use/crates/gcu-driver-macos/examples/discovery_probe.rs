//! Native discovery diagnostics. Initialize AppKit on main before the runtime.
//! Usage: discovery_probe [name] <absolute-app-path> [bundle-id].
#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !gcu_driver_macos::input::ensure_appkit() {
        return Err("main-thread AppKit initialization failed".into());
    }
    let mut args = std::env::args().skip(1);
    let name = args.next().unwrap_or_else(|| "fixture".into());
    let path = args
        .next()
        .or_else(|| std::env::var("GCU_FIXTURE_APP").ok())
        .ok_or(
            "usage: discovery_probe [name] <absolute-app-path> [bundle-id]; or set GCU_FIXTURE_APP",
        )?;
    let id = args
        .next()
        .unwrap_or_else(|| "com.grok.gcu-s02-fixture2".into());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(native::run(&name, &path, &id))
}

#[cfg(target_os = "macos")]
mod native {
    use gcu_core::driver::{AppQuery, Driver, Result};
    use gcu_driver_macos::MacosDriver;
    use std::{fmt::Debug, future::Future, time::Instant};

    pub async fn run(
        name: &str,
        path: &str,
        id: &str,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let started = Instant::now();
        println!("START MacosDriver::new");
        let driver = MacosDriver::new().await?;
        println!(
            "OK MacosDriver::new elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0
        );
        for label in ["list_apps first", "list_apps second"] {
            timed(label, async {
                driver.list_apps().await.map(|apps| apps.len())
            })
            .await;
            println!("discovery_status: {:?}", driver.discovery_status());
        }
        let mut selected = None;
        for (label, query) in [("name", name), ("path", path), ("bundle id", id)] {
            let query = AppQuery::from(query);
            let result = timed(
                &format!("resolve_app {label}: {}", query.0),
                driver.resolve_app(&query),
            )
            .await;
            if selected.is_none() {
                selected = result;
            }
        }
        timed("list_windows", driver.list_windows(selected.as_ref())).await;
        if let Some(app) = selected {
            timed("key_window", driver.key_window(&app)).await;
        } else {
            println!("key_window: skipped (all resolve_app queries failed)");
        }
        println!("discovery_status: {:?}", driver.discovery_status());
        Ok(())
    }

    async fn timed<T: Debug>(label: &str, call: impl Future<Output = Result<T>>) -> Option<T> {
        println!("START {label}");
        let started = Instant::now();
        match call.await {
            Ok(result) => {
                println!(
                    "OK {label} elapsed_ms={:.3} result={result:?}",
                    started.elapsed().as_secs_f64() * 1000.0
                );
                Some(result)
            }
            Err(error) => {
                println!(
                    "ERROR {label} elapsed_ms={:.3} error={error}",
                    started.elapsed().as_secs_f64() * 1000.0
                );
                None
            }
        }
    }
}
