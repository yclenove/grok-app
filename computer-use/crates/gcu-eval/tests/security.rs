//! Evidence integrity only. These checks never certify native safety or run the cited tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

const SECURITY: &str = "computer-use/docs/security.md";
const INVENTORY: &str = "computer-use/docs/data-inventory.md";
const SANDBOX: &str = "computer-use/crates/gcu-script/tests/sandbox.rs";
const REVIEW: &str = "computer-use/docs/script-sandbox-review.md";
const ITEMS: [&str; 10] = [
    "S1 拒绝类别",
    "S2 审批与 Stop",
    "S3 gateway 与 Host",
    "S4 脚本沙箱",
    "S5 脱敏与数据",
    "S6 剪贴板",
    "S7 组合键黑名单",
    "S8 提示注入",
    "S9 平台权限与身份",
    "S10 供应链",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_owned()
}

/// Evidence cites files outside `computer-use/` (src-tauri, .github, src). Build
/// hosts sync only `computer-use/`, so there the checks cannot run; CI always
/// has the full tree and must enforce them.
fn full_tree() -> bool {
    if root().join(".github/workflows/computer-use.yml").exists() {
        return true;
    }
    assert!(
        std::env::var_os("CI").is_none(),
        "CI must run the evidence checks on the full repository tree"
    );
    eprintln!("SKIP: partial checkout (no .github); evidence checks run in CI");
    false
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path))
        .unwrap_or_else(|error| panic!("cannot read evidence {path}: {error}"))
}

fn table<'a>(doc: &'a str, name: &str, columns: usize) -> Result<Vec<Vec<&'a str>>, String> {
    let start = format!("<!-- {name}:start -->");
    let end = format!("<!-- {name}:end -->");
    if doc.matches(&start).count() != 1 || doc.matches(&end).count() != 1 {
        return Err(format!("{name}: expected one marker pair"));
    }
    let (_, body) = doc.split_once(&start).unwrap();
    let (body, _) = body.split_once(&end).ok_or("reversed table markers")?;
    let lines: Vec<_> = body
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 3 {
        return Err(format!("{name}: empty table"));
    }
    let mut rows = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim();
        if !line.starts_with('|') || !line.ends_with('|') {
            return Err(format!("{name}: non-table line"));
        }
        let cells: Vec<_> = line[1..line.len() - 1].split('|').map(str::trim).collect();
        if cells.len() != columns {
            return Err(format!(
                "{name}: expected {columns} columns, got {}",
                cells.len()
            ));
        }
        if i == 1 {
            if cells
                .iter()
                .any(|cell| cell.len() < 3 || !cell.chars().all(|c| c == '-' || c == ':'))
            {
                return Err(format!("{name}: invalid header separator"));
            }
        } else if i > 1 {
            rows.push(cells);
        }
    }
    Ok(rows)
}

fn codes(text: &str) -> Vec<&str> {
    text.split('`')
        .enumerate()
        .filter_map(|(i, part)| (i % 2 == 1).then_some(part))
        .collect()
}

fn references(text: &str) -> Vec<&str> {
    codes(text)
        .into_iter()
        .filter(|part| {
            part.starts_with("computer-use/")
                || part.starts_with("src-tauri/")
                || part.starts_with("src/")
                || part.starts_with(".github/")
                || part.starts_with("docs/")
        })
        .collect()
}

fn existing_file(reference: &str) -> Result<PathBuf, String> {
    let file = reference.split('#').next().unwrap();
    let path = Path::new(file);
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!("unsafe evidence path: {reference}"));
    }
    let path = root().join(path);
    let canonical = path
        .canonicalize()
        .map_err(|_| format!("missing evidence: {reference}"))?;
    if !canonical.is_file() || !canonical.starts_with(root().canonicalize().unwrap()) {
        return Err(format!("evidence escapes repository: {reference}"));
    }
    Ok(canonical)
}

