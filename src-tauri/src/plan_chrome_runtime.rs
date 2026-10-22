//! Prepared Plan writes keep their original store and reservation order.
//! Reservation never waits for disk/effect locks; commit runs outside pool guards.
use super::{cap_body, cap_entries, plan_chrome_path, PlanChromeStored};
use parking_lot::Mutex;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock, Weak,
    },
    time::Instant,
};

struct Lane {
    id: uuid::Uuid,
    revision: AtomicU64,
    applied: AtomicU64,
    published: AtomicU64,
    live_rpc_revision: AtomicU64,
    close_target: Mutex<(u64, Option<String>)>,
    effects: Mutex<()>,
}

// UI reads must keep an idle clock alive until the corresponding close. Keep
// one lease per displayed path, bounded independently of historical replies.
type UiLeases = HashMap<PathBuf, (Arc<Lane>, Instant)>;

fn ui_leases() -> &'static Mutex<UiLeases> {
    static LEASES: OnceLock<Mutex<UiLeases>> = OnceLock::new();
    LEASES.get_or_init(Default::default)
}

#[derive(Clone)]
pub(crate) struct PlanChromeClock {
    path: PathBuf,
    lane: Arc<Lane>,
}

impl PlanChromeClock {
    pub(crate) fn for_session(session_id: &str) -> Self {
        static LANES: OnceLock<Mutex<HashMap<PathBuf, Weak<Lane>>>> = OnceLock::new();
        let path = plan_chrome_path(session_id);
        let mut lanes = LANES.get_or_init(Default::default).lock();
        let lane = lanes.get(&path).and_then(Weak::upgrade).unwrap_or_else(|| {
            // A live runtime or prepared operation holds the clock. Discard
            // expired weak entries; there is no growing table of past replies.
            lanes.retain(|_, lane| lane.strong_count() > 0);
            let lane = Arc::new(Lane {
                id: uuid::Uuid::new_v4(),
                revision: AtomicU64::new(0),
                applied: AtomicU64::new(0),
                published: AtomicU64::new(0),
                live_rpc_revision: AtomicU64::new(0),
                close_target: Mutex::new((0, None)),
                effects: Mutex::new(()),
            });
            lanes.insert(path.clone(), Arc::downgrade(&lane));
            lane
        });
        Self { path, lane }
    }

    pub(crate) fn matches_store(&self, session_id: &str) -> bool {
        self.path == plan_chrome_path(session_id)
    }

