use super::{
    cancel_job, commit, events_page, job_get, plan, preview, propose_mappings, scan_start,
    search_imported_with_leaf_window,
};
use crate::session_migration::budget::{disk_sufficient, SESSION_BODY_MAX_BYTES};
use crate::session_migration::catalog::imports_dir;
use crate::session_migration::types::Provider;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn home_lock() -> crate::session_migration::test_home::IsolatedAppHome {
    crate::session_migration::test_home::IsolatedAppHome::acquire("r7")
}

fn require_r7_opt_in() {
    assert_eq!(
        std::env::var("GROK_R7_HARNESS").as_deref(),
        Ok("1"),
        "R7 large fixture generation requires GROK_R7_HARNESS=1"
    );
}

fn assert_search_finds_synthetic_needle(import_id: &str, needle: &str) {
    let mut cursor = None;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..1024 {
        let window = search_imported_with_leaf_window(
            import_id,
            None,
            None,
            needle,
            cursor.as_deref(),
            Some(5),
            None,
        )
        .unwrap();
        if !window.hits.is_empty() {
            return;
        }
        let next = window
            .next_cursor
            .expect("search must find the synthetic needle");
        assert!(
            seen.insert(next.clone()),
            "search cursor must make progress"
        );
        cursor = Some(next);
    }
    panic!("synthetic needle search exceeded its bounded page budget");
}

fn fixture_dir() -> PathBuf {
    std::env::var("GROK_R7_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("grok-r7-fixture"))
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    let mut n = 0u64;
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            n = n.saturating_add(dir_size(&p));
        } else if let Ok(meta) = p.metadata() {
            n = n.saturating_add(meta.len());
        }
    }
    n
}

