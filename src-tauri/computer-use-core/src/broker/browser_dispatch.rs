//! Single Broker-owned admit path for every browser write.

use super::*;
use crate::browser::action_ledger;
use crate::browser::{
    ManagedLocator, ManagedPage, ManagedTabAction, TabInfo, WorkerCompletion, WorkerError,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(test)]
mod transaction_tests {
    use super::*;

    #[test]
    fn publishing_a_browser_outcome_does_not_release_the_outer_transaction() {
        let adapter = Arc::new(crate::fake::FakeAdapter::new());
        let broker =
            ComputerUseBroker::new(adapter.clone(), super::super::gates::enabled_opts(100));
        broker.open_run("session", "run").unwrap();
        let generation = broker
            .authorize_target("run", &adapter.fixture_id())
            .unwrap();
        let permit = {
            let state = broker.inner.lock();
            let run = &state.runs["run"];
            run.in_flight.store(true, Ordering::SeqCst);
            InFlightClear(run.in_flight.clone())
        };
        broker.finish_browser(
            "run",
            "id",
            generation,
            OutcomeKind::Unknown,
            true,
            None,
            None,
        );
        assert!(matches!(
            broker.observe("run"),
            Err(BrokerError::LeaseHeld { .. })
        ));
        assert_eq!(adapter.observe_calls(), 0);
        assert!(broker.inner.lock().runs["run"].last_unknown);
        drop(permit);
        broker.observe("run").unwrap();
        assert!(!broker.inner.lock().runs["run"].last_unknown);
    }
}

#[derive(Clone)]
pub(super) enum BrowserReplay {
    Tab(TabInfo),
    Download(std::path::PathBuf),
    Page(ManagedPage),
    Upload,
}

pub enum BrowserWriteResult {
    Tab(TabInfo),
    Download(std::path::PathBuf),
    Page(ManagedPage),
    Uploaded,
}

pub enum BrowserOp<'a> {
    Navigate {
        url: &'a str,
    },
    Download {
        filename: &'a str,
        model_path: Option<&'a str>,
        snapshot_id: &'a str,
        element_ref: &'a str,
    },
    Act {
        kind: &'a str,
        snapshot_id: &'a str,
        locator: ManagedLocator,
        params: serde_json::Value,
    },
    Upload {
        snapshot_id: &'a str,
        element_ref: &'a str,
        source: &'a std::path::Path,
    },
    NewTab {
        profile: &'a str,
    },
    Popup {
        profile: &'a str,
        element_ref: &'a str,
    },
}

pub struct BrowserWrite<'a> {
    pub session_id: Option<&'a str>,
    pub run_id: &'a str,
    pub tab_id: &'a str,
    pub action_id: &'a str,
    pub page_generation: u64,
    pub op: BrowserOp<'a>,
}

enum Admit {
    Replay(BrowserReplay),
    Execute {
        page_generation: u64,
        inflight: Arc<AtomicBool>,
        run_generation: u64,
        managed_request: crate::browser::ManagedRequest,
    },
}

impl ComputerUseBroker {
    pub fn dispatch_browser(
        &self,
        req: BrowserWrite<'_>,
    ) -> Result<BrowserWriteResult, BrokerError> {
        match self.admit_browser(&req)? {
            Admit::Replay(replay) => Ok(replay_to_result(replay)),
            Admit::Execute {
                page_generation,
                inflight,
                run_generation,
                managed_request,
            } => {
                let _guard = InFlightClear(inflight);
                self.trace_browser(&req);
                let result =
                    self.execute_browser(&req, page_generation, run_generation, &managed_request);
                let result = if self.browser_commit_ok(req.run_id, run_generation) {
                    result
                } else {
                    Err(BrokerError::BrowserWorker(WorkerError::unknown(
                        409,
                        "request_revoked",
                        "request completed after authority was revoked; do not replay",
                    )))
                };
                match result {
                    Ok(result) => {
                        self.finish_browser(
                            req.run_id,
                            req.action_id,
                            run_generation,
                            OutcomeKind::Applied,
                            false,
                            Some(result_to_replay(&result)),
                            Some("applied"),
                        );
                        Ok(result)
                    }
                    Err(error) => {
                        let (kind, unknown) = classify_browser_error(&error);
                        self.finish_browser(
                            req.run_id,
                            req.action_id,
                            run_generation,
                            kind,
                            unknown,
                            None,
                            Some(&error.to_string()),
                        );
                        Err(error)
                    }
                }
            }
        }
    }

