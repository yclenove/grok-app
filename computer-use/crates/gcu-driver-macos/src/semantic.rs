//! Live accessibility snapshot and semantic actions. Portable tests never call this.

use gcu_core::driver::{Cancel, Delivery, DriverAction, DriverError, Result};

use crate::driver::MacosDriver;

#[cfg(target_os = "macos")]
use crate::ax;

#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use gcu_core::{driver::TreeOpts, model::RawTree};

#[cfg(target_os = "macos")]
use crate::ax::ErasedToggle;
#[cfg(target_os = "macos")]
use crate::discovery::TreeTarget;

impl MacosDriver {
    #[cfg(target_os = "macos")]
    pub(crate) async fn snapshot_live(
        &self,
        target: &TreeTarget,
        opts: &TreeOpts,
        cancel: &Cancel,
        remaining: Duration,
    ) -> Result<RawTree> {
        let Some(thread) = self.ax.clone() else {
            return Err(DriverError::Native {
                message: "accessibility tree source is not connected".into(),
                maybe_executed: false,
            });
        };
        if ax::needs_enhanced_accessibility(&target.bundle_id, &target.app_name) {
            let toggles = Arc::new(ax::AccessibilityToggles::new(
                crate::real::AxToggleStore::new(Arc::clone(&thread)),
            ));
            let enabling = Arc::clone(&toggles);
            let pid = target.pid;
            let enabled = tokio::task::spawn_blocking(move || enabling.enable(pid))
                .await
                .map_err(|_| DriverError::Crashed {
                    maybe_executed: false,
                })?;
            enabled?;
            self.track_toggles(ArcToggle(toggles));
        }
        let Some(index) = target.ax_index else {
            return Ok(RawTree {
                root: None,
                incomplete: None,
            });
        };
        let source = crate::real::RealAxSource::open(thread, target.pid, index).await?;
        source.set_text_limit(opts.text_limit);
        let mut walk = opts.clone();
        walk.timeout = remaining.min(ax::TREE_BUILD_TIMEOUT);
        let root = source.root();
        ax::build_raw_tree(&source, &root, &walk, cancel, target.scale).await
    }

    pub(crate) async fn perform_live_action(
        &self,
        req: &DriverAction,
        cancel: &Cancel,
    ) -> Result<Delivery> {
        #[cfg(target_os = "macos")]
        if self.live_ax.load(Ordering::Acquire) {
            if let Some(thread) = &self.ax {
                let source = crate::real::RealAxSource::attach(Arc::clone(thread));
                return ax::perform(&source, req, cancel, gcu_core::driver::DRIVER_DEADLINE).await;
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (req, cancel);
        Err(action_source_missing())
    }
}

fn action_source_missing() -> DriverError {
    DriverError::Native {
        message: "accessibility action source is not connected".into(),
        maybe_executed: false,
    }
}

/// `Arc` wrapper so tracked toggles restore when the driver drops them.
#[cfg(target_os = "macos")]
struct ArcToggle<S: ax::ToggleStore>(Arc<ax::AccessibilityToggles<S>>);

#[cfg(target_os = "macos")]
impl<S: ax::ToggleStore + 'static> ErasedToggle for ArcToggle<S> {
    fn restore(&self) {
        self.0.restore();
    }
}
