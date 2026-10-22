//! codex-cli 0.160.0 rollouts exported by the owner on 2026-10-21 and scrubbed
//! by `tests/fixtures/session-migration/codex-0.160/scrub.py`, run through the
//! production scan → preview → plan → publish → reader path.
use super::*;
use crate::session_migration::test_home::IsolatedAppHome;
use crate::session_migration::types::{ContentBlock, ImportedEvent, MessageRole};

const MAIN: &str = "rollout-2026-10-22T00-19-04-01a15f42-520d-7f22-8aa3-b764cfccc141.jsonl";
const BRANCH: &str = "rollout-2026-10-22T00-28-11-01a15f42-520d-7f22-8aa3-b764cfccc141_01a15f4a-aa3e-7781-8d92-dee133706e94.jsonl";
const EXEC: &str = "rollout-2026-10-22T00-20-29-01a15f43-a066-7351-b5de-316fdc562f9f.jsonl";
const PARENT_ID: &str = "01a15f42-520d-7f22-8aa3-b764cfccc141";
const BRANCH_ID: &str = "01a15f42-520d-7f22-8aa3-b764cfccc141_01a15f4a-aa3e-7781-8d92-dee133706e94";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/session-migration/codex-0.160")
        .join(name);
    std::fs::read(path).unwrap()
}

/// Codex keeps rollouts in dated folders; the branch lands in the resume day.
fn write_root(home: &IsolatedAppHome, files: &[(&str, &str)]) -> PathBuf {
    let root = home.path.join("codex home 演示");
    for (day, name) in files {
        let dir = root.join("sessions/2026/10").join(day);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), fixture(name)).unwrap();
    }
    root
}

fn scan(root: &Path) -> Vec<SourceCandidate> {
    scan_start(vec![(Provider::Codex, root.to_string_lossy().into_owned())])
        .unwrap()
        .candidates
}

fn by_suffix<'a>(candidates: &'a [SourceCandidate], name: &str) -> &'a SourceCandidate {
    candidates
        .iter()
        .find(|c| c.locator.ends_with(name))
        .unwrap_or_else(|| panic!("{name} not discovered: {candidates:?}"))
}

fn publish(candidate: &SourceCandidate) -> (String, String) {
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let plan = plan(vec![preview_id], vec![]).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, &Uuid::new_v4().to_string()).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let job = job_get(&queued.job_id).unwrap();
    // A1's image was already unreadable inside Codex, so the honest outcome is
    // partial: the job completes with issues and no item fails.
    assert!(
        matches!(
            job.state,
            JobState::Completed | JobState::CompletedWithIssues
        ),
        "{job:?}"
    );
    assert!(
        job.items
            .iter()
            .all(|item| item.phase == "committed" && item.error.is_none()),
        "{job:?}"
    );
    (
        job.items[0].import_id.clone().unwrap(),
        job.items[0].revision_id.clone().unwrap(),
    )
}

/// The complete immutable archive, not the selection-scoped reader view.
fn archived(import_id: &str, revision: &str) -> Vec<ImportedEvent> {
    let path = crate::session_migration::catalog::revision_dir(import_id, revision)
        .unwrap()
        .join("events.jsonl");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str::<ImportedEvent>(line).unwrap())
        .collect()
}

/// The only adapter issues are Codex's encrypted reasoning items, which are
/// disclosed and never decoded (F11). A1's unreadable image is an asset state.
/// Injected context must not become the title.
fn assert_only_encrypted_reasoning_issues(candidate: &SourceCandidate) {
    let (_, details) = preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    assert!(!details.issues.is_empty());
    for issue in &details.issues {
        assert!(
            issue.code == "unsupported-item" && issue.message == "response_item type reasoning",
            "{:?}",
            details.issues
        );
    }
    assert!(
        !candidate.title.contains("AGENTS") && !candidate.title.contains("redacted"),
        "injected context is not a title: {}",
        candidate.title
    );
}

fn user_texts(events: &[ImportedEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ImportedEvent::Message {
                role: MessageRole::User,
                blocks,
                ..
            } => Some(
                blocks
                    .iter()
                    .filter_map(ContentBlock::plain_text)
                    .collect::<Vec<_>>()
                    .join(""),
            ),
            _ => None,
        })
        .collect()
}