fn test_names(source: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    let mut test = false;
    for line in source.lines().map(str::trim) {
        if line.starts_with("#[test]") || line.starts_with("#[tokio::test") {
            test = true;
        }
        let declaration = line.strip_prefix("async ").unwrap_or(line);
        if let Some(declaration) = declaration.strip_prefix("fn ") {
            if test {
                if let Some((name, _)) = declaration.split_once('(') {
                    names.insert(name);
                }
            }
            test = false;
        }
    }
    names
}

fn test_reference(reference: &str) -> Result<(), String> {
    let (file, name) = reference
        .split_once('#')
        .ok_or_else(|| format!("test needs a name: {reference}"))?;
    if !file.ends_with(".rs") || name.is_empty() {
        return Err(format!("invalid test reference: {reference}"));
    }
    let source = std::fs::read_to_string(existing_file(reference)?).map_err(|e| e.to_string())?;
    if !test_names(&source).contains(name) {
        return Err(format!("missing test declaration: {reference}"));
    }
    Ok(())
}

fn checklist(doc: &str) -> Result<Vec<Vec<&str>>, String> {
    let rows = table(doc, "security-checklist", 6)?;
    if rows.iter().map(|row| row[0]).collect::<Vec<_>>() != ITEMS {
        return Err("the ten section-seven items must appear once in order".into());
    }
    for row in &rows {
        if row.iter().any(|cell| cell.is_empty()) {
            return Err(format!("{}: missing evidence or explicit gap", row[0]));
        }
        if !matches!(row[4], "signed" | "partial" | "not_run" | "needs-decision") {
            return Err(format!("{}: invalid or contradictory status", row[0]));
        }
        let tests: Vec<_> = references(row[2])
            .into_iter()
            .filter(|r| r.contains('#'))
            .collect();
        if tests.is_empty() {
            return Err(format!("{}: no traceable automated evidence", row[0]));
        }
        for test in tests {
            test_reference(test)?;
        }
        for cell in [row[1], row[3]] {
            for reference in references(cell) {
                existing_file(reference)?;
            }
        }
        // The only signature supplied by this audit's authorization is the
        // controller's scoped Unix script review. New sign-offs need review.
        if row[4] == "signed"
            && (row[0] != ITEMS[3]
                || !references(row[3]).contains(&REVIEW)
                || !row[3].contains("Unix"))
        {
            return Err(format!("{}: no authorized scoped signature", row[0]));
        }
    }
    Ok(rows)
}

#[test]
fn security_checklist_ten_items_have_traceable_evidence_and_honest_status() {
    if !full_tree() {
        return;
    }
    let doc = read(SECURITY);
    let rows = checklist(&doc).unwrap();
    assert_eq!(rows.iter().filter(|row| row[4] == "signed").count(), 1);
    assert_eq!(rows[3][4], "signed");
    assert!(doc.contains("rcargo test -p gcu-eval --test security"));
    assert!(doc.contains("rcargo --sh 'cargo deny check'"));
}

#[test]
fn both_documents_reference_existing_sources_and_test_declarations() {
    if !full_tree() {
        return;
    }
    for document in [SECURITY, INVENTORY] {
        let doc = read(document);
        for reference in references(&doc) {
            existing_file(reference).unwrap();
            if reference.contains('#') {
                test_reference(reference).unwrap();
            }
        }
    }
}