fn write_100mib_session(path: &Path) -> u64 {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(b"[").unwrap();
    let chunk = vec![b'x'; 1024 * 1024];
    for i in 0..100u32 {
        if i > 0 {
            file.write_all(b",").unwrap();
        }
        let role = if i % 2 == 0 { "user" } else { "assistant" };
        write!(file, r#"{{"role":"{role}","content":""#).unwrap();
        file.write_all(&chunk).unwrap();
        if i == 99 {
            file.write_all(br#"r7-unique-needle"#).unwrap();
        }
        file.write_all(br#""}"#).unwrap();
    }
    file.write_all(b"]").unwrap();
    file.flush().unwrap();
    file.metadata().unwrap().len()
}

fn import_generic_file(path: &Path, request_id: &str) -> String {
    let scan = scan_start(vec![(
        Provider::GenericFile,
        path.parent().unwrap().to_string_lossy().into(),
    )])
    .unwrap();
    let cand = scan
        .candidates
        .iter()
        .find(|c| {
            c.locator
                .contains(&path.file_name().unwrap().to_string_lossy().into_owned())
        })
        .expect("exact synthetic source candidate");
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
    let plan = plan(vec![pid], maps).unwrap();
    let job = commit(&plan.plan_id, &plan.plan_hash, request_id).unwrap();
    crate::session_migration::job_control::run_pending().unwrap();
    let done = job_get(&job.job_id).unwrap();
    assert_eq!(done.items.len(), 1);
    assert!(done.items[0].error.is_none());
    assert!(done.items[0].revision_id.is_some());
    done.items[0]
        .import_id
        .clone()
        .expect("imported 100MiB session")
}

#[test]
#[ignore = "perf harness; run with --ignored and GROK_R7_HARNESS=1"]
fn r7_isolated_harness_records_api_times() {
    require_r7_opt_in();
    let home = home_lock();
    let tmp = home.path.clone();
    let cache = fixture_dir();
    std::fs::create_dir_all(&cache).unwrap();
    let cached_src = cache.join("chat-100mib.json");
    let cached_imports = cache.join("imports");
    let cached_id = cache.join("import_id.txt");
    let files = tmp.join("files");
    std::fs::create_dir_all(&files).unwrap();
    let big = files.join("chat-100mib.json");

    let fixture_start = Instant::now();
    let cache_hit = cached_src.is_file() && cached_imports.is_dir() && cached_id.is_file();
    let source_bytes;
    let import_id;
    if cache_hit {
        std::fs::copy(&cached_src, &big).expect("copy cached 100MiB fixture");
        copy_dir(&cached_imports, &imports_dir()).expect("copy cached imports");
        source_bytes = cached_src.metadata().unwrap().len();
        import_id = std::fs::read_to_string(&cached_id)
            .unwrap()
            .trim()
            .to_string();
    } else {
        source_bytes = write_100mib_session(&big);
        std::fs::copy(&big, &cached_src).expect("cache 100MiB source");
        import_id = import_generic_file(&big, "r7-100mib");
        copy_dir(&imports_dir(), &cached_imports).expect("cache imports dir");
        std::fs::write(&cached_id, &import_id).expect("cache import id");
    }
    let disk_delta = dir_size(&imports_dir());
    let fixture_ms = fixture_start.elapsed().as_millis();
    assert!(source_bytes >= 100 * 1024 * 1024);
    assert!(!import_id.is_empty());

    let api_start = Instant::now();
    let (page, next, _total) = events_page(&import_id, None, None, Some(20), None).unwrap();
    let api_cold_ms = api_start.elapsed().as_millis();
    let ui_first_visible_ms = "not-measured-in-backend-harness";
    assert!(!page.is_empty(), "100MiB session must page events");
    let warm = Instant::now();
    if let Some(cursor) = next {
        let _ = events_page(&import_id, None, Some(&cursor), Some(20), None).unwrap();
    } else {
        let _ = events_page(&import_id, None, None, Some(20), None).unwrap();
    }
    let api_warm_ms = warm.elapsed().as_millis();
    assert_search_finds_synthetic_needle(&import_id, "r7-unique-needle");

    let small = files.join("chat-cancel.json");
    std::fs::write(&small, r#"[{"role":"user","content":"cancel-me"}]"#).unwrap();
    let scan = scan_start(vec![(
        Provider::GenericFile,
        files.to_string_lossy().into(),
    )])
    .unwrap();
    let cand = scan
        .candidates
        .iter()
        .find(|c| c.locator.contains("chat-cancel.json"))
        .expect("cancel fixture");
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
    let plan = plan(vec![pid], maps).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, "r7-cancel").unwrap();
    let cancel = Instant::now();
    let stopped = cancel_job(&queued.job_id).unwrap();
    let cancel_confirm_ms = cancel.elapsed().as_millis();
    assert!(
        matches!(
            stopped.state,
            super::JobState::Cancelling | super::JobState::Cancelled
        ),
        "cancel_job must confirm on the shipped job: {:?}",
        stopped.state
    );

    eprintln!(
        "R7_HARNESS cache_hit={cache_hit} setup_fixture_ms={fixture_ms} api_cold_ms={api_cold_ms} api_warm_ms={api_warm_ms} ui_first_visible_ms={ui_first_visible_ms} cancel_confirm_ms={cancel_confirm_ms} disk_delta={disk_delta} source_bytes={source_bytes} rss=wrapper-measured cpu=wrapper-measured import_id={import_id}"
    );
    drop(home);
}

const GIB: u64 = 1024 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;
/// Prior 100 MiB run: disk_delta=1680344154 / source_bytes=104860767 ≈ 16.02×.
const R7_EXPAND_NUM: u128 = 1_680_344_154;
const R7_EXPAND_DEN: u128 = 104_860_767;
const BATCH_FILES: u32 = 4;
const CHUNKS_PER_FILE: u32 = 256;

fn gib_fixture_dir() -> PathBuf {
    std::env::var("GROK_R7_1GIB_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let sibling = PathBuf::from(r"H:\aicoding\grok-r7-1gib-fixture");
            if sibling.parent().is_some_and(|p| p.is_dir()) {
                sibling
            } else {
                fixture_dir().join("batch-1gib")
            }
        })
}

fn write_session_chunks(path: &Path, chunks: u32, needle: Option<&[u8]>) -> u64 {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(b"[").unwrap();
    let chunk = vec![b'x'; MIB as usize];
    for i in 0..chunks {
        if i > 0 {
            file.write_all(b",").unwrap();
        }
        let role = if i % 2 == 0 { "user" } else { "assistant" };
        write!(file, r#"{{"role":"{role}","content":""#).unwrap();
        file.write_all(&chunk).unwrap();
        if i + 1 == chunks {
            if let Some(needle) = needle {
                file.write_all(needle).unwrap();
            }
        }
        file.write_all(br#""}"#).unwrap();
    }
    file.write_all(b"]").unwrap();
    file.flush().unwrap();
    let len = file.metadata().unwrap().len();
    assert!(
        len <= SESSION_BODY_MAX_BYTES,
        "single body {} exceeds SESSION_BODY_MAX_BYTES",
        len
    );
    len
}

#[test]
#[ignore = "perf harness; run with --ignored and GROK_R7_HARNESS=1"]
fn r7_isolated_1gib_batch_records_api_times() {
    require_r7_opt_in();
    let home = home_lock();
    let tmp = home.path.clone();
    let estimated_new = ((GIB as u128) * R7_EXPAND_NUM / R7_EXPAND_DEN) as u64;
    let available = fs2::available_space(&tmp).unwrap_or(0);
    if !disk_sufficient(available, estimated_new) {
        panic!(
            "R7_1GIB STATUS=blocked-external disk-full available={available} estimated_new={estimated_new} path={}",
            tmp.display()
        );
    }

    let cache = gib_fixture_dir();
    std::fs::create_dir_all(&cache).unwrap();
    let cached_src = cache.join("src");
    let cached_imports = cache.join("imports");
    let cached_id = cache.join("import_id.txt");
    let cached_bytes = cache.join("source_bytes.txt");
    let files = tmp.join("files");
    std::fs::create_dir_all(&files).unwrap();

    let fixture_start = Instant::now();
    let cache_hit = cached_src.is_dir()
        && cached_imports.is_dir()
        && cached_id.is_file()
        && cached_bytes.is_file();
    let source_bytes;
    let import_id;
    if cache_hit {
        copy_dir(&cached_src, &files).expect("copy cached 1GiB sources");
        copy_dir(&cached_imports, &imports_dir()).expect("copy cached 1GiB imports");
        source_bytes = std::fs::read_to_string(&cached_bytes)
            .unwrap()
            .trim()
            .parse::<u64>()
            .unwrap();
        import_id = std::fs::read_to_string(&cached_id)
            .unwrap()
            .trim()
            .to_string();
    } else {
        std::fs::create_dir_all(&cached_src).unwrap();
        let mut total = 0u64;
        for i in 0..BATCH_FILES {
            let name = format!("chat-1gib-{i:02}.json");
            let dest = files.join(&name);
            let needle = if i == 0 {
                Some(br#"r7-1gib-needle"#.as_slice())
            } else {
                None
            };
            let n = write_session_chunks(&dest, CHUNKS_PER_FILE, needle);
            total = total.saturating_add(n);
            std::fs::copy(&dest, cached_src.join(&name)).expect("cache 1GiB chunk");
        }
        source_bytes = total;
        let mut ids = Vec::new();
        for i in 0..BATCH_FILES {
            let path = files.join(format!("chat-1gib-{i:02}.json"));
            ids.push(import_generic_file(&path, &format!("r7-1gib-batch-{i}")));
        }
        assert!(!ids.is_empty(), "1 GiB batch commit produced no import_id");
        import_id = ids[0].clone();
        copy_dir(&imports_dir(), &cached_imports).expect("cache 1GiB imports");
        std::fs::write(&cached_id, &import_id).expect("cache 1GiB import id");
        std::fs::write(&cached_bytes, source_bytes.to_string()).expect("cache 1GiB bytes");
    }
    let disk_delta = dir_size(&imports_dir());
    let fixture_ms = fixture_start.elapsed().as_millis();
    assert!(
        source_bytes >= GIB,
        "batch source_bytes {source_bytes} < 1 GiB"
    );
    assert!(!import_id.is_empty());

    crate::session_migration::continuation::take_load_events_calls();
    let api_start = Instant::now();
    let (page, next, _total) = events_page(&import_id, None, None, Some(20), None).unwrap();
    let api_cold_ms = api_start.elapsed().as_millis();
    let ui_first_visible_ms = "not-measured-in-backend-harness";
    assert!(!page.is_empty(), "1 GiB batch must page events");
    assert_eq!(
        crate::session_migration::continuation::take_load_events_calls(),
        0,
        "events_page must not load the full archive"
    );
    let warm = Instant::now();
    if let Some(cursor) = next {
        let _ = events_page(&import_id, None, Some(&cursor), Some(20), None).unwrap();
    } else {
        let _ = events_page(&import_id, None, None, Some(20), None).unwrap();
    }
    let api_warm_ms = warm.elapsed().as_millis();
    assert_search_finds_synthetic_needle(&import_id, "r7-1gib-needle");
    assert_eq!(
        crate::session_migration::continuation::take_load_events_calls(),
        0
    );

    let small = files.join("chat-1gib-cancel.json");
    std::fs::write(&small, r#"[{"role":"user","content":"cancel-1gib"}]"#).unwrap();
    let scan = scan_start(vec![(
        Provider::GenericFile,
        files.to_string_lossy().into(),
    )])
    .unwrap();
    let cand = scan
        .candidates
        .iter()
        .find(|c| c.locator.contains("chat-1gib-cancel.json"))
        .expect("1 GiB cancel fixture");
    let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
    let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
    let plan = plan(vec![pid], maps).unwrap();
    let queued = commit(&plan.plan_id, &plan.plan_hash, "r7-1gib-cancel").unwrap();
    let cancel = Instant::now();
    let stopped = cancel_job(&queued.job_id).unwrap();
    let cancel_confirm_ms = cancel.elapsed().as_millis();
    assert!(
        matches!(
            stopped.state,
            super::JobState::Cancelling | super::JobState::Cancelled
        ),
        "cancel_job must confirm on the shipped job: {:?}",
        stopped.state
    );

    eprintln!(
        "R7_1GIB cache_hit={cache_hit} setup_fixture_ms={fixture_ms} api_cold_ms={api_cold_ms} api_warm_ms={api_warm_ms} ui_first_visible_ms={ui_first_visible_ms} cancel_confirm_ms={cancel_confirm_ms} disk_delta={disk_delta} source_bytes={source_bytes} estimated_new={estimated_new} available={available} rss=wrapper-measured load_events_calls=0 cancel_job={:?} import_id={import_id}",
        stopped.state
    );
    drop(home);
}

#[test]
#[ignore = "v8 100MiB cold/warm; never treat import-cache copy as cold"]
fn v8_100mib_three_cold_three_warm_no_import_cache() {
    let run_start = Instant::now();
    eprintln!("V10_PHASE t_ms=0 phase=process-ready");
    std::thread::sleep(std::time::Duration::from_secs(1));
    let src_root = std::env::var("GROK_V8_100MIB_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("grok-v8-100mib-source"));
    std::fs::create_dir_all(&src_root).unwrap();
    let src = src_root.join("chat-100mib.json");
    if !src.is_file() || src.metadata().map(|m| m.len()).unwrap_or(0) < 100 * MIB {
        write_100mib_session(&src);
    }
    let source_bytes = src.metadata().unwrap().len();
    assert!(
        source_bytes >= 100 * MIB,
        "source must be >= 100 MiB, got {source_bytes}"
    );

    let mut over_3x = false;
    let mut imports_over_3x = false;
    for i in 0..3u32 {
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-start",
            run_start.elapsed().as_millis()
        );
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire(&format!(
            "v8-100mib-{i}"
        ));
        let files = home.path.join("files");
        std::fs::create_dir_all(&files).unwrap();
        let big = files.join("chat-100mib.json");
        std::fs::copy(&src, &big).expect("copy 100MiB source into isolated home");

        let t_scan = Instant::now();
        let scan = scan_start(vec![(
            Provider::GenericFile,
            files.to_string_lossy().into(),
        )])
        .unwrap();
        let scan_ms = t_scan.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-scan-done",
            run_start.elapsed().as_millis()
        );
        let cand = scan
            .candidates
            .iter()
            .find(|c| c.locator.contains("chat-100mib.json"))
            .expect("100MiB candidate");

        let t_preview = Instant::now();
        let (pid, _) = preview(&cand.candidate_id, Some(&cand.source_revision)).unwrap();
        let preview_ms = t_preview.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-preview-done",
            run_start.elapsed().as_millis()
        );

        let t_plan = Instant::now();
        let maps = propose_mappings(std::slice::from_ref(&pid), None).unwrap();
        let planned = plan(vec![pid], maps).unwrap();
        let plan_ms = t_plan.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-plan-done",
            run_start.elapsed().as_millis()
        );

        let t_commit = Instant::now();
        let job = commit(
            &planned.plan_id,
            &planned.plan_hash,
            &format!("v8-100mib-{i}"),
        )
        .unwrap();
        crate::session_migration::job_control::run_pending().unwrap();
        let done = job_get(&job.job_id).unwrap();
        let commit_ms = t_commit.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-commit-done",
            run_start.elapsed().as_millis()
        );
        let item = &done.items[0];
        let import_id = item.import_id.clone().expect("imported 100MiB session");
        let revision_id = item.revision_id.clone().expect("published revision");
        let dest =
            crate::session_migration::catalog::revision_dir(&import_id, &revision_id).unwrap();
        let published_bytes = dir_size(&dest);
        let imports_bytes = dir_size(&imports_dir());
        let ratio = published_bytes as f64 / source_bytes as f64;
        if published_bytes > source_bytes.saturating_mul(3) {
            over_3x = true;
        }
        if imports_bytes > source_bytes.saturating_mul(3) {
            imports_over_3x = true;
        }

        let t_page = Instant::now();
        let (page, _next, _total) = events_page(&import_id, None, None, Some(20), None).unwrap();
        let first_page_ms = t_page.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=cold-{i}-first-page-done",
            run_start.elapsed().as_millis()
        );
        assert!(!page.is_empty(), "cold {i} first page must not be empty");

        eprintln!(
            "V8_100MIB_COLD i={i} cache_state=cold-full-publish source_bytes={source_bytes} published_bytes={published_bytes} imports_bytes={imports_bytes} ratio={ratio:.4} scan_ms={scan_ms} preview_ms={preview_ms} plan_ms={plan_ms} commit_ms={commit_ms} first_page_ms={first_page_ms} ui_first_visible_ms=not-measured-in-backend-harness import_id={import_id}"
        );

        if i == 0 {
            for w in 0..3u32 {
                let t_w = Instant::now();
                let (wpage, _wnext, _) =
                    events_page(&import_id, None, None, Some(20), None).unwrap();
                let warm_page_ms = t_w.elapsed().as_millis();
                assert!(!wpage.is_empty(), "warm {w} page must not be empty");
                let t_s = Instant::now();
                assert_search_finds_synthetic_needle(&import_id, "r7-unique-needle");
                let search_ms = t_s.elapsed().as_millis();
                eprintln!(
                    "V8_100MIB_WARM w={w} cache_state=warm-reread first_page_ms={warm_page_ms} search_ms={search_ms}"
                );
            }
        }
        let _ = home;
    }
    assert!(
        !over_3x,
        "published revision exceeded 3× source on a 100 MiB cold publish (product blocker)"
    );
    assert!(
        !imports_over_3x,
        "complete imports storage exceeded 3× source on a 100 MiB cold publish (product blocker)"
    );
}

#[test]
#[ignore = "v10 1GiB genuine-cold performance gate"]
fn v10_1gib_two_cold_one_warm_no_import_cache() {
    let run_start = Instant::now();
    eprintln!("V10_PHASE t_ms=0 phase=process-ready");
    std::thread::sleep(std::time::Duration::from_secs(1));
    let source_root = std::env::var("GROK_V10_1GIB_SRC")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(r"H:\aicoding\grok-r7-1gib-fixture-current\src"));
    let mut source_files: Vec<PathBuf> = std::fs::read_dir(&source_root)
        .expect("read V10 1GiB synthetic source directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect();
    source_files.sort();
    let source_bytes: u64 = source_files
        .iter()
        .map(|path| path.metadata().expect("source metadata").len())
        .sum();
    assert_eq!(source_files.len(), BATCH_FILES as usize);
    assert!(source_bytes >= GIB, "source_bytes {source_bytes} < 1 GiB");

    for cold in 0..2u32 {
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-start",
            run_start.elapsed().as_millis()
        );
        let home = crate::session_migration::test_home::IsolatedAppHome::acquire(&format!(
            "v10-1gib-cold-{cold}"
        ));
        let files = home.path.join("files");
        std::fs::create_dir_all(&files).unwrap();
        for source in &source_files {
            std::fs::copy(source, files.join(source.file_name().unwrap()))
                .expect("copy synthetic source only; never copy imports cache");
        }
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-source-ready",
            run_start.elapsed().as_millis()
        );

        let t_scan = Instant::now();
        let scan = scan_start(vec![(
            Provider::GenericFile,
            files.to_string_lossy().into(),
        )])
        .unwrap();
        let scan_ms = t_scan.elapsed().as_millis();
        let mut candidates: Vec<_> = scan
            .candidates
            .iter()
            .filter(|candidate| candidate.locator.contains("chat-1gib-"))
            .collect();
        candidates.sort_by(|left, right| left.locator.cmp(&right.locator));
        assert_eq!(candidates.len(), BATCH_FILES as usize);
        let needle_candidate_id = candidates
            .iter()
            .find(|candidate| candidate.locator.ends_with("chat-1gib-00.json"))
            .expect("needle source candidate")
            .candidate_id
            .clone();
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-scan-done",
            run_start.elapsed().as_millis()
        );

        let t_preview = Instant::now();
        let mut preview_ids = Vec::with_capacity(candidates.len());
        for (index, candidate) in candidates.into_iter().enumerate() {
            let (preview_id, _) =
                preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
            preview_ids.push(preview_id);
            eprintln!(
                "V10_PHASE t_ms={} phase=1gib-cold-{cold}-preview-{index}-done",
                run_start.elapsed().as_millis()
            );
        }
        let preview_ms = t_preview.elapsed().as_millis();

        let t_plan = Instant::now();
        let mappings = propose_mappings(&preview_ids, None).unwrap();
        let planned = plan(preview_ids, mappings).unwrap();
        let plan_ms = t_plan.elapsed().as_millis();
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-plan-done",
            run_start.elapsed().as_millis()
        );

        let t_commit = Instant::now();
        let queued = commit(
            &planned.plan_id,
            &planned.plan_hash,
            &format!("v10-1gib-cold-{cold}"),
        )
        .unwrap();
        crate::session_migration::job_control::run_pending().unwrap();
        let done = job_get(&queued.job_id).unwrap();
        let commit_ms = t_commit.elapsed().as_millis();
        assert_eq!(done.items.len(), BATCH_FILES as usize);
        assert!(done.items.iter().all(|item| item.error.is_none()));
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-commit-done",
            run_start.elapsed().as_millis()
        );

        let mut published_bytes = 0u64;
        let mut first_import_id = None;
        let mut needle_import_id = None;
        for item in &done.items {
            let import_id = item.import_id.as_ref().expect("1GiB import id");
            let revision_id = item.revision_id.as_ref().expect("1GiB revision id");
            if first_import_id.is_none() {
                first_import_id = Some(import_id.clone());
            }
            if item.candidate_id == needle_candidate_id {
                needle_import_id = Some(import_id.clone());
            }
            published_bytes = published_bytes.saturating_add(dir_size(
                &crate::session_migration::catalog::revision_dir(import_id, revision_id).unwrap(),
            ));
        }
        let imports_bytes = dir_size(&imports_dir());
        assert!(published_bytes <= source_bytes.saturating_mul(3));
        assert!(imports_bytes <= source_bytes.saturating_mul(3));
        let import_id = first_import_id.expect("first 1GiB import");
        let needle_import_id = needle_import_id.expect("needle 1GiB import");

        let t_page = Instant::now();
        let (page, _, _) = events_page(&import_id, None, None, Some(20), None).unwrap();
        let first_page_ms = t_page.elapsed().as_millis();
        assert!(!page.is_empty());
        assert!(first_page_ms <= 2_000, "first page took {first_page_ms}ms");
        eprintln!(
            "V10_PHASE t_ms={} phase=1gib-cold-{cold}-first-page-done",
            run_start.elapsed().as_millis()
        );

        if cold == 0 {
            let warm_start = Instant::now();
            let (warm_page, _, _) = events_page(&import_id, None, None, Some(20), None).unwrap();
            let warm_page_ms = warm_start.elapsed().as_millis();
            assert!(!warm_page.is_empty());
            let search_start = Instant::now();
            let mut search_cursor = None;
            let hits = loop {
                let window = search_imported_with_leaf_window(
                    &needle_import_id,
                    None,
                    None,
                    "r7-1gib-needle",
                    search_cursor.as_deref(),
                    Some(5),
                    None,
                )
                .unwrap();
                let hits = window.hits;
                let next_cursor = window.next_cursor;
                if !hits.is_empty() || next_cursor.is_none() {
                    break hits;
                }
                search_cursor = next_cursor;
            };
            let search_ms = search_start.elapsed().as_millis();
            assert!(
                !hits.is_empty(),
                "warm search must find the synthetic needle"
            );
            eprintln!("V10_1GIB_WARM first_page_ms={warm_page_ms} search_ms={search_ms}");
        }

        eprintln!(
            "V10_1GIB_COLD i={cold} cache_state=cold-full-publish source_bytes={source_bytes} published_bytes={published_bytes} imports_bytes={imports_bytes} published_ratio={:.4} imports_ratio={:.4} scan_ms={scan_ms} preview_ms={preview_ms} plan_ms={plan_ms} commit_ms={commit_ms} first_page_ms={first_page_ms} ui_first_visible_ms=not-measured-in-backend-harness",
            published_bytes as f64 / source_bytes as f64,
            imports_bytes as f64 / source_bytes as f64,
        );
        drop(home);
    }

    let cancel_home =
        crate::session_migration::test_home::IsolatedAppHome::acquire("v10-1gib-cancel");
    let files = cancel_home.path.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(
        files.join("cancel.json"),
        r#"[{"role":"user","content":"cancel"}]"#,
    )
    .unwrap();
    let scan = scan_start(vec![(
        Provider::GenericFile,
        files.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = scan.candidates.first().expect("cancel candidate");
    let (preview_id, _) =
        preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = propose_mappings(std::slice::from_ref(&preview_id), None).unwrap();
    let planned = plan(vec![preview_id], mappings).unwrap();
    let queued = commit(&planned.plan_id, &planned.plan_hash, "v10-1gib-cancel").unwrap();
    let cancel_start = Instant::now();
    let cancelled = cancel_job(&queued.job_id).unwrap();
    let cancel_confirm_ms = cancel_start.elapsed().as_millis();
    assert!(
        cancel_confirm_ms <= 500,
        "cancel confirmation took {cancel_confirm_ms}ms"
    );
    assert!(matches!(
        cancelled.state,
        super::JobState::Cancelling | super::JobState::Cancelled
    ));
    eprintln!(
        "V10_1GIB_CANCEL confirm_ms={cancel_confirm_ms} state={:?}",
        cancelled.state
    );
}

// Home-fixture coverage only, not the ignored 100MiB/1GiB benchmarks.
fn acquire_and_release_harness_home() {
    let home = home_lock();
    let path = home.path.clone();
    assert!(path.is_dir());
    drop(home);
    assert!(!path.exists());
}
crate::session_migration::test_home::fixture_home_contract_tests!(acquire_and_release_harness_home);
