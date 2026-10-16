//! One bounded setup/recovery wait, independent of individual contract actions.

use std::{future::Future, time::Duration};

use gcu_core::{driver::Cancel, model::RawTree};
use serde_json::{json, Value};
use tokio::time::Instant;

use super::fixture_tree::{self, count};

pub const BUDGET: Duration = Duration::from_secs(5);
const INTERVAL: Duration = Duration::from_millis(100);

pub struct Ready {
    pub tree: RawTree,
    pub evidence: Value,
}

/// `read` receives the remaining setup budget and a token cancelled on expiry.
/// Read errors are terminal; only successfully reported partial trees are polled.
pub async fn wait<F, Fut>(phase: &str, mut read: F) -> Result<Ready, String>
where
    F: FnMut(Duration, Cancel) -> Fut,
    Fut: Future<Output = Result<RawTree, String>>,
{
    wait_for(phase, fixture_tree::READY_CONTROLS, &mut read).await
}

/// A necessary reset may begin on another tab; wait for its reset control
/// before switching back, then use `wait` for the full default fixture.
pub async fn wait_for<F, Fut>(phase: &str, controls: &[&str], mut read: F) -> Result<Ready, String>
where
    F: FnMut(Duration, Cancel) -> Fut,
    Fut: Future<Output = Result<RawTree, String>>,
{
    let missing = |raw: &RawTree| -> Vec<String> {
        controls
            .iter()
            .filter(|id| {
                raw.root.as_ref().is_none_or(|root| {
                    fixture_tree::find(root, &|n| fixture_tree::exact(n, id)).is_none()
                })
            })
            .map(|id| (*id).to_string())
            .collect()
    };
    let started = Instant::now();
    let deadline = started + BUDGET;
    let mut polls = 0;
    let mut first = None;
    let mut first_incomplete_at = None;
    let mut first_complete_after_incomplete_ms = None;
    let mut last = RawTree::default();
    let result = loop {
        if Instant::now() >= deadline {
            break Err("fixture readiness timed out after 5s".to_string());
        }
        polls += 1;
        let cancel = Cancel::default();
        let read_result = tokio::time::timeout_at(
            deadline,
            read(
                deadline.saturating_duration_since(Instant::now()),
                cancel.clone(),
            ),
        )
        .await;
        last = match read_result {
            Ok(Ok(raw)) => raw,
            Ok(Err(error)) => break Err(format!("fixture readiness read failed: {error}")),
            Err(_) => {
                cancel.cancel();
                break Err("fixture readiness timed out during tree read after 5s".into());
            }
        };
        if polls == 1 {
            first = Some(json!({"nodes": count(&last), "incomplete": last.incomplete}));
            // ADR 0004 evidence: a fixture missing known visible controls
            // cannot silently be presented as complete. The Host chooses the
            // required controls for the default tab or the necessary reset.
            if !missing(&last).is_empty() && last.incomplete.is_none() {
                break Err("ADR 0004: first partial fixture tree omitted incomplete".into());
            }
        }
        if let Some(fact) = &last.incomplete {
            first_incomplete_at.get_or_insert_with(Instant::now);
            if fact.read_nodes as usize != count(&last) {
                break Err(
                    "ADR 0004: incomplete.read_nodes differs from retained node count".into(),
                );
            }
        } else if let Some(at) = first_incomplete_at {
            first_complete_after_incomplete_ms
                .get_or_insert_with(|| at.elapsed().as_secs_f64() * 1000.0);
        }
        if last.incomplete.is_none() && missing(&last).is_empty() {
            break Ok(());
        }
        tokio::time::sleep_until((Instant::now() + INTERVAL).min(deadline)).await;
    };
    let evidence = json!({
        "kind": "FIXTURE_READINESS", "phase": phase, "ready": result.is_ok(),
        "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
        "first_incomplete_to_complete_ms": first_complete_after_incomplete_ms,
        "polls": polls, "polls_include_first_read": true, "first": first,
        "last_incomplete": last.incomplete, "last_nodes": count(&last),
        "missing": missing(&last), "error": result.as_ref().err(),
    });
    println!("{evidence}");
    result.map_err(|error| format!("{error}; {evidence}"))?;
    Ok(Ready {
        tree: last,
        evidence,
    })
}
