//! This executable is built only for the test-worker-faults feature. All fault
//! injection is additionally absent in release builds, even with all features.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(debug_assertions)]
mod fixture;
#[cfg(debug_assertions)]
mod input_fault;
#[cfg(debug_assertions)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    use gcu_driver_windows::worker::{codec, serve, Reply, Request, Response};
    use serde::{Deserialize, Serialize};
    use std::{io::Write, path::PathBuf};
    use tokio::io::AsyncWriteExt;
    #[derive(Deserialize, Serialize)]
    struct Call {
        version: u32,
        id: u64,
        epoch: u64,
        namespace: String,
        request: Request,
    }
    let dir = PathBuf::from(std::env::var_os("GCU_TEST_DIR").unwrap());
    let mut starts = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("pids"))
        .unwrap();
    writeln!(starts, "{}", std::process::id()).unwrap();
    let fault = std::env::var("GCU_TEST_FAULT").unwrap_or_default();
    if fault == "fail_startup" {
        return;
    }
    if fault.starts_with("input_") {
        input_fault::run(dir, fault).await;
        return;
    }
    if matches!(
        fault.as_str(),
        "normal" | "normal_restart" | "normal_unknown" | "normal_hang"
    ) {
        let (input, mut forwarding) = tokio::io::duplex(64 * 1024);
        tokio::spawn(async move {
            let mut source = tokio::io::stdin();
            while let Some(call) = codec::read_frame::<_, Call>(&mut source).await.unwrap() {
                if matches!(call.request, Request::Perform(_)) {
                    let mut calls = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("calls"))
                        .unwrap();
                    writeln!(calls, "mutation").unwrap();
                    if fault == "normal_hang"
                        && std::fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(dir.join("once"))
                            .is_ok()
                    {
                        std::future::pending::<()>().await;
                    }
                }
                if fault == "normal_restart"
                    && matches!(call.request, Request::Snapshot(..))
                    && std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(dir.join("once"))
                        .is_ok()
                {
                    std::process::abort();
                }
                if codec::write_frame(&mut forwarding, &call).await.is_err() {
                    return;
                }
            }
        });
        serve(input, tokio::io::stdout(), |namespace| async move {
            Ok(fixture::driver(&namespace).await)
        })
        .await
        .unwrap();
        return;
    }
    let driver = fixture::driver("0").await;
    let mut input = tokio::io::stdin();
    let mut output = tokio::io::stdout();
    while let Some(call) = codec::read_frame::<_, Call>(&mut input).await.unwrap() {
        let reply = if matches!(call.request, Request::Doctor) {
            if fault == "hang_restart" && dir.join("once").exists() {
                std::future::pending::<()>().await;
            }
            let report = driver.doctor().await;
            Reply::Doctor {
                capabilities: report.capabilities,
                ready: report.ready,
                issues: report.issues,
            }
        } else {
            let mut calls = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("calls"))
                .unwrap();
            writeln!(
                calls,
                "{}",
                if call.request.is_read_only() {
                    "read"
                } else {
                    "mutation"
                }
            )
            .unwrap();
            let first = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dir.join("once"))
                .is_ok();
            if first || fault == "crash_always" {
                match fault.as_str() {
                    "crash_once" | "crash_always" | "hang_restart" => std::process::abort(),
                    "hang_once" => std::future::pending::<()>().await,
                    "bad_once" => {
                        output.write_all(&[0, 0, 0, 1, b'!']).await.unwrap();
                        output.flush().await.unwrap();
                        continue;
                    }
                    "truncated_once" => {
                        output.write_all(&[0, 0, 0, 8, b'{']).await.unwrap();
                        output.flush().await.unwrap();
                        return;
                    }
                    "overlong_once" => {
                        output.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
                        output.flush().await.unwrap();
                        continue;
                    }
                    "wrong_id_once" => {
                        codec::write_frame(
                            &mut output,
                            &Response {
                                version: call.version,
                                id: call.id + 1,
                                epoch: call.epoch,
                                result: Ok(Reply::Apps {
                                    apps: Vec::new(),
                                    indexing: false,
                                }),
                            },
                        )
                        .await
                        .unwrap();
                        continue;
                    }
                    "native_timeout_before" => {
                        codec::write_frame(
                            &mut output,
                            &Response {
                                version: call.version,
                                id: call.id,
                                epoch: call.epoch,
                                result: Err(gcu_driver_windows::worker::Failure(
                                    gcu_core::driver::DriverError::Timeout {
                                        maybe_executed: false,
                                    },
                                )),
                            },
                        )
                        .await
                        .unwrap();
                        continue;
                    }
                    _ => (),
                }
            }
            match call.request {
                Request::Perform(_) => {
                    Reply::Delivery(gcu_core::driver::Delivery::Accessibility.into())
                }
                _ => Reply::Apps {
                    apps: Vec::new(),
                    indexing: false,
                },
            }
        };
        codec::write_frame(
            &mut output,
            &Response {
                version: call.version,
                id: call.id,
                epoch: call.epoch,
                result: Ok(reply),
            },
        )
        .await
        .unwrap();
    }
}
#[cfg(not(debug_assertions))]
fn main() {}
