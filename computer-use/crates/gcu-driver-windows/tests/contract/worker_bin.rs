//! Debug-only native worker adversary. No injection surface in shipping gcu.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(all(windows, debug_assertions))]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    use gcu_core::driver::DriverError;
    use gcu_driver_windows::{
        worker::{codec, serve_with_input, Failure, Reply, Response},
        WindowsDriver,
    };
    use std::{io::Write, path::PathBuf, sync::Arc};
    let dir = PathBuf::from(std::env::var_os("GCU_FAULT_DIR").unwrap());
    let mut starts = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("starts"))
        .unwrap();
    writeln!(starts, "{}", std::process::id()).unwrap();
    let (server_out, mut observer) = tokio::io::duplex(64 * 1024);
    let forward = tokio::spawn(async move {
        let mut output = tokio::io::stdout();
        while let Some(mut response) = codec::read_frame::<_, Response>(&mut observer)
            .await
            .unwrap()
        {
            if matches!(response.result, Ok(Reply::Delivery(_))) {
                // The real native action has finished. An independent append-only
                // trace and fixture state prove effects, not this response.
                let mut calls = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("calls"))
                    .unwrap();
                writeln!(calls, "mutation").unwrap();
                response.result = Err(Failure(DriverError::Timeout {
                    maybe_executed: true,
                }));
            }
            if codec::write_frame(&mut output, &response).await.is_err() {
                break;
            }
        }
    });
    let _ = serve_with_input(
        tokio::io::stdin(),
        server_out,
        |namespace, tracker| async move {
            Ok(
                Arc::new(WindowsDriver::new_worker_with_input(&namespace, tracker).await?)
                    as Arc<dyn gcu_core::driver::Driver>,
            )
        },
    )
    .await;
    forward.await.unwrap();
}
#[cfg(not(all(windows, debug_assertions)))]
fn main() {}
