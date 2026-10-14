//! Private worker entry; stdout contains only framed responses.
use crate::exit::Failure;

pub(crate) async fn run() -> Result<u8, Failure> {
    #[cfg(windows)]
    {
        gcu_driver_windows::worker::serve_with_input(
            tokio::io::stdin(),
            tokio::io::stdout(),
            |namespace, tracker| async move {
                Ok(std::sync::Arc::new(
                    gcu_driver_windows::WindowsDriver::new_worker_with_input(&namespace, tracker)
                        .await?,
                )
                    as std::sync::Arc<dyn gcu_core::driver::Driver>)
            },
        )
        .await
        .map_err(|_| Failure::internal("Worker connection terminated"))?;
        Ok(0)
    }
    #[cfg(not(windows))]
    Err(Failure::arguments("Native worker requires Windows"))
}
