//! Foreground permission is represented solely by the route core supplied.
use super::api::{InputApi, Target};
use crate::uia::patterns::ActionContext;
use gcu_core::driver::{DriverError, Result};
/// Activate then verify the actual foreground HWND. No input follows a failed
/// activation, and cancellation after activation preserves uncertainty.
pub async fn activate(api: &dyn InputApi, target: &Target, context: &ActionContext) -> Result<()> {
    context.check()?;
    if api.foreground().await? != target.window.hwnd {
        api.activate(target.clone(), context.clone()).await?;
        for _ in 0..10 {
            context.check()?;
            if api.foreground().await? == target.window.hwnd {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
    verify(api, target).await
}
/// Verify immediately before each global packet; never inject into another app.
pub async fn verify(api: &dyn InputApi, target: &Target) -> Result<()> {
    if api.foreground().await? == target.window.hwnd {
        Ok(())
    } else {
        Err(DriverError::CapabilityUnavailable)
    }
}
