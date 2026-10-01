//! Read-only wait on the original unique-bus-owner/object path. A fresh tree,
//! matching label or same-position replacement may never inherit this ref.
use super::*;

pub(super) fn eligible(bits: &[u32], rect: Option<(i32, i32, i32, i32)>, g: Geometry) -> bool {
    visible(bits)
        && rect.is_some_and(|(x, y, w, h)| {
            w > 0
                && h > 0
                && x >= i32::from(g.x)
                && y >= i32::from(g.y)
                && i64::from(x) + i64::from(w) <= i64::from(g.x) + i64::from(g.width)
                && i64::from(y) + i64::from(h) <= i64::from(g.y) + i64::from(g.height)
        })
}

fn parameters(value: &serde_json::Value) -> Result<(&str, Duration), String> {
    let p = value.as_object().ok_or("wait parameters required")?;
    if p.keys().any(|k| k != "nameEquals" && k != "timeoutMs") {
        return Err("unsupported wait parameter".into());
    }
    let expected = p
        .get("nameEquals")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty() && s.len() <= 256 && !s.contains('\0'))
        .ok_or("invalid nameEquals")?;
    let timeout = match p.get("timeoutMs") {
        None => 2000,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=10_000).contains(n))
            .ok_or("timeoutMs must be between 1 and 10000")?,
    };
    Ok((expected, Duration::from_millis(timeout)))
}

struct Budget<'a, F> {
    req: &'a DispatchRequest,
    deadline: Instant,
    native: F,
}
impl<F: FnMut() -> Result<(), String>> Budget<'_, F> {
    fn check(&mut self) -> Result<(), String> {
        self.req.cancellation.check()?;
        if Instant::now() >= self.deadline {
            return Err("wait timed out".into());
        }
        (self.native)()?;
        if Instant::now() >= self.deadline {
            return Err("wait timed out".into());
        }
        Ok(())
    }
    fn read<T>(&mut self, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        self.check()?;
        let result = f();
        self.check()?;
        result
    }
}

fn chain<F: FnMut() -> Result<(), String>>(
    snapshot: &Snapshot,
    node: &Node,
    connection: &Connection,
    budget: &mut Budget<'_, F>,
) -> Result<(), String> {
    let mut current = node;
    for parent in node.ancestry.iter().map(Some).chain(std::iter::once(None)) {
        let role: u32 = budget.read(|| {
            proxy(connection, &current.object, ACCESSIBLE)?
                .call("GetRole", &())
                .map_err(err)
        })?;
        let bits = budget.read(|| states(connection, &current.object))?;
        if role == 40 || role != current.role || !visible(&bits) {
            return Err(format!("AT-SPI wait identity, visibility or protection changed: role {} -> {role}, states {bits:?}", current.role));
        }
        if let Some(bounds) = current.bounds {
            if budget.read(|| extents(connection, &current.object))? != bounds {
                return Err("AT-SPI wait observed bounds changed".into());
            }
        }
        match parent {
            Some(parent) => {
                let actual: Object = budget.read(|| {
                    proxy(connection, &current.object, ACCESSIBLE)?
                        .get_property("Parent")
                        .map_err(err)
                })?;
                if &actual != parent
                    || !budget
                        .read(|| children(connection, parent))?
                        .contains(&current.object)
                {
                    return Err("AT-SPI wait left its original ancestry".into());
                }
                current = snapshot
                    .nodes
                    .values()
                    .find(|n| &n.object == parent)
                    .ok_or("AT-SPI observed ancestor is unavailable")?;
            }
            None if current.object != snapshot.root => {
                return Err("AT-SPI wait escaped native root".into())
            }
            None => (),
        }
    }
    Ok(())
}

pub(super) fn run(
    accessibility: &Accessibility,
    req: &DispatchRequest,
    validate_native: impl FnMut() -> Result<(), String>,
) -> Result<AdapterActResult, String> {
    let (expected, timeout) = parameters(&req.parameters)?;
    let mut budget = Budget {
        req,
        deadline: Instant::now() + timeout,
        native: validate_native,
    };
    let ActionTarget::Element { element_ref } = &req.target else {
        return Err("AT-SPI wait requires an exact observed reference".into());
    };
    let snapshot = accessibility
        .snapshots
        .iter()
        .find(|s| {
            s.run == req.run_id
                && s.target == req.target_id
                && s.id == req.snapshot_id
                && s.geometry.revision() == req.geometry_revision
        })
        .ok_or("AT-SPI wait snapshot is stale")?;
    let node = snapshot
        .nodes
        .get(element_ref)
        .filter(|n| n.actions.iter().any(|a| a == "wait"))
        .ok_or("AT-SPI wait reference is unknown or ineligible")?;
    let connection = accessibility
        .connection
        .as_ref()
        .ok_or("AT-SPI disconnected")?;
    loop {
        if budget.read(|| process(connection, &snapshot.root.0))? != snapshot.pid {
            return Err("AT-SPI wait window binding changed".into());
        }
        // Keep the budget around each remote query, not an aggregate helper
        // that could issue several one-second D-Bus calls past the deadline.
        let title: String = budget.read(|| {
            proxy(connection, &snapshot.root, ACCESSIBLE)?
                .get_property("Name")
                .map_err(err)
        })?;
        if title != snapshot.title {
            return Err("AT-SPI wait window title changed".into());
        }
        chain(snapshot, node, connection, &mut budget)?;
        let role: u32 = budget.read(|| {
            proxy(connection, &node.object, ACCESSIBLE)?
                .call("GetRole", &())
                .map_err(err)
        })?;
        if role == 40 || role != node.role {
            return Err("AT-SPI wait control became protected or changed role".into());
        }
        let name: String = budget.read(|| {
            proxy(connection, &node.object, ACCESSIBLE)?
                .get_property("Name")
                .map_err(err)
        })?;
        if name.len() > MAX_TEXT {
            return Err("AT-SPI wait name exceeds read limit".into());
        }
        // A remote getter can pump toolkit work. Revalidate before accepting a
        // result, including bounds/ancestry/protection and local retirement.
        chain(snapshot, node, connection, &mut budget)?;
        budget.check()?;
        if name == expected {
            return Ok(AdapterActResult {
                applied: false,
                outcome: None,
                postcondition_ok: true,
                verifiable: true,
                detail: "wait matched the original AT-SPI control; no input dispatched".into(),
            });
        }
        std::thread::sleep(
            Duration::from_millis(25)
                .min(budget.deadline.saturating_duration_since(Instant::now())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn wait_parameters_are_bounded_and_strict() {
        assert_eq!(
            parameters(&json!({"nameEquals":"中文🙂"})).unwrap().1,
            Duration::from_secs(2)
        );
        assert!(parameters(&json!({"nameEquals":"ok","timeoutMs":1})).is_ok());
        assert!(parameters(&json!({"nameEquals":"ok","timeoutMs":10000})).is_ok());
        for p in [
            json!({}),
            json!(null),
            json!({"nameEquals":" "}),
            json!({"nameEquals":"a\u{0}"}),
            json!({"nameEquals":"x".repeat(257)}),
            json!({"nameEquals":"ok","timeoutMs":0}),
            json!({"nameEquals":"ok","timeoutMs":10001}),
            json!({"nameEquals":"ok","timeoutMs":1.1}),
            json!({"nameEquals":"ok","timeoutMs":"1"}),
            json!({"nameEquals":"ok","extra":true}),
        ] {
            assert!(parameters(&p).is_err(), "{p}");
        }
    }
}
