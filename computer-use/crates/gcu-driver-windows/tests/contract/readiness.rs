//! Observation retries only: external fixture JSON AND a complete UIA tree.
#![cfg_attr(not(windows), allow(dead_code))]
use super::harness::{attempt_bounded, progress, Clock};
use gcu_core::model::{RawNode, RawTree, States};
use serde_json::Value;
use std::time::Duration;

pub fn summary(tree: &RawTree) -> String {
    fn visit(node: &RawNode, depth: usize, rows: &mut Vec<String>) {
        if rows.len() >= 64 {
            return;
        }
        let label = |s: &Option<String>| {
            if node.states.contains(States::PASSWORD) {
                "<protected>".into()
            } else {
                s.as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(96)
                    .collect::<String>()
            }
        };
        rows.push(format!(
            "depth={depth} role={:?} name={:?} id={:?}",
            node.role,
            label(&node.name),
            label(&node.identifier)
        ));
        for child in &node.children {
            visit(child, depth + 1, rows);
        }
    }
    let mut rows = Vec::new();
    if let Some(root) = &tree.root {
        visit(root, 0, &mut rows);
    }
    format!("incomplete={:?}; first_nodes={rows:?}", tree.incomplete)
}

pub async fn wait<C: Clock>(
    clock: &mut C,
    budget: Duration,
    label: &str,
    mut oracle: impl FnMut() -> Result<Value, String>,
    mut read: impl AsyncFnMut() -> Result<RawTree, String>,
    accept: impl Fn(&RawTree) -> bool,
) -> Result<RawTree, String> {
    let start = clock.now();
    let mut attempt = 0;
    loop {
        attempt += 1;
        let step = clock.now();
        let before = oracle();
        let remaining = budget.saturating_sub(step.saturating_sub(start));
        let tree = attempt_bounded(clock, remaining, label, read()).await;
        let after = oracle();
        let detail = tree.as_ref().map(summary).unwrap_or_else(|e| e.clone());
        let last = format!("attempt={attempt}, tree_ms={}, elapsed_ms={}, before_oracle={before:?}, last_oracle={after:?}; {detail}",
            clock.now().saturating_sub(step).as_millis(), clock.now().saturating_sub(start).as_millis());
        progress(format!("UIA READINESS: {label}; {last}"));
        let ready =
            |v: &Result<Value, String>| v.as_ref().is_ok_and(|v| v["counter"].as_u64().is_some());
        if ready(&before)
            && ready(&after)
            && tree
                .as_ref()
                .is_ok_and(|t| t.incomplete.is_none() && accept(t))
        {
            return tree;
        }
        let elapsed = clock.now().saturating_sub(start);
        if elapsed >= budget {
            return Err(format!("{label}: timeout; last state={last}; see UIA_FIRST_PARTIAL_SOURCE/UIA_NATIVE_FAILURE for call, HRESULT, elapsed_ms and node identity"));
        }
        clock
            .wait(Duration::from_millis(500).min(budget - elapsed))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_core::model::{Incomplete, IncompleteReason};
    use std::cell::Cell;
    struct Manual(Duration);
    impl Clock for Manual {
        fn now(&self) -> Duration {
            self.0
        }
        async fn tick(&mut self) {
            self.0 += Duration::from_millis(25);
        }
    }
    fn partial() -> RawTree {
        RawTree {
            incomplete: Some(Incomplete {
                reason: IncompleteReason::PartialError,
                read_nodes: 53,
                unread_nodes: None,
            }),
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn partial_tree_retries_at_500ms_and_complete_tree_requires_external_oracle() {
        let reads = Cell::new(0);
        let oracles = Cell::new(0);
        let mut clock = Manual(Duration::ZERO);
        wait(
            &mut clock,
            Duration::from_secs(30),
            "fixture",
            || {
                oracles.set(oracles.get() + 1);
                if reads.get() < 2 {
                    Err("JSON not ready".into())
                } else {
                    Ok(serde_json::json!({"counter":0}))
                }
            },
            async || {
                reads.set(reads.get() + 1);
                Ok(if reads.get() == 1 {
                    partial()
                } else {
                    RawTree::default()
                })
            },
            |_| true,
        )
        .await
        .unwrap();
        assert_eq!(reads.get(), 3);
        assert_eq!(oracles.get(), 6);
        assert_eq!(clock.0, Duration::from_secs(1));
    }
    #[tokio::test]
    async fn incomplete_tree_timeout_keeps_last_oracle_summary_and_attempt_count() {
        let mut clock = Manual(Duration::ZERO);
        let error = wait(
            &mut clock,
            Duration::from_secs(30),
            "counter_button",
            || Ok(serde_json::json!({"counter":7})),
            async || Ok(partial()),
            |_| true,
        )
        .await
        .unwrap_err();
        assert_eq!(clock.0, Duration::from_secs(30));
        for detail in [
            "attempt=61",
            "read_nodes: 53",
            "counter",
            "Number(7)",
            "PartialError",
        ] {
            assert!(error.contains(detail), "{error}");
        }
    }
    #[tokio::test]
    async fn missing_control_and_pending_native_call_never_pass() {
        for pending in [false, true] {
            let mut clock = Manual(Duration::ZERO);
            let error = wait(
                &mut clock,
                Duration::from_millis(500),
                "missing",
                || Ok(serde_json::json!({"counter":0})),
                async || {
                    if pending {
                        std::future::pending::<()>().await;
                    }
                    Ok(RawTree::default())
                },
                |_| false,
            )
            .await
            .unwrap_err();
            assert!(error.contains("last_oracle=Ok"));
            assert_eq!(clock.0, Duration::from_millis(500));
        }
    }
}
