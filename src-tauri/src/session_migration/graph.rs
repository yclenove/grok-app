//! Graph checks: unique ids, parent cycles, tool-call pairing.

use std::collections::{HashMap, HashSet};

use super::types::{EventRelation, ImportIssue, ImportedEvent, RelationKind, ToolOutcome};

#[derive(Debug, Clone, Default)]
pub struct GraphReport {
    pub issues: Vec<ImportIssue>,
    pub cycle: bool,
    pub missing_parents: u32,
    pub unpaired_calls: u32,
    pub unpaired_results: u32,
}

pub fn validate(events: &[ImportedEvent], relations: &[EventRelation]) -> GraphReport {
    let mut report = GraphReport::default();
    let mut by_id = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        if by_id.insert(event.id(), index).is_some() {
            report.issues.push(
                ImportIssue::new("duplicate-id", "event identity is repeated")
                    .with_span(event.id().to_string()),
            );
        }
    }
    let mut parents = vec![None; events.len()];
    for relation in relations {
        if relation.kind != RelationKind::Parent {
            continue;
        }
        let Some(&child) = by_id.get(relation.from.as_str()) else {
            report.issues.push(
                ImportIssue::new("missing-child", "relation child is absent")
                    .with_span(relation.from.clone()),
            );
            continue;
        };
        let Some(&parent) = by_id.get(relation.to.as_str()) else {
            report.missing_parents += 1;
            report.issues.push(
                ImportIssue::new("missing-parent", "relation parent is absent")
                    .with_span(relation.from.clone()),
            );
            continue;
        };
        if parents[child].is_some_and(|old| old != parent) {
            report.issues.push(
                ImportIssue::new("multiple-parents", "event has conflicting parents")
                    .with_span(relation.from.clone()),
            );
        } else {
            parents[child] = Some(parent);
        }
    }
    // Each node is visited at most twice. A long history must not trigger an
    // arbitrary depth cutoff or a quadratic walk from every event.
    let mut colors = vec![0u8; events.len()];
    for start in 0..events.len() {
        if colors[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if colors[index] == 2 {
                break;
            }
            if colors[index] == 1 {
                report.cycle = true;
                report.issues.push(
                    ImportIssue::new("parent-cycle", "parent chain contains a cycle")
                        .with_span(events[index].id().to_string()),
                );
                break;
            }
            colors[index] = 1;
            path.push(index);
            current = parents[index];
        }
        for index in path {
            colors[index] = 2;
        }
    }
    if report.cycle {
        return report;
    }
    let mut children = vec![Vec::new(); events.len()];
    let scope_boundaries: HashSet<usize> = relations
        .iter()
        .filter(|r| r.kind == RelationKind::Subagent)
        .filter_map(|r| by_id.get(r.from.as_str()).copied())
        .collect();
    let mut stack = Vec::new();
    for (index, parent) in parents.iter().enumerate() {
        match parent {
            Some(parent) => children[*parent].push(index),
            None => stack.push((index, false, None)),
        }
    }
    // The nearest call must be an ancestor of the result. Matching only callId
    // across the whole file could claim a sibling/subagent's call succeeded.
    let mut active_calls: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut paired = HashSet::new();
    while let Some((index, exiting, restore_scope)) = stack.pop() {
        let event = &events[index];
        if exiting {
            if let ImportedEvent::ToolCall { call_id, .. } = event {
                if let Some(calls) = active_calls.get_mut(call_id.as_str()) {
                    calls.pop();
                }
            }
            if let Some(scope) = restore_scope {
                active_calls = scope;
            }
            continue;
        }
        let restore_scope = scope_boundaries
            .contains(&index)
            .then(|| std::mem::take(&mut active_calls));
        match event {
            ImportedEvent::ToolCall { call_id, .. } => {
                active_calls
                    .entry(call_id.as_str())
                    .or_default()
                    .push(index);
            }
            ImportedEvent::ToolResult {
                call_id, outcome, ..
            } => {
                if let Some(call) = active_calls
                    .get(call_id.as_str())
                    .and_then(|calls| calls.last())
                {
                    paired.insert(*call);
                } else {
                    report.unpaired_results += 1;
                    report.issues.push(
                        ImportIssue::new("unpaired-tool-result", "no matching call in this branch")
                            .with_span(event.id().to_string()),
                    );
                }
                if *outcome == ToolOutcome::Unknown {
                    report.issues.push(
                        ImportIssue::new("tool-unknown", "tool outcome is unknown")
                            .with_span(event.id().to_string()),
                    );
                }
            }
            _ => {}
        }
        stack.push((index, true, restore_scope));
        stack.extend(
            children[index]
                .iter()
                .rev()
                .map(|child| (*child, false, None)),
        );
    }
    for (index, event) in events.iter().enumerate() {
        if matches!(event, ImportedEvent::ToolCall { .. }) && !paired.contains(&index) {
            report.unpaired_calls += 1;
            report.issues.push(
                ImportIssue::new("unpaired-tool-call", "call has no result in its branch")
                    .with_span(event.id().to_string()),
            );
        }
    }
    report
}

