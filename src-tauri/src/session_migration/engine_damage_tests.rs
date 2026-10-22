//! Corrupt sources remain read-only and retain their diagnosis through publication.
use super::*;
use crate::session_migration::{
    prepare::{self, PrepareBatchState, PrepareItemState},
    scan_selection,
    test_home::IsolatedAppHome,
    types::ImportOutcome,
};

const PROVIDERS: [Provider; 3] = [Provider::Codex, Provider::ClaudeCode, Provider::Cursor];

fn message(provider: Provider) -> &'static [u8] {
    match provider {
        Provider::Codex => {
            b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"readable turn\"}}\n"
        }
        Provider::ClaudeCode => {
            b"{\"type\":\"user\",\"uuid\":\"u1\",\"sessionId\":\"damage-test\",\"message\":{\"content\":\"readable turn\"}}\n"
        }
        Provider::Cursor => {
            b"{\"role\":\"user\",\"id\":\"u1\",\"content\":\"readable turn\"}\n"
        }
        _ => unreachable!(),
    }
}

fn source(home: &IsolatedAppHome, bytes: &[u8]) -> (PathBuf, PathBuf) {
    let root = home.path.join("source");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("damaged.jsonl");
    std::fs::write(&path, bytes).unwrap();
    (root, path)
}

fn prepare_scan(scan: &ScanState) -> prepare::PrepareBatchStatus {
    let selected = scan_selection::freeze_current(&scan.scan_id, scan.epoch).unwrap();
    let batch = prepare::start(&scan.scan_id, scan.epoch, &selected.selection_id, vec![]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let status = prepare::status(&batch.batch_id).unwrap();
        if !status.state.runnable() && !prepare::running_for_test(&batch.batch_id) {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "preparation timed out: {status:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        prepare::reset_runtime();
        reset_runtime();
    }
}

fn partial_case(damage: &[u8], expected: &str) {
    partial_case_for(&PROVIDERS, damage, expected);
}

fn partial_case_for(providers: &[Provider], damage: &[u8], expected: &str) {
    for &provider in providers {
        let home = IsolatedAppHome::acquire("import-damage-partial");
        let _reset = Reset;
        reset_runtime();
        let bytes = [message(provider), damage].concat();
        let (root, path) = source(&home, &bytes);
        let scanned = scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
        assert!(scanned.complete && scanned.issues.is_empty(), "{scanned:?}");
        assert_eq!(scanned.candidates.len(), 1);
        let candidate = &scanned.candidates[0];
        let (_, details) =
            preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
        assert_eq!(details.outcome, ImportOutcome::Partial, "{provider:?}");
        let expected = if provider == Provider::Cursor && expected == "unknown-record" {
            "unknown-role"
        } else {
            expected
        };
        assert!(
            details.issues.iter().any(|issue| issue.code == expected),
            "{details:?}"
        );
        assert!(details
            .events
            .iter()
            .any(|event| event.text.contains("readable turn")));
        assert!(!serde_json::to_string(&details)
            .unwrap()
            .contains("never expose"));

        let prepared = prepare_scan(&scanned);
        assert_eq!(prepared.state, PrepareBatchState::Completed);
        assert_eq!(prepared.prepared, 1);
        let inputs = prepare::validated_plan_inputs(&prepared.batch_id).unwrap();
        let mappings = prepare::mappings(&prepared.batch_id, None).unwrap();
        let plan = plan_from_validated_prepare(inputs, mappings).unwrap();
        let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
        crate::session_migration::job_control::run_pending().unwrap();
        let finished = job_get(&queued.job_id).unwrap();
        assert_eq!(
            finished.state,
            JobState::CompletedWithIssues,
            "{finished:?}"
        );
        let item = &finished.items[0];
        assert_eq!(item.phase, "committed");
        assert_eq!(item.outcome.as_deref(), Some("partial"));
        let page = events_window(
            item.import_id.as_deref().unwrap(),
            item.revision_id.as_deref(),
            None,
            Some(50),
            None,
            None,
        )
        .unwrap();
        assert_eq!(page.health.outcome, "partial");
        assert!(page
            .health
            .issues
            .iter()
            .any(|issue| issue.code == expected));
        assert!(!page.events.is_empty());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            bytes,
            "{provider:?} source changed"
        );
    }
}

#[test]
fn truncated_jsonl_retains_partial_diagnosis_after_publication() {
    partial_case(b"{\"type\":", "trailing-incomplete");
}

#[test]
fn invalid_utf8_retains_partial_diagnosis_after_publication() {
    partial_case(b"{\"text\":\"\xff\"}\n", "invalid-utf8");
}

#[test]
fn binary_record_retains_partial_diagnosis_after_publication() {
    partial_case(b"\x7fELF\0\x01\x02\n", "binary-record");
}

#[test]
fn broken_jsonl_retains_partial_diagnosis_after_publication() {
    partial_case(b"not JSON\n", "broken-record");
}

#[test]
fn truncated_terminated_jsonl_record_is_damaged_instead_of_a_pending_tail() {
    partial_case(b"{\"type\":\n", "broken-record");
}