    pub(crate) fn prepare(
        &self,
        session_id: &str,
        mutation: PlanChromeMutation,
    ) -> Option<PreparedPlanChrome> {
        if session_id.trim().is_empty() || !self.matches_store(session_id) {
            return None;
        }
        // Rust 1.99 renames fetch_update to try_update; keep the older name so
        // pre-1.99 toolchains still build.
        #[allow(deprecated)]
        let revision = self
            .lane
            .revision
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                value.checked_add(1)
            })
            .ok()?
            + 1;
        {
            let mut target = self.lane.close_target.lock();
            if revision > target.0 {
                let tool_call_id = match &mutation {
                    PlanChromeMutation::Event { tool_call_id, .. } => tool_call_id.clone(),
                    PlanChromeMutation::Decision { plan, .. } => {
                        plan.as_ref().and_then(|plan| plan.tool_call_id.clone())
                    }
                    PlanChromeMutation::Stale { plan } => {
                        plan.as_ref().and_then(|plan| plan.tool_call_id.clone())
                    }
                };
                *target = (revision, tool_call_id.or_else(|| target.1.clone()));
            }
        }
        Some(PreparedPlanChrome {
            clock: self.clone(),
            session_id: session_id.into(),
            revision,
            mutation,
        })
    }

    fn revision_token(&self, revision: u64) -> String {
        format!("{}:{revision}", self.lane.id)
    }

    pub(crate) fn current_ui_revision(&self) -> String {
        self.revision_token(self.lane.revision.load(Ordering::SeqCst))
    }

    pub(crate) fn published_ui_revision(&self) -> String {
        self.revision_token(self.lane.published.load(Ordering::SeqCst))
    }

    fn retain_for_ui(&self) {
        let mut leases = ui_leases().lock();
        leases.insert(self.path.clone(), (self.lane.clone(), Instant::now()));
        while leases.len() > 256 {
            let oldest = leases
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(path, _)| path.clone())
                .unwrap();
            leases.remove(&oldest);
        }
    }

    pub(crate) fn read_for_ui(&self, session_id: &str) -> Option<super::PlanChromeView> {
        let _effects = self.lane.effects.lock();
        if !self.matches_store(session_id) {
            return None;
        }
        let chrome = super::load_plan_chrome_for_ui(session_id).unwrap_or_default();
        let revision = self.lane.revision.load(Ordering::SeqCst);
        let chrome_revision = (self.lane.applied.load(Ordering::SeqCst) == revision).then(|| {
            self.retain_for_ui();
            self.revision_token(revision)
        });
        Some(super::PlanChromeView {
            chrome,
            chrome_revision,
        })
    }

    pub(crate) fn dismiss_from_ui(
        &self,
        session_id: &str,
        expected: Option<&str>,
    ) -> Result<(), String> {
        let _effects = self.lane.effects.lock();
        let revision = self.lane.revision.load(Ordering::SeqCst);
        if !self.matches_store(session_id)
            || expected != Some(self.revision_token(revision).as_str())
        {
            return Err("plan chrome changed; refresh before closing".into());
        }
        if revision != 0 && self.lane.live_rpc_revision.load(Ordering::SeqCst) == revision {
            return Err("a live plan approval must be answered through its pending request".into());
        }
        let next = revision
            .checked_add(1)
            .ok_or("plan chrome revision exhausted")?;
        // A displayed progress event may not have reached disk yet. Preserve
        // the tool captured by that native reservation so residual updates
        // stay closed; UI-supplied chrome content is never the authority.
        let plan = {
            let target = self.lane.close_target.lock();
            (target.0 == revision)
                .then(|| target.1.clone())
                .flatten()
                .map(|tool_call_id| PlanChromeDraft {
                    entries: Value::Null,
                    body: None,
                    rpc_id: None,
                    tool_call_id: Some(tool_call_id),
                })
        };
        // Keep the displayed revision while attempting IO, so a failed write
        // remains retryable. The effect lock excludes another UI close; new
        // runtime reservations still invalidate every checked write step.
        let write = PreparedPlanChrome {
            clock: self.clone(),
            session_id: session_id.into(),
            revision,
            mutation: PlanChromeMutation::Decision {
                decision: "abandoned".into(),
                keep_body: false,
                plan,
            },
        };
        if !write.write()? {
            return Err("plan chrome changed; refresh before closing".into());
        }
        if self
            .lane
            .revision
            .compare_exchange(revision, next, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.lane.applied.store(next, Ordering::SeqCst);
        }
        Ok(())
    }
}

pub(crate) struct PlanChromeDraft {
    pub(crate) entries: Value,
    pub(crate) body: Option<String>,
    pub(crate) rpc_id: Option<u64>,
    pub(crate) tool_call_id: Option<String>,
}

pub(crate) enum PlanChromeMutation {
    Event {
        entries: Value,
        body: Option<String>,
        rpc_id: Option<u64>,
        tool_call_id: Option<String>,
        fresh_gate: bool,
    },
    Decision {
        decision: String,
        keep_body: bool,
        plan: Option<PlanChromeDraft>,
    },
    Stale {
        plan: Option<PlanChromeDraft>,
    },
}

pub(crate) struct PreparedPlanChrome {
    clock: PlanChromeClock,
    session_id: String,
    revision: u64,
    mutation: PlanChromeMutation,
}

impl PreparedPlanChrome {
    pub(crate) fn ui_revision(&self) -> String {
        if matches!(
            &self.mutation,
            PlanChromeMutation::Event {
                rpc_id: Some(_),
                ..
            }
        ) {
            self.clock
                .lane
                .live_rpc_revision
                .fetch_max(self.revision, Ordering::SeqCst);
        }
        self.clock.retain_for_ui();
        self.clock.revision_token(self.revision)
    }
    fn is_current(&self) -> bool {
        self.clock.matches_store(&self.session_id)
            && self.clock.lane.revision.load(Ordering::SeqCst) == self.revision
    }

