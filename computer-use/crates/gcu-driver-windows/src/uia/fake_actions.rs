//! Fake UIA action scripting, identity changes and exact dispatch traces.
use super::{
    cache::Properties,
    patterns::*,
    source::{FakeUia, NodeId},
};
use gcu_core::driver::{DriverError, Result};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

#[derive(Default)]
pub(super) struct FakeActions {
    pub(super) hwnd: u64,
    pub(super) observed: BTreeMap<NodeId, (Properties, Vec<i32>, u64)>,
    runtime_ids: BTreeMap<NodeId, Vec<i32>>,
    replies: BTreeMap<Pattern, VecDeque<Result<PatternResult>>>,
    calls: Vec<PatternAction>,
    prepare_delay: Duration,
    dispatch_delay: Duration,
}
impl FakeActions {
    pub(super) fn new_generation(&mut self, generation: u64) {
        self.observed.clear();
        self.runtime_ids = std::mem::take(&mut self.runtime_ids)
            .into_iter()
            .map(|(id, runtime)| {
                (
                    NodeId {
                        generation,
                        slot: id.slot,
                    },
                    runtime,
                )
            })
            .collect();
    }
    pub(super) fn runtime(&self, node: NodeId) -> Vec<i32> {
        self.runtime_ids
            .get(&node)
            .cloned()
            .unwrap_or_else(|| vec![node.slot as i32])
    }
}
impl FakeUia {
    /// Script the next result of one native pattern; unspecified calls succeed.
    pub fn reply(&self, pattern: Pattern, result: Result<PatternResult>) {
        if let Ok(mut s) = self.0.lock() {
            s.actions
                .replies
                .entry(pattern)
                .or_default()
                .push_back(result);
        }
    }
    /// Commands actually dispatched, in order. No focus/pointer command exists.
    pub fn action_calls(&self) -> Vec<PatternAction> {
        self.0
            .lock()
            .map(|s| s.actions.calls.clone())
            .unwrap_or_default()
    }
    /// Delay preparation and/or the in-flight provider response separately.
    pub fn set_action_delay(&self, preparation: Duration, dispatched: Duration) {
        if let Ok(mut s) = self.0.lock() {
            s.actions.prepare_delay = preparation;
            s.actions.dispatch_delay = dispatched;
        }
    }
    /// Change live metadata after observation, without changing the stored identity.
    pub fn update_properties(&self, slot: u64, update: impl FnOnce(&mut Properties)) {
        if let Ok(mut s) = self.0.lock() {
            let id = s.node(slot);
            if let Some((p, _)) = s.nodes.get_mut(&id) {
                update(p);
            }
        }
    }
    /// Simulate runtime identity reuse even when name/type/frame remain equal.
    pub fn set_runtime_id(&self, slot: u64, runtime: Vec<i32>) {
        if let Ok(mut s) = self.0.lock() {
            let id = s.node(slot);
            s.actions.runtime_ids.insert(id, runtime);
        }
    }
    /// Report the element's owning process. `None` leaves the portable probe unset.
    /// A mismatch with the admitted app pid refuses the action before dispatch.
    pub fn set_owner_pid(&self, pid: Option<u32>) {
        if let Ok(mut state) = self.0.lock() {
            state.owner_pid = pid;
        }
    }
    /// Change exact HWND ancestry without resolving any other node.
    pub fn set_action_hwnd(&self, hwnd: u64) {
        if let Ok(mut s) = self.0.lock() {
            s.actions.hwnd = hwnd;
        }
    }
    pub(super) fn validate_action(&self, node: NodeId, hwnd: u64) -> Result<Properties> {
        let s = self.0.lock().map_err(|_| DriverError::Crashed {
            maybe_executed: false,
        })?;
        let (old, runtime, observed_hwnd) = s
            .actions
            .observed
            .get(&node)
            .ok_or(DriverError::StaleElement)?;
        let fresh = &s.nodes.get(&node).ok_or(DriverError::StaleElement)?.0;
        if *observed_hwnd != hwnd
            || s.actions.hwnd != hwnd
            || *runtime != s.actions.runtime(node)
            || !super::actions::same_identity(old, fresh)
        {
            return Err(DriverError::StaleElement);
        }
        // Match native ancestor checks; same-name replacement controls are never
        // searched as possible action targets.
        let mut ancestor = node;
        for _ in 0..super::tree::DEPTH_CAP {
            let Some((parent, (properties, _))) = s
                .nodes
                .iter()
                .find(|(_, (_, children))| children.contains(&ancestor))
            else {
                break;
            };
            if super::sensitive::is_execution_surface(properties) {
                return Err(DriverError::ElementNotActionable);
            }
            ancestor = *parent;
        }
        Ok(fresh.clone())
    }
    pub(super) async fn perform_action(
        &self,
        node: NodeId,
        hwnd: u64,
        action: PatternAction,
        context: ActionContext,
    ) -> Result<PatternResult> {
        let (prepare, dispatched) = {
            let s = self.0.lock().map_err(|_| DriverError::Crashed {
                maybe_executed: false,
            })?;
            (s.actions.prepare_delay, s.actions.dispatch_delay)
        };
        if !prepare.is_zero() {
            tokio::time::sleep(prepare).await;
        }
        context.check()?;
        let props = self.validate_action(node, hwnd)?;
        if !action.pattern().available(&props.patterns) {
            return Ok(PatternResult::Unsupported);
        }
        if props.password
            || props.partial
            || !props.enabled
            || super::sensitive::is_execution_surface(&props)
        {
            return Ok(PatternResult::Unavailable);
        }
        if matches!(
            action,
            PatternAction::SetValue(_)
                | PatternAction::SetRangeValue(_)
                | PatternAction::AdjustRange(_)
        ) && props.read_only != Some(false)
        {
            return Ok(PatternResult::Unavailable);
        }
        let previous = context.begin()?;
        let result = {
            let mut s = self.0.lock().map_err(|_| DriverError::Crashed {
                maybe_executed: true,
            })?;
            let result = s
                .actions
                .replies
                .get_mut(&action.pattern())
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(PatternResult::Performed));
            s.actions.calls.push(action);
            result
        };
        if !dispatched.is_zero() {
            tokio::time::sleep(dispatched).await;
        }
        if matches!(
            result,
            Ok(PatternResult::Unsupported | PatternResult::Unavailable)
        ) || result.as_ref().is_err_and(|e| !e.maybe_executed())
        {
            context.restore(previous);
        }
        result.map_err(|e| context.error(e))
    }
}
