//! Bounded diagnostics from the immutable revision, not the visible event page.

use serde::Serialize;

const MAX_ISSUES: usize = 20;
const MAX_CODE_CHARS: usize = 100;
const MAX_MESSAGE_CHARS: usize = 1000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionHealth {
    pub outcome: String,
    pub issue_count: Option<usize>,
    pub issues: Vec<ReaderIssue>,
    pub diagnostics_limited: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderIssue {
    pub code: String,
    pub message: String,
}

fn bounded_text(value: Option<&serde_json::Value>, cap: usize, limited: &mut bool) -> String {
    let Some(text) = value.and_then(serde_json::Value::as_str) else {
        return String::new();
    };
    let mut chars = text.chars();
    let result: String = chars.by_ref().take(cap).collect();
    *limited |= chars.next().is_some();
    result
}

pub fn from_manifest(value: &serde_json::Value) -> RevisionHealth {
    let outcome = value
        .get("outcome")
        .and_then(serde_json::Value::as_str)
        .filter(|outcome| matches!(*outcome, "complete" | "partial" | "unsupported" | "failed"))
        .unwrap_or("unknown");
    let source = value.get("issues").and_then(serde_json::Value::as_array);
    let mut limited = source.is_some_and(|items| items.len() > MAX_ISSUES);
    let issues = source
        .into_iter()
        .flatten()
        .take(MAX_ISSUES)
        .map(|issue| ReaderIssue {
            code: bounded_text(issue.get("code"), MAX_CODE_CHARS, &mut limited),
            message: bounded_text(issue.get("message"), MAX_MESSAGE_CHARS, &mut limited),
        })
        .collect();
    RevisionHealth {
        outcome: outcome.to_owned(),
        issue_count: source.map(Vec::len),
        issues,
        diagnostics_limited: limited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn skipped_records_remain_visible_without_corresponding_events() {
        let report = from_manifest(&json!({
            "outcome": "partial",
            "issues": [{"code": "unknown-role", "message": "Skipped unsupported role", "span": "private-path"}],
            "evidence": {"path": "private-path"}
        }));
        assert_eq!(report.outcome, "partial");
        assert_eq!(report.issue_count, Some(1));
        assert_eq!(report.issues[0].code, "unknown-role");
        assert!(!serde_json::to_string(&report)
            .unwrap()
            .contains("private-path"));
        assert!(!report.diagnostics_limited);
    }

    #[test]
    fn old_or_unknown_manifest_is_not_silently_reported_complete() {
        for value in [json!({}), json!({"outcome":"future-state", "issues":null})] {
            let report = from_manifest(&value);
            assert_eq!(report.outcome, "unknown");
            assert_eq!(report.issue_count, None);
        }
        let complete = from_manifest(&json!({"outcome":"complete", "issues":[]}));
        assert_eq!(complete.issue_count, Some(0));
        assert_eq!(complete.outcome, "complete");
    }

    #[test]
    fn diagnostics_are_bounded_without_losing_total_or_splitting_unicode() {
        let report = from_manifest(&json!({
            "outcome":"partial",
            "issues": (0..200).map(|_| json!({"code":"x".repeat(200), "message":"🦀".repeat(3000)})).collect::<Vec<_>>()
        }));
        assert_eq!(report.issue_count, Some(200));
        assert_eq!(report.issues.len(), MAX_ISSUES);
        assert_eq!(report.issues[0].code.chars().count(), MAX_CODE_CHARS);
        assert_eq!(report.issues[0].message.chars().count(), MAX_MESSAGE_CHARS);
        assert!(report.diagnostics_limited);
    }

    #[test]
    fn malformed_issue_retains_count_but_never_renders_object_or_source_data() {
        let report = from_manifest(
            &json!({"outcome":"partial", "issues":[null, {"code":{}, "message":42}]}),
        );
        assert_eq!(report.issue_count, Some(2));
        assert!(report
            .issues
            .iter()
            .all(|issue| issue.code.is_empty() && issue.message.is_empty()));
    }
}