/// Walk parent links from leaf to root without depth truncation or duplication.
/// Invalid/ambiguous graphs are reported by validate before creating a handoff.
pub fn lineage<'a>(
    events: &'a [ImportedEvent],
    relations: &[EventRelation],
    leaf: &str,
) -> Vec<&'a ImportedEvent> {
    let by_id: HashMap<&str, &ImportedEvent> = events.iter().map(|e| (e.id(), e)).collect();
    let mut parent: HashMap<&str, &str> = HashMap::new();
    for relation in relations {
        if relation.kind == RelationKind::Parent {
            parent.entry(&relation.from).or_insert(&relation.to);
        }
    }
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = leaf;
    while let Some(event) = by_id.get(current) {
        if !visited.insert(current) {
            break;
        }
        chain.push(*event);
        match parent.get(current) {
            Some(next) => current = next,
            None => break,
        }
    }
    chain.reverse();
    chain
}

/// Parent-chain ids from leaf to root. Does not load event bodies.
pub fn lineage_ids(relations: &[EventRelation], leaf: &str) -> Vec<String> {
    let mut parent: HashMap<&str, &str> = HashMap::new();
    for relation in relations {
        if relation.kind == RelationKind::Parent {
            parent.entry(&relation.from).or_insert(&relation.to);
        }
    }
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    let mut current = leaf;
    loop {
        if !visited.insert(current) {
            break;
        }
        chain.push(current.to_string());
        match parent.get(current) {
            Some(next) => current = next,
            None => break,
        }
    }
    chain.reverse();
    chain
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_migration::types::{EventBase, MessageRole, TimeOrigin};

    fn msg(id: &str, ordinal: u32) -> ImportedEvent {
        ImportedEvent::Message {
            base: EventBase {
                id: id.into(),
                ordinal,
                spans: vec![],
                timestamp: None,
                time_origin: TimeOrigin::Unknown,
            },
            role: MessageRole::User,
            blocks: vec![],
            complete: true,
        }
    }

    #[test]
    fn detects_duplicate_ids() {
        let events = vec![msg("a", 0), msg("a", 1)];
        let r = validate(&events, &[]);
        assert!(r.issues.iter().any(|i| i.code == "duplicate-id"));
    }

    fn parent(child: &str, parent: &str) -> EventRelation {
        EventRelation {
            kind: RelationKind::Parent,
            from: child.into(),
            to: parent.into(),
            confidence: super::super::types::RelationConfidence::Explicit,
        }
    }

    #[test]
    fn long_history_is_not_truncated_and_late_cycle_is_detected() {
        let events: Vec<_> = (0..60_001).map(|i| msg(&i.to_string(), i)).collect();
        let mut relations: Vec<_> = (1..60_001)
            .map(|i| parent(&i.to_string(), &(i - 1).to_string()))
            .collect();
        assert!(validate(&events, &relations).issues.is_empty());
        assert_eq!(lineage(&events, &relations, "60000").len(), 60_001);
        relations.push(parent("0", "60000"));
        assert!(validate(&events, &relations).cycle);
        assert_eq!(lineage(&events, &relations, "60000").len(), 60_001);
    }

    #[test]
    fn tool_ids_do_not_pair_across_sibling_branches_or_subagents() {
        use super::super::types::{RelationConfidence, ToolInput};
        let call = ImportedEvent::ToolCall {
            base: msg("call", 1).base().clone(),
            call_id: "same".into(),
            name: "Read".into(),
            input: ToolInput::Json {
                value: serde_json::json!({}),
            },
            parent_message_id: None,
        };
        let result = ImportedEvent::ToolResult {
            base: msg("result", 2).base().clone(),
            call_id: "same".into(),
            blocks: vec![],
            outcome: ToolOutcome::Success,
        };
        let events = vec![msg("root", 0), call, result];
        let sibling = vec![parent("call", "root"), parent("result", "root")];
        let report = validate(&events, &sibling);
        assert_eq!((report.unpaired_calls, report.unpaired_results), (1, 1));
        let mut main = vec![parent("call", "root"), parent("result", "call")];
        assert!(validate(&events, &main).issues.is_empty());
        main.push(EventRelation {
            kind: RelationKind::Subagent,
            from: "result".into(),
            to: "call".into(),
            confidence: RelationConfidence::Explicit,
        });
        let report = validate(&events, &main);
        assert_eq!((report.unpaired_calls, report.unpaired_results), (1, 1));
    }
}