#[test]
fn codex_0160_tool_outputs_keep_text_images_and_failures_after_publication() {
    let home = IsolatedAppHome::acquire("codex-0160-tools");
    reset_runtime();
    let root = write_root(&home, &[("22", MAIN)]);
    let before = std::fs::read(root.join("sessions/2026/10/22").join(MAIN)).unwrap();
    let candidates = scan(&root);
    assert_only_encrypted_reasoning_issues(by_suffix(&candidates, MAIN));
    let (import_id, revision) = publish(by_suffix(&candidates, MAIN));
    reset_runtime();
    let events = archived(&import_id, &revision);
    let results: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ImportedEvent::ToolResult { blocks, .. } => Some(blocks),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 6, "every custom_tool_call_output is kept");
    // custom_tool_call carries a string `input`, not `arguments`.
    for event in &events {
        if let ImportedEvent::ToolCall { input, .. } = event {
            let shown = serde_json::to_string(input).unwrap();
            assert!(shown.contains("tools."), "tool input kept: {shown}");
        }
    }
    for blocks in &results {
        assert!(
            blocks
                .iter()
                .any(|b| b.plain_text().is_some_and(|t| !t.is_empty())),
            "tool output must not be empty: {blocks:?}"
        );
    }
    let all_text: String = results
        .iter()
        .flat_map(|blocks| blocks.iter().filter_map(ContentBlock::plain_text))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(all_text.contains("2 + 3 = 5"));
    assert!(all_text.contains("\\\"exit_code\\\":1") || all_text.contains("\"exit_code\":1"));
    assert!(all_text.contains("ValueError"));
    let image_results = results
        .iter()
        .filter(|blocks| {
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlock::Asset { .. }))
        })
        .count();
    assert_eq!(image_results, 1, "view_image output keeps its image block");
    let tool_image = results
        .iter()
        .flat_map(|blocks| blocks.iter())
        .find_map(|block| match block {
            ContentBlock::Asset { asset_id, .. } => Some(asset_id.clone()),
            _ => None,
        })
        .unwrap();
    let assets: Vec<crate::session_migration::types::AssetRecord> = serde_json::from_slice(
        &std::fs::read(
            crate::session_migration::catalog::revision_dir(&import_id, &revision)
                .unwrap()
                .join("assets.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let states: Vec<_> = assets
        .iter()
        .map(|a| (a.asset_id.clone(), a.state, a.source_ref.clone()))
        .collect();
    // Both inline images (A1's img-a.png and the view_image result) are stored;
    // nothing is invented for the `-` path Codex itself could not read.
    assert!(assets.iter().any(|a| a.asset_id == tool_image));
    for asset in &assets {
        let inline = asset
            .source_ref
            .as_deref()
            .is_some_and(|r| r.starts_with("data:image/"));
        if inline {
            assert_eq!(
                asset.state,
                crate::session_migration::types::AssetState::Stored,
                "{states:?}"
            );
        } else {
            assert_ne!(
                asset.state,
                crate::session_migration::types::AssetState::Stored,
                "{states:?}"
            );
        }
    }
    assert_eq!(assets.len(), 2, "{states:?}");
    // Codex wrote no image item for `-`; its own note stays as message text.
    assert!(user_texts(&events)
        .iter()
        .any(|t| t.contains("Codex could not read the local image at `-`")));
    assert_eq!(
        std::fs::read(root.join("sessions/2026/10/22").join(MAIN)).unwrap(),
        before,
        "source unchanged"
    );
    reset_runtime();
}

#[test]
fn codex_0160_rollback_branch_is_its_own_session_with_the_parent_prefix() {
    let home = IsolatedAppHome::acquire("codex-0160-branch");
    reset_runtime();
    let root = write_root(&home, &[("22", MAIN), ("22", EXEC), ("23", BRANCH)]);
    let candidates = scan(&root);
    assert_eq!(candidates.len(), 3, "{candidates:?}");
    let main = by_suffix(&candidates, MAIN);
    let branch = by_suffix(&candidates, BRANCH);
    assert_eq!(main.native_session_id, PARENT_ID);
    // Titles are the user's own words: no injected context, no image wrapper.
    assert_eq!(main.title, "图里写的是什么字？只回答文字。");
    assert_eq!(branch.title, "calc.py 一共有几行？");
    assert_eq!(
        branch.native_session_id, BRANCH_ID,
        "a resume branch must not reuse the parent session identity"
    );

    // The verified base splices cleanly: no history-base issue, only the
    // disclosed encrypted reasoning carried over from the parent prefix.
    assert_only_encrypted_reasoning_issues(branch);
    let (main_import, main_revision) = publish(main);
    let (branch_import, branch_revision) = publish(branch);
    assert_ne!(main_import, branch_import, "two separate entries");
    reset_runtime();

    let branch_events = archived(&branch_import, &branch_revision);
    let conn = open_catalog().unwrap();
    assert_eq!(
        get_entry(&conn, &branch_import).unwrap().unwrap().title,
        "calc.py 一共有几行？",
        "the imported branch keeps the title discovery showed"
    );
    drop(conn);
    let users = user_texts(&branch_events);
    for prompt in [
        "运行 `python calc.py 2 3`",
        "运行 `python calc.py 2 x`",
        "用 view_image 工具查看 img-b.png",
    ] {
        assert!(
            users.iter().any(|t| t.contains(prompt)),
            "branch keeps the parent prefix turn {prompt}: {users:?}"
        );
    }
    assert!(users.iter().any(|t| t.contains("calc.py 一共有几行")));
    assert!(
        !users.iter().any(|t| t.contains("edited by codex")),
        "the rolled-back parent turn is not part of the branch"
    );
    assert!(
        !branch_events
            .iter()
            .any(|e| matches!(e, ImportedEvent::Compaction { .. })),
        "the parent's later compaction is outside the branch base"
    );

    let main_events = archived(&main_import, &main_revision);
    let main_users = user_texts(&main_events);
    assert!(main_users.iter().any(|t| t.contains("edited by codex")));
    assert!(!main_users.iter().any(|t| t.contains("calc.py 一共有几行")));
    assert!(main_events
        .iter()
        .any(|e| matches!(e, ImportedEvent::Compaction { .. })));
    reset_runtime();
}

#[test]
fn codex_0160_branch_without_its_parent_is_partial_and_not_spliced() {
    let home = IsolatedAppHome::acquire("codex-0160-orphan");
    reset_runtime();
    let root = write_root(&home, &[("23", BRANCH)]);
    let candidates = scan(&root);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].native_session_id, BRANCH_ID);
    let (_, details) = preview(
        &candidates[0].candidate_id,
        Some(&candidates[0].source_revision),
    )
    .unwrap();
    assert!(
        details
            .issues
            .iter()
            .any(|i| i.code == "codex-history-base-missing"),
        "{:?}",
        details.issues
    );
    assert!(!details
        .events
        .iter()
        .any(|e| e.text.contains("python calc.py 2 3")));
    assert!(details
        .events
        .iter()
        .any(|e| e.text.contains("calc.py 一共有几行")));
    reset_runtime();
}
