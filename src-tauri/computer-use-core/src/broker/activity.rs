//! Physical completion is independent of an unknown business outcome.
use super::*;

pub(super) struct PhysicalCompletion {
    activity: Option<Arc<crate::task_consent::PhysicalActivity>>,
}

impl PhysicalCompletion {
    pub(super) fn new(activity: Option<Arc<crate::task_consent::PhysicalActivity>>) -> Self {
        Self { activity }
    }
}

impl Drop for PhysicalCompletion {
    fn drop(&mut self) {
        if let Some(activity) = &self.activity {
            activity.finish();
        }
    }
}

pub(super) fn unknown_work_pending(run: &RunState) -> bool {
    run.last_unknown
        && !run.unknown_idle_confirmed
        && !run
            .activity
            .as_ref()
            .is_some_and(|activity| activity.idle_confirmed())
}

impl ComputerUseBroker {
    /// A remote adapter can return unknown before its original operation ends.
    /// Confirm only the owned resource's idle state, outside the authority lock.
    /// This starts its idle period; it does not clear unknown or replay.
    pub(super) fn refresh_unknown_activity(&self) {
        let candidates: Vec<_> = self
            .inner
            .lock()
            .runs
            .iter()
            .filter_map(|(id, run)| {
                (unknown_work_pending(run)
                    && !run.in_flight.load(Ordering::SeqCst)
                    && run.stop == StopState::Running
                    && !run.paused)
                    .then(|| {
                        run.target
                            .clone()
                            .map(|target| (id.clone(), run.generation, target))
                    })
                    .flatten()
            })
            .collect();
        for (id, generation, target) in candidates {
            let Ok(executor) = self.executor_for_target(&target) else {
                continue;
            };
            if !executor.adapter.is_idle(&id) {
                continue;
            }
            let mut inner = self.inner.lock();
            let current = inner.runs.get_mut(&id).is_some_and(|run| {
                if run.generation != generation
                    || run.stop != StopState::Running
                    || run.paused
                    || !run.last_unknown
                    || run.unknown_idle_confirmed
                    || run.in_flight.load(Ordering::SeqCst)
                    || run.target.as_ref().is_none_or(|current| {
                        current.target_id != target.target_id
                            || current.surface != target.surface
                            || current.executor_generation != target.executor_generation
                    })
                {
                    return false;
                }
                run.unknown_idle_confirmed = true;
                true
            });
            if current {
                note_locked_activity(&mut inner, &id);
            }
        }
    }
}