#[test]
fn security_checklist_missing_duplicate_or_false_signature_is_rejected() {
    if !full_tree() {
        return;
    }
    let doc = read(SECURITY);
    assert!(checklist(&doc.replace(ITEMS[0], ITEMS[1])).is_err());
    let row = doc.lines().find(|line| line.starts_with("| S1 ")).unwrap();
    assert!(checklist(&doc.replace(row, "")).is_err());
    assert!(checklist(&doc.replace(row, &format!("{row}\n{row}"))).is_err());
    let row = doc.lines().find(|line| line.starts_with("| S8 ")).unwrap();
    assert!(row.contains("| not_run |"));
    assert!(checklist(&doc.replace(row, &row.replace("| not_run |", "| signed |"))).is_err());
    assert!(
        checklist(&doc.replace(row, &row.replace("| not_run |", "| not_run; signed |"))).is_err()
    );
    let row = doc.lines().find(|line| line.starts_with("| S4 ")).unwrap();
    assert!(checklist(&doc.replace(row, &row.replace(REVIEW, "missing-review.md"))).is_err());
    assert!(checklist(&doc.replace(row, &row.replace("Unix", "Windows"))).is_err());
}

#[test]
fn script_signature_references_existing_sandbox_tests_with_unix_scope() {
    if !full_tree() {
        return;
    }
    let doc = read(SECURITY);
    let rows = checklist(&doc).unwrap();
    let script = &rows[3];
    let tests: Vec<_> = references(script[2])
        .into_iter()
        .filter(|r| r.starts_with(SANDBOX))
        .collect();
    let review = read(REVIEW);
    for required in [
        "manifest_disables_forbidden_rquickjs_features",
        "prototype_constructor_escape_is_rejected",
        "memory_bomb_stays_inside_process_and_rejects",
        "host_buffer_over_32mb_is_not_retained",
        "queue_over_32_is_rejected_without_blocking",
        "native_concurrency_stays_at_one",
        "watchdog_kill_runs_within_500ms_of_deadline",
        "cancel_during_host_call_unblocks_without_replay",
    ] {
        assert!(
            tests
                .iter()
                .any(|reference| reference.split_once('#').unwrap().1 == required),
            "{required}"
        );
        assert!(
            review.contains(required),
            "signature basis must name {required}"
        );
    }
    assert!(review.contains("2026-10-17"));
    assert!(review.contains("not** signed for Windows"));
    assert!(script[3].contains("Windows not_run"));
    assert!(script[5].contains("script_rpc"));
}

#[test]
fn inventory_every_persistent_item_has_existing_cleanup_sources_or_explicit_gap() {
    if !full_tree() {
        return;
    }
    let doc = read(INVENTORY);
    let rows = table(&doc, "data-inventory", 10).unwrap();
    let mut ids = BTreeSet::new();
    for row in rows {
        assert!(ids.insert(row[0]), "duplicate inventory item {}", row[0]);
        assert!(
            row.iter().all(|cell| !cell.is_empty()),
            "incomplete {}",
            row[0]
        );
        assert!(matches!(
            row[1],
            "memory" | "disk" | "memory+disk" | "not_implemented"
        ));
        assert!(matches!(
            row[8].split(':').next().unwrap(),
            "yes" | "metadata-only" | "not_run" | "not_implemented"
        ));
        let refs = references(row[7]);
        assert!(
            !refs.is_empty(),
            "{}: cleanup source or design reference missing",
            row[0]
        );
        for reference in refs {
            existing_file(reference).unwrap();
            if reference.contains('#') {
                test_reference(reference).unwrap();
            }
        }
        if matches!(row[1], "disk" | "memory+disk") {
            assert!(
                row[5].contains("Unix") && row[5].contains("Windows"),
                "{}: platform permissions missing",
                row[0]
            );
            assert!(
                row[7].contains("not_run") || references(row[7]).iter().any(|r| r.contains('#')),
                "{}: no cleanup test or explicit gap",
                row[0]
            );
        }
    }
    for required in [
        "D1 原图",
        "D2 模型图与快照",
        "D3 缩略图与全图",
        "D4 审批 crop",
        "D5 MCP 临时图与溢出",
        "D6 CLI 导出",
        "D7 调试载荷",
        "D8 错误日志",
        "D9 调用摘要",
        "D10 Stop 锁存",
        "D11 停止原因标记",
        "D12 policy store",
        "D13 token 与控制端点",
        "D14 剪贴板",
        "D15 journal",
        "D16 Wayland restore token",
        "D17 脚本 worker",
    ] {
        assert!(ids.contains(required), "missing {required}");
    }
}