#[test]
fn unknown_record_type_retains_partial_diagnosis_after_publication() {
    partial_case(
        b"{\"type\":\"future-record\",\"private\":\"never expose\"}\n",
        "unknown-record",
    );
}

#[test]
fn missing_record_type_is_diagnosed_instead_of_silently_skipped() {
    partial_case(b"{\"private\":\"never expose\"}\n", "unknown-record");
}

#[test]
fn unknown_codex_event_type_is_diagnosed_instead_of_silently_skipped() {
    partial_case_for(
        &[Provider::Codex],
        b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"future-record\",\"private\":\"never expose\"}}\n",
        "unknown-record",
    );
}

#[test]
fn wholly_damaged_jsonl_fails_preview_and_preparation_without_publication() {
    for provider in PROVIDERS {
        for (bytes, reason) in [
            (b"{\"type\":".as_slice(), "trailing-incomplete"),
            (b"{\"text\":\"\xff\"}\n".as_slice(), "invalid-utf8"),
            (b"\x7fELF\0\x01\x02\n".as_slice(), "binary-record"),
            (b"not JSON\n".as_slice(), "broken-record"),
        ] {
            let home = IsolatedAppHome::acquire("import-damage-unreadable");
            let _reset = Reset;
            reset_runtime();
            let (root, path) = source(&home, bytes);
            let scanned =
                scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
            assert_eq!(scanned.candidates.len(), 1);
            let candidate = &scanned.candidates[0];
            let error =
                preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap_err();
            assert_eq!(error.code, ErrorCode::FormatUnsupported);
            assert_eq!(error.message, reason);
            let prepared = prepare_scan(&scanned);
            assert_eq!(prepared.state, PrepareBatchState::Failed);
            assert_eq!(prepared.prepared, 0);
            assert_eq!(prepared.failed, 1);
            let items = prepare::items_page(&prepared.batch_id, None, None).unwrap();
            assert_eq!(items.items[0].state, PrepareItemState::Failed);
            assert_eq!(items.items[0].error.as_ref().unwrap(), &error);
            assert!(list_entries(&open_catalog().unwrap()).unwrap().is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
}

#[test]
fn zero_byte_jsonl_is_a_scan_issue_instead_of_an_empty_success() {
    for provider in PROVIDERS {
        let home = IsolatedAppHome::acquire("import-damage-empty");
        let _reset = Reset;
        let (root, path) = source(&home, b"");
        let scanned = scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
        assert!(scanned.complete && scanned.candidates.is_empty());
        assert_eq!(scanned.issues.len(), 1);
        assert_eq!(scanned.issues[0].code, "FORMAT_UNSUPPORTED");
        assert_eq!(scanned.issues[0].message, "empty-jsonl");
        let error = adapters::normalize_path(provider, &path).unwrap_err();
        assert_eq!(error.message, "empty-jsonl");
        assert!(std::fs::read(path).unwrap().is_empty());
    }
}

#[test]
fn oversized_jsonl_line_fails_with_resource_diagnosis_and_preserves_bytes() {
    let bytes = vec![b'x'; crate::session_migration::budget::JSON_RECORD_MAX_BYTES + 1];
    for provider in PROVIDERS {
        let home = IsolatedAppHome::acquire("import-damage-oversize");
        let _reset = Reset;
        let (root, path) = source(&home, &bytes);
        let scanned = scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
        assert_eq!(scanned.candidates.len(), 1);
        let error = preview(&scanned.candidates[0].candidate_id, None).unwrap_err();
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(error.message, "jsonl-record-too-large");
        let prepared = prepare_scan(&scanned);
        assert_eq!(prepared.prepared, 0);
        assert_eq!(prepared.failed, 1);
        let items = prepare::items_page(&prepared.batch_id, None, None).unwrap();
        assert_eq!(
            items.items[0].error.as_ref().unwrap().message,
            "jsonl-record-too-large"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn changed_source_fails_preview_and_preparation_without_repairing_source() {
    for provider in PROVIDERS {
        let home = IsolatedAppHome::acquire("import-damage-changed");
        let _reset = Reset;
        let (root, path) = source(&home, message(provider));
        let scanned = scan_start(vec![(provider, root.to_string_lossy().into_owned())]).unwrap();
        let rewritten = [message(provider), b"{\"changed\":true}\n"].concat();
        std::fs::write(&path, &rewritten).unwrap();
        assert_eq!(
            preview(&scanned.candidates[0].candidate_id, None)
                .unwrap_err()
                .code,
            ErrorCode::SourceChanged
        );
        let prepared = prepare_scan(&scanned);
        assert_eq!(prepared.prepared, 0);
        assert_eq!(prepared.failed, 1);
        let items = prepare::items_page(&prepared.batch_id, None, None).unwrap();
        assert_eq!(
            items.items[0].error.as_ref().unwrap().code,
            ErrorCode::SourceChanged
        );
        assert_eq!(std::fs::read(path).unwrap(), rewritten);
    }
}