    fn admit_browser(&self, req: &BrowserWrite<'_>) -> Result<Admit, BrokerError> {
        crate::browser::validate_browser_action_id(req.action_id)?;
        self.preflight_browser_run(req)?;
        let replay_existing = {
            let g = self.inner.lock();
            g.runs
                .get(req.run_id)
                .is_some_and(|run| run.browser_replay.contains_key(req.action_id))
        };

        let identity = if matches!(req.op, BrowserOp::NewTab { .. }) {
            None
        } else {
            Some(self.tabs.tab_write_identity(req.run_id, req.tab_id)?)
        };
        if let Some(identity) = identity.as_ref() {
            if !replay_existing {
                if req.page_generation != 0 && req.page_generation != identity.page_generation {
                    return Err(BrokerError::IdentityMismatch("pageGeneration"));
                }
                match snapshot_requirement(&req.op) {
                    SnapshotNeed::None => {}
                    SnapshotNeed::Current {
                        snapshot_id,
                        element_ref,
                    } => {
                        match identity.snapshot_id.as_deref() {
                            Some(current) if current == snapshot_id => {}
                            _ => return Err(BrokerError::IdentityMismatch("snapshotId")),
                        }
                        if !element_ref.is_empty() && !identity.refs.contains(element_ref) {
                            return Err(BrokerError::IdentityMismatch("elementRef"));
                        }
                    }
                    SnapshotNeed::Observation { element_ref } => {
                        let Some(current) = identity.snapshot_id.as_deref() else {
                            return Err(BrokerError::IdentityMismatch("snapshotId"));
                        };
                        let _ = current;
                        if !element_ref.is_empty() && !identity.refs.contains(element_ref) {
                            return Err(BrokerError::IdentityMismatch("elementRef"));
                        }
                    }
                }
            }
        }

        let page_id = identity
            .as_ref()
            .map(|row| row.page_id.as_str())
            .unwrap_or("");
        let page_generation = if req.page_generation != 0 {
            req.page_generation
        } else {
            identity
                .as_ref()
                .map(|row| row.page_generation)
                .unwrap_or(0)
        };
        let snapshot_hint = identity
            .as_ref()
            .and_then(|row| row.snapshot_id.as_deref())
            .unwrap_or("");
        let fingerprint = action_ledger::fingerprint(&canonical_op(
            &req.op,
            page_id,
            page_generation,
            snapshot_hint,
        ));

        let mut g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let budget = self.action_budget;
        let run = g.runs.get_mut(req.run_id).ok_or(BrokerError::RunNotFound)?;
        if let Some(session) = req.session_id {
            if run.app_session_id != session {
                return Err(BrokerError::IdentityMismatch("appSessionId"));
            }
        }
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        if run.paused {
            return Err(BrokerError::Schema("run is paused".into()));
        }
        if run.last_unknown {
            return Err(BrokerError::Schema(
                "unknown result must observe before the next action".into(),
            ));
        }
        if let Some(existing) = run.fingerprints.get(req.action_id) {
            if existing != &fingerprint {
                return Err(BrokerError::Schema("actionId fingerprint conflict".into()));
            }
            if let Some(replay) = run.browser_replay.get(req.action_id) {
                return Ok(Admit::Replay(replay.clone()));
            }
            if run.in_flight.load(Ordering::SeqCst) {
                return Err(BrokerError::LeaseHeld {
                    run_id: req.run_id.to_string(),
                });
            }
        } else if run.seen.contains_key(req.action_id) {
            return Err(BrokerError::DuplicateAction);
        } else if run.seen.len() >= budget {
            return Err(BrokerError::Schema(
                "action budget exhausted; start a new run".into(),
            ));
        }
        if run.in_flight.load(Ordering::SeqCst) {
            return Err(BrokerError::LeaseHeld {
                run_id: req.run_id.to_string(),
            });
        }
        if matches!(&req.op, BrowserOp::Act { kind, .. } if kind.eq_ignore_ascii_case("wait")) {
            if run.observe_count >= self.observe_budget {
                return Err(BrokerError::Schema(
                    "observe budget exhausted; start a new run".into(),
                ));
            }
            run.observe_count = run.observe_count.saturating_add(1);
        }
        let managed_request = self
            .tabs
            .admit_managed_request(req.run_id, run.cancellation.clone())?;
        run.in_flight.store(true, Ordering::SeqCst);
        run.fingerprints
            .insert(req.action_id.to_string(), fingerprint);
        run.seen.insert(
            req.action_id.to_string(),
            ActionOutcome {
                action_id: req.action_id.to_string(),
                run_id: req.run_id.to_string(),
                kind: OutcomeKind::Unknown,
                executed: false,
                reason: Some("action pending".into()),
                generation: run.generation,
            },
        );
        Ok(Admit::Execute {
            page_generation,
            inflight: run.in_flight.clone(),
            run_generation: run.generation,
            managed_request,
        })
    }

