//! Read-only name waits on the EXACT retained AX object. Never recapture,
//! replace a reference by name/position, read AXValue, or dispatch input.
use super::*;
use grok_computer_use_core::adapter::ActionScope;
use std::time::{Duration, Instant};

fn parameters(req: &DispatchRequest) -> Result<(&str, Duration), String> {
    let p = req
        .parameters
        .as_object()
        .ok_or("wait parameters required")?;
    if p.keys().any(|k| k != "nameEquals" && k != "timeoutMs") {
        return Err("unsupported wait parameter".into());
    }
    let name = p
        .get("nameEquals")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.trim().is_empty() && s.len() <= 256)
        .ok_or("invalid nameEquals")?;
    let timeout = match p.get("timeoutMs") {
        None => 2000,
        Some(value) => value
            .as_u64()
            .filter(|ms| (1..=10_000).contains(ms))
            .ok_or("timeoutMs must be between 1 and 10000")?,
    };
    Ok((name, Duration::from_millis(timeout)))
}

fn chain(
    tree: &Tree,
    index: usize,
    root: &Element,
    pid: u32,
    budget: &Budget<'_>,
) -> Result<(), String> {
    if !tree.nodes[0].element.same(root) {
        return Err("AX wait snapshot root was replaced".into());
    }
    let mut current = index;
    loop {
        let item = &tree.nodes[current];
        let role = budget.role(item.element.raw())?;
        // Names may change (that is the condition being awaited), but identity,
        // ancestry, role, protection and observed geometry may not.
        if protected(budget, &item.element, &role)?
            || role != item.role
            || budget.flag(item.element.raw(), "AXHidden")? == Some(true)
            || budget.optional_bounds(item.element.raw())? != item.bounds
        {
            return Err("AX wait control identity or presentation changed".into());
        }
        match item.parent {
            Some(parent) => {
                let ancestor = &tree.nodes[parent].element;
                if !budget
                    .related(&item.element, "AXParent", pid)?
                    .same(ancestor)
                    || !budget.related(&item.element, "AXWindow", pid)?.same(root)
                    || !budget
                        .children(ancestor, pid, MAX_NODES)?
                        .0
                        .iter()
                        .any(|n| n.same(&item.element))
                {
                    return Err("AX wait control left its observed ancestry".into());
                }
                current = parent;
            }
            None if current == 0 => return budget.check(),
            _ => return Err("AX wait ancestry does not reach the selected root".into()),
        }
    }
}

pub(in super::super) fn wait(
    adapter: &MacosAdapter,
    req: &DispatchRequest,
) -> Result<AdapterActResult, String> {
    let (expected, timeout) = parameters(req)?;
    let deadline = Instant::now() + timeout;
    if req.scope != ActionScope::Directed {
        return Err("desktop fallback is forbidden".into());
    }
    let ActionTarget::Element { element_ref } = &req.target else {
        return Err("wait requires an exact AX elementRef".into());
    };
    let shared = adapter.snapshots.get(&req.run_id, &req.target_id)?;
    let frame = shared.frame();
    if frame.snapshot_id != req.snapshot_id || frame.revision() != req.geometry_revision {
        return Err("AX wait snapshot/geometry is stale".into());
    }
    let tree = shared.payload().try_lock().ok_or("AX snapshot is busy")?;
    let index = tree
        .nodes
        .iter()
        .position(|n| n.reference == *element_ref && n.wait)
        .ok_or("unknown or ineligible AX wait reference")?;
    let instance = WindowInstance::parse(&req.target_id)?;
    let authority = || {
        req.cancellation.check()?;
        if !adapter.input_available() {
            return Err("macOS permission revoked during wait".into());
        }
        adapter
            .snapshots
            .require_current(&req.run_id, &req.target_id, &shared)
    };
    loop {
        authority()?;
        if Instant::now() >= deadline {
            return Err("wait timed out".into());
        }
        let budget = Budget::with_deadline(&req.cancellation, deadline);
        let result =
            adapter
                .windows
                .with_window_budget(instance, frame.bounds, &budget, |root, budget| {
                    chain(&tree, index, root, instance.window.pid, budget)?;
                    // Recheck protection immediately before reading a public name.
                    let node = &tree.nodes[index];
                    let role = budget.role(node.element.raw())?;
                    if protected(budget, &node.element, &role)? {
                        return Err("AX wait control became protected".into());
                    }
                    let matched = name(budget, &node.element)? == expected;
                    // A getter can pump remote work. Reject replacement/retirement or
                    // protection changes during that read instead of accepting it.
                    chain(&tree, index, root, instance.window.pid, budget)?;
                    if window_bounds(instance.window)? != frame.bounds
                        || capture::display_revision()? != frame.display_revision
                    {
                        return Err("macOS geometry changed during wait".into());
                    }
                    identity::validate(instance.window)?;
                    budget.check()?;
                    authority()?;
                    Ok(matched)
                });
        authority()?;
        if Instant::now() >= deadline {
            return Err("wait timed out".into());
        }
        let matched = result?;
        if matched {
            return Ok(AdapterActResult {
                applied: false,
                outcome: None,
                postcondition_ok: true,
                verifiable: true,
                detail: "wait matched the original AX control; no input dispatched".into(),
            });
        }
        // Each poll releases WindowBindings. Stop never needs its native lock;
        // only this read-only operation's guard drop releases its occupancy.
        std::thread::sleep(
            Duration::from_millis(25).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}