#[test]
fn evidence_bad_paths_missing_tests_and_comment_only_names_are_rejected() {
    if !full_tree() {
        return;
    }
    assert!(existing_file("../AGENTS.md").is_err());
    assert!(existing_file("/etc/passwd").is_err());
    assert!(existing_file("computer-use/docs/no-such-evidence.md").is_err());
    assert!(test_reference(&format!("{SANDBOX}#no_such_test")).is_err());
    assert!(test_reference(SANDBOX).is_err());
    assert!(
        test_names("// fn pretend() {}\nfn helper() {}\n#[test]\nfn real() {}").contains("real")
    );
    assert!(
        !test_names("// fn pretend() {}\nfn helper() {}\n#[test]\nfn real() {}")
            .contains("pretend")
    );
    assert!(!test_names("fn helper() {}").contains("helper"));
}

#[test]
fn external_review_remains_unsigned_and_release_gate_is_not_waived() {
    if !full_tree() {
        return;
    }
    let doc = read(SECURITY);
    let rows = table(&doc, "external-review", 4).unwrap();
    assert_eq!(rows, vec![vec!["外部审阅者", "", "", "needs-decision"]]);
    assert!(doc.contains("07 §8"));
    assert!(doc.contains("L1 / Wayland / DOM"));
    assert!(doc.contains("不得发布"));
}

#[test]
fn payload_flag_is_read_only_from_log_policy() {
    if !full_tree() {
        return;
    }
    let name = "GCU_LOG_PAYLOADS";
    let needles = [
        format!("env::var(\"{name}\")"),
        format!("env::var_os(\"{name}\")"),
    ];
    let mut hits = Vec::new();
    walk_rust(&root(), &needles, &mut hits);
    let expected = root().join("computer-use/crates/gcu-host-protocol/src/log_policy.rs");
    assert_eq!(hits, vec![expected]);
}

fn walk_rust(dir: &Path, needles: &[String], hits: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = path.file_name().and_then(|item| item.to_str());
        if matches!(
            name,
            Some("target" | "node_modules" | ".git" | "dist" | ".grok")
        ) {
            continue;
        }
        if path.is_dir() {
            walk_rust(&path, needles, hits);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if needles.iter().any(|needle| source.contains(needle)) {
            hits.push(path);
        }
    }
}

fn action_pins(workflow: &str) -> Result<usize, String> {
    let mut count = 0;
    for line in workflow.lines() {
        let line = line.trim().trim_start_matches("- ");
        let Some(action) = line.strip_prefix("uses:") else {
            continue;
        };
        let action = action.split_whitespace().next().ok_or("empty action")?;
        if action.starts_with("./") {
            continue;
        }
        let (_, revision) = action.rsplit_once('@').ok_or("missing action revision")?;
        if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!("action is not SHA-pinned: {action}"));
        }
        count += 1;
    }
    Ok(count)
}

#[test]
fn supply_chain_actions_are_sha_pinned_and_deny_sbom_sources_exist() {
    if !full_tree() {
        return;
    }
    for workflow in [
        ".github/workflows/computer-use.yml",
        ".github/workflows/release-computer-use.yml",
    ] {
        assert!(action_pins(&read(workflow)).unwrap() > 0);
    }
    assert!(action_pins("- uses: actions/checkout@v4").is_err());
    assert!(action_pins("uses: actions/checkout").is_err());
    for source in [
        "computer-use/deny.toml",
        "computer-use/Cargo.lock",
        "computer-use/xtask/src/sbom.rs",
        "computer-use/xtask/src/release.rs",
    ] {
        existing_file(source).unwrap();
    }
    // Source/configuration integrity only. cargo-deny and actual signed SBOM
    // artifacts remain separate required checks, never inferred from this test.
}