    fn preflight_browser_run(&self, req: &BrowserWrite<'_>) -> Result<(), BrokerError> {
        let g = self.inner.lock();
        if !g.feature_enabled {
            return Err(BrokerError::FeatureDisabled);
        }
        let run = g.runs.get(req.run_id).ok_or(BrokerError::RunNotFound)?;
        if let Some(session) = req.session_id {
            if run.app_session_id != session {
                return Err(BrokerError::IdentityMismatch("appSessionId"));
            }
        }
        if run.stop != StopState::Running {
            return Err(BrokerError::StopRequested);
        }
        if run.paused {
            return Err(BrokerError::Schema("run is paused".into()));
        }
        Ok(())
    }

    fn browser_commit_ok(&self, run_id: &str, run_generation: u64) -> bool {
        let g = self.inner.lock();
        g.runs.get(run_id).is_some_and(|run| {
            run.generation == run_generation && run.stop == StopState::Running && !run.paused
        })
    }

    fn execute_browser(
        &self,
        req: &BrowserWrite<'_>,
        page_generation: u64,
        run_generation: u64,
        managed_request: &crate::browser::ManagedRequest,
    ) -> Result<BrowserWriteResult, BrokerError> {
        let tabs = self.tabs.for_managed_request(managed_request)?;
        match &req.op {
            BrowserOp::Navigate { url } => {
                crate::browser::is_allowed_navigate_url(url)?;
                let after = tabs.navigate_with_action_commit(
                    req.run_id,
                    req.tab_id,
                    page_generation,
                    req.action_id,
                    url,
                    || self.browser_commit_ok(req.run_id, run_generation),
                )?;
                if after.tab_id != req.tab_id || after.run_id != req.run_id {
                    return Err(BrokerError::IdentityMismatch("tabId"));
                }
                Ok(BrowserWriteResult::Tab(after))
            }
            BrowserOp::Download {
                filename,
                model_path,
                snapshot_id,
                element_ref,
            } => {
                crate::staging::reject_model_path(*model_path)?;
                if snapshot_id.trim().is_empty() {
                    return Err(BrokerError::Schema("snapshotId required".into()));
                }
                if element_ref.trim().is_empty() {
                    return Err(BrokerError::Schema("elementRef required".into()));
                }
                let name = crate::staging::sanitize_filename(filename)?;
                let path = tabs.stage_download_with_action(
                    req.run_id,
                    req.tab_id,
                    page_generation,
                    req.action_id,
                    snapshot_id,
                    Some(element_ref),
                    &name,
                )?;
                Ok(BrowserWriteResult::Download(path))
            }
            BrowserOp::Act {
                kind,
                snapshot_id,
                locator,
                params,
            } => {
                let page = tabs.act_managed_tab(
                    req.run_id,
                    req.tab_id,
                    ManagedTabAction {
                        page_generation,
                        snapshot_id,
                        action_id: req.action_id,
                        kind,
                        locator: locator.clone(),
                        params: params.clone(),
                    },
                )?;
                Ok(BrowserWriteResult::Page(page))
            }
            BrowserOp::Upload {
                snapshot_id: _,
                element_ref,
                source,
            } => {
                tabs.upload_managed(req.run_id, req.tab_id, req.action_id, element_ref, source)?;
                Ok(BrowserWriteResult::Uploaded)
            }
            BrowserOp::NewTab { profile } => {
                let session = req
                    .session_id
                    .ok_or(BrokerError::IdentityMismatch("appSessionId"))?;
                let info = tabs.open_managed_profile(session, req.run_id, profile)?;
                Ok(BrowserWriteResult::Tab(info))
            }
            BrowserOp::Popup {
                profile,
                element_ref,
            } => {
                let page =
                    tabs.open_managed_popup(req.run_id, profile, req.action_id, element_ref)?;
                Ok(BrowserWriteResult::Page(page))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_browser(
        &self,
        run_id: &str,
        action_id: &str,
        run_generation: u64,
        kind: OutcomeKind,
        unknown: bool,
        replay: Option<BrowserReplay>,
        reason: Option<&str>,
    ) {
        let mut g = self.inner.lock();
        let late = g
            .runs
            .get(run_id)
            .is_some_and(|run| run.generation != run_generation);
        if late {
            g.dropped_late += 1;
        }
        if let Some(run) = g.runs.get_mut(run_id) {
            if late {
                run.seen.insert(
                    action_id.to_string(),
                    ActionOutcome {
                        action_id: action_id.to_string(),
                        run_id: run_id.to_string(),
                        kind: OutcomeKind::Unknown,
                        executed: false,
                        reason: Some("result after generation change; do not replay".into()),
                        generation: run_generation,
                    },
                );
                return;
            }
            run.seen.insert(
                action_id.to_string(),
                ActionOutcome {
                    action_id: action_id.to_string(),
                    run_id: run_id.to_string(),
                    kind,
                    executed: matches!(kind, OutcomeKind::Applied | OutcomeKind::Verified),
                    reason: reason.map(str::to_string),
                    generation: run_generation,
                },
            );
            if unknown {
                run.last_unknown = true;
            }
            if let Some(replay) = replay {
                run.browser_replay.insert(action_id.to_string(), replay);
            }
        }
    }

    fn trace_browser(&self, req: &BrowserWrite<'_>) {
        let mut g = self.inner.lock();
        push_trace(
            &mut g,
            "browser",
            req.run_id,
            &trace_detail(&req.op, req.action_id),
            TraceAudience::Model,
        );
    }
}

enum SnapshotNeed<'a> {
    None,
    Current {
        snapshot_id: &'a str,
        element_ref: &'a str,
    },
    Observation {
        element_ref: &'a str,
    },
}

fn snapshot_requirement<'a>(op: &'a BrowserOp<'a>) -> SnapshotNeed<'a> {
    match op {
        BrowserOp::Download {
            snapshot_id,
            element_ref,
            ..
        }
        | BrowserOp::Upload {
            snapshot_id,
            element_ref,
            ..
        } => SnapshotNeed::Current {
            snapshot_id,
            element_ref,
        },
        BrowserOp::Act {
            snapshot_id,
            locator,
            ..
        } => SnapshotNeed::Current {
            snapshot_id,
            element_ref: locator.element_ref.as_deref().unwrap_or(""),
        },
        BrowserOp::Popup { element_ref, .. } => SnapshotNeed::Observation { element_ref },
        BrowserOp::Navigate { .. } | BrowserOp::NewTab { .. } => SnapshotNeed::None,
    }
}

fn canonical_op(
    op: &BrowserOp<'_>,
    page_id: &str,
    page_generation: u64,
    snapshot_hint: &str,
) -> String {
    match op {
        BrowserOp::Navigate { url } => action_ledger::canonical_semantics(
            "navigate",
            page_id,
            page_generation,
            "",
            None,
            &serde_json::json!({ "url": url }),
        ),
        BrowserOp::Download {
            filename,
            snapshot_id,
            element_ref,
            ..
        } => action_ledger::canonical_semantics(
            "download",
            page_id,
            page_generation,
            snapshot_id,
            Some(*element_ref),
            &serde_json::json!({ "filename": filename }),
        ),
        BrowserOp::Act {
            kind,
            snapshot_id,
            locator,
            params,
        } => action_ledger::canonical_semantics(
            kind,
            page_id,
            page_generation,
            snapshot_id,
            locator.element_ref.as_deref(),
            params,
        ),
        BrowserOp::Upload {
            snapshot_id,
            element_ref,
            source,
        } => {
            let name = source
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("upload");
            action_ledger::canonical_semantics(
                "upload",
                page_id,
                page_generation,
                snapshot_id,
                Some(*element_ref),
                &serde_json::json!({ "filename": name }),
            )
        }
        BrowserOp::NewTab { profile } => action_ledger::canonical_semantics(
            "new_tab",
            "",
            0,
            "",
            None,
            &serde_json::json!({ "profile": profile }),
        ),
        BrowserOp::Popup { element_ref, .. } => action_ledger::canonical_semantics(
            "popup",
            page_id,
            page_generation,
            snapshot_hint,
            Some(*element_ref),
            &serde_json::json!({}),
        ),
    }
}

fn redacted_url(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_string()
}

fn trace_detail(op: &BrowserOp<'_>, action_id: &str) -> String {
    match op {
        BrowserOp::Navigate { url } => {
            format!("navigate {action_id} {}", redacted_url(url))
        }
        BrowserOp::Download { filename, .. } => {
            format!("download {action_id} {filename}")
        }
        BrowserOp::Act { kind, .. } => format!("act {kind} {action_id}"),
        BrowserOp::Upload { .. } => format!("upload {action_id}"),
        BrowserOp::NewTab { profile } => format!("new_tab {action_id} {profile}"),
        BrowserOp::Popup { .. } => format!("popup {action_id}"),
    }
}

fn replay_to_result(replay: BrowserReplay) -> BrowserWriteResult {
    match replay {
        BrowserReplay::Tab(info) => BrowserWriteResult::Tab(info),
        BrowserReplay::Download(path) => BrowserWriteResult::Download(path),
        BrowserReplay::Page(page) => BrowserWriteResult::Page(page),
        BrowserReplay::Upload => BrowserWriteResult::Uploaded,
    }
}

fn result_to_replay(result: &BrowserWriteResult) -> BrowserReplay {
    match result {
        BrowserWriteResult::Tab(info) => BrowserReplay::Tab(info.clone()),
        BrowserWriteResult::Download(path) => BrowserReplay::Download(path.clone()),
        BrowserWriteResult::Page(page) => BrowserReplay::Page(page.clone()),
        BrowserWriteResult::Uploaded => BrowserReplay::Upload,
    }
}

fn classify_browser_error(error: &BrokerError) -> (OutcomeKind, bool) {
    match error {
        BrokerError::BrowserWorker(WorkerError {
            completion: WorkerCompletion::Unknown,
            ..
        })
        | BrokerError::Timeout => (OutcomeKind::Unknown, true),
        BrokerError::Adapter(text) if text.contains("unknown") => (OutcomeKind::Unknown, true),
        _ => (OutcomeKind::Rejected, false),
    }
}