    pub(crate) fn commit(&self) -> Result<bool, String> {
        self.commit_then(|| {})
    }

    /// Keep publication in the same ordered lane, but drop the disk lock before
    /// emitting IPC. Reservation may advance concurrently; a newer publisher
    /// waits here outside all runtime guards, so it cannot emit before this one.
    pub(crate) fn commit_then(&self, publish: impl FnOnce()) -> Result<bool, String> {
        let _effects = self.clock.lane.effects.lock();
        if !self.is_current() {
            return Ok(false);
        }
        let result = self.write();
        // Disk failure must not hide a still-live human gate from the user.
        // It remains actionable through the runtime snapshot, and is logged.
        if self.is_current() {
            self.clock
                .lane
                .published
                .store(self.revision, Ordering::SeqCst);
            publish();
        }
        result
    }

    fn write(&self) -> Result<bool, String> {
        let check = || {
            if self.is_current() {
                Ok(())
            } else {
                Err("stale prepared plan chrome".into())
            }
        };
        let result = (|| {
            let _lock = crate::store_lock::lock_exclusive_many_checked(
                std::slice::from_ref(&self.clock.path),
                &check,
            )?;
            check()?;
            let mut chrome: PlanChromeStored = std::fs::read_to_string(&self.clock.path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            match &self.mutation {
                PlanChromeMutation::Event {
                    entries,
                    body,
                    rpc_id,
                    tool_call_id,
                    fresh_gate,
                } => {
                    if !super::merge_plan_event(
                        &mut chrome,
                        entries,
                        body,
                        *rpc_id,
                        tool_call_id,
                        *fresh_gate,
                    ) {
                        return Ok(false);
                    }
                }
                PlanChromeMutation::Decision {
                    decision,
                    keep_body,
                    plan,
                } => {
                    // A reply can finalize before its prepared event has hit
                    // disk. Keep the captured runtime body, not an older file.
                    if let Some(plan) = plan {
                        super::merge_plan_event(
                            &mut chrome,
                            &plan.entries,
                            &plan.body,
                            plan.rpc_id,
                            &plan.tool_call_id,
                            true,
                        );
                    }
                    super::merge_plan_decision(&mut chrome, decision, *keep_body);
                }
                PlanChromeMutation::Stale { plan } => {
                    if let Some(plan) = plan {
                        super::merge_plan_event(
                            &mut chrome,
                            &plan.entries,
                            &plan.body,
                            plan.rpc_id,
                            &plan.tool_call_id,
                            false,
                        );
                    }
                    super::merge_gate_stale(&mut chrome);
                }
            }
            chrome.body = cap_body(&chrome.body);
            chrome.entries = cap_entries(chrome.entries);
            let bytes = serde_json::to_vec_pretty(&chrome).map_err(|error| error.to_string())?;
            crate::store_lock::write_bytes_replace_checked(&self.clock.path, &bytes, &check)?;
            Ok(true)
        })();
        if !self.is_current() {
            Ok(false)
        } else {
            if result.is_ok() {
                self.clock
                    .lane
                    .applied
                    .store(self.revision, Ordering::SeqCst);
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicted_idle_ui_identity_can_refresh_and_close_without_a_live_runtime() {
        let _home =
            crate::session_migration::test_home::IsolatedAppHome::acquire("idle-plan-chrome-lease");
        let id = uuid::Uuid::new_v4().to_string();
        let token = super::super::load_plan_chrome_view(&id)
            .unwrap()
            .chrome_revision;
        for _ in 0..256 {
            super::super::load_plan_chrome_view(&uuid::Uuid::new_v4().to_string()).unwrap();
        }
        assert!(super::super::dismiss_plan_chrome(&id, token.as_deref()).is_err());
        let current = super::super::load_plan_chrome_view(&id)
            .unwrap()
            .chrome_revision;
        super::super::dismiss_plan_chrome(&id, current.as_deref()).unwrap();
        assert!(super::super::load_plan_chrome(&id).unwrap().user_closed);
        assert!(ui_leases().lock().len() <= 256);
    }
}
