//! Exclusive ownerless prewarm consumption. No map lock crosses a kill or RPC.
use super::*;
use crate::permission::PermissionPolicy;
use std::path::Path;
use std::time::Duration;

pub(super) enum PrewarmSelection {
    Reused(PrewarmedProcess),
    Rejected(PrewarmedProcess, &'static str),
    KeptForCwd(String),
    Waiting,
    Unavailable,
}

impl SessionManager {
    #[cfg(test)]
    pub(super) fn take_prewarm(
        &self,
        policy: PermissionPolicy,
        effort: &str,
        sandbox: &str,
        cwd: &Path,
    ) -> PrewarmSelection {
        self.take_prewarm_for_session(policy, effort, sandbox, cwd, None)
    }

    pub(super) fn take_prewarm_for_session(
        &self,
        policy: PermissionPolicy,
        effort: &str,
        sandbox: &str,
        cwd: &Path,
        session_created: Option<chrono::DateTime<chrono::Utc>>,
    ) -> PrewarmSelection {
        // Configuration IO is outside the slot lock and has no repair writes.
        let target =
            crate::providers::ProcessConfigBinding::current(&crate::store::load_settings());
        let custom = target.as_ref().is_ok_and(|(custom, _)| *custom);
        let mut slot = self.prewarm.lock();
        let selection = match std::mem::replace(&mut *slot, PrewarmState::None) {
            PrewarmState::Ready(p) => {
                let process_cwd = p.acp.cwd().to_string_lossy();
                if !target
                    .as_ref()
                    .is_ok_and(|(_, configuration)| p.acp.matches_process_config(configuration))
                {
                    // An obsolete config is not useful even for its original
                    // sandbox cwd. Hand it to the caller for bounded recycling.
                    PrewarmSelection::Rejected(p, "provider or configuration changed")
                } else if !Self::reuse_sandbox_cwd_ok(
                    p.sandbox_profile.as_deref(),
                    process_cwd.as_ref(),
                    cwd.to_string_lossy().as_ref(),
                ) {
                    let previous = process_cwd.into_owned();
                    *slot = PrewarmState::Ready(p);
                    PrewarmSelection::KeptForCwd(previous)
                } else if session_created
                    .is_some_and(|created| prewarm_predates_session(p.created_wall, created))
                {
                    PrewarmSelection::Rejected(p, "started before this chat; rescan skills")
                } else if Self::reuse_gate(
                    p.acp.is_alive(),
                    p.policy,
                    p.effort.as_deref(),
                    p.sandbox_profile.as_deref(),
                    p.acp.is_custom_route(),
                    policy,
                    effort,
                    sandbox,
                    custom,
                ) {
                    PrewarmSelection::Reused(p)
                } else {
                    PrewarmSelection::Rejected(p, "process flags changed")
                }
            }
            PrewarmState::Spawning { since } if since.elapsed() < Duration::from_millis(2500) => {
                *slot = PrewarmState::Spawning { since };
                PrewarmSelection::Waiting
            }
            other => {
                *slot = other;
                PrewarmSelection::Unavailable
            }
        };
        drop(slot);
        // A config edit can occur while waiting for the slot lock. Re-read
        // after exclusive consumption, before returning it to session/open.
        match selection {
            PrewarmSelection::Reused(p) if !p.acp.prewarm_config_is_current() => {
                PrewarmSelection::Rejected(p, "provider or configuration changed")
            }
            other => other,
        }
    }
}
