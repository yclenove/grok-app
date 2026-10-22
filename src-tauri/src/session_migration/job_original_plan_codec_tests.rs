//! Positive inputs come from actual registered scan/preview/plan/acceptance.
//! Mutations below are negative codec or live-context controls, never grants.
use super::*;
use crate::session_migration::{
    catalog, engine, errors::ErrorCode, jobs, scan, test_home::IsolatedAppHome, types::Provider,
};
use std::marker::PhantomData;
use std::rc::Rc;

struct Isolation {
    previous: VecDeque<Key>,
    _thread: PhantomData<Rc<()>>,
}

impl Isolation {
    fn acquire() -> Self {
        Self {
            previous: CACHE.with(|cache| std::mem::take(&mut *cache.borrow_mut())),
            _thread: PhantomData,
        }
    }
}

impl Drop for Isolation {
    fn drop(&mut self) {
        CACHE.with(|cache| cache.replace(std::mem::take(&mut self.previous)));
    }
}

struct Runtime;
impl Drop for Runtime {
    fn drop(&mut self) {
        engine::reset_runtime();
    }
}

fn accepted(home: &IsolatedAppHome, index: usize) -> (jobs::JobRecord, Record, String) {
    engine::reset_runtime();
    let source = home.path.join(format!("source-{index}"));
    std::fs::create_dir(&source).unwrap();
    std::fs::write(
        source.join("chat.json"),
        br#"[{"role":"user","content":"synthetic codec input"}]"#,
    )
    .unwrap();
    let scan = scan::scan_start(vec![(
        Provider::GenericFile,
        source.to_string_lossy().into(),
    )])
    .unwrap();
    let candidate = &scan.candidates[0];
    let (preview, _) =
        engine::preview(&candidate.candidate_id, Some(&candidate.source_revision)).unwrap();
    let mappings = engine::propose_mappings(std::slice::from_ref(&preview), None).unwrap();
    let plan = engine::plan(vec![preview], mappings).unwrap();
    let job = engine::commit(
        &plan.plan_id,
        &plan.plan_hash,
        &format!("codec-request-{index}"),
    )
    .unwrap();
    let conn = catalog::open_catalog().unwrap();
    let meta = |prefix: &str| {
        conn.query_row(
            "SELECT value FROM meta WHERE key=?1",
            [format!("{prefix}:{}", job.job_id)],
            |row| row.get::<_, String>(0),
        )
        .unwrap()
    };
    let scope: Record = serde_json::from_str(&meta("job_entry_scope")).unwrap();
    scope.validate(&job.job_id).unwrap();
    let original = meta("job_entry_plan_snapshot");
    assert_eq!(scope.plan_hash, plan.plan_hash);
    assert_eq!(
        scope.plan_json_digest,
        hash::sha256_bytes(original.as_bytes())
    );
    (job, scope, original)
}

#[test]
fn warm_reads_on_new_connections_only_memoize_the_exact_original_codec() {
    let home = IsolatedAppHome::acquire("ordinary-codec-connections");
    let _runtime = Runtime;
    let (job, _, _) = accepted(&home, 0);
    let _isolation = Isolation::acquire();
    engine::take_durable_plan_loads();
    assert_eq!(engine::job_get(&job.job_id).unwrap().job_id, job.job_id);
    assert_eq!(engine::take_durable_plan_loads(), 1);
    for _ in 0..3 {
        let conn = catalog::open_catalog().unwrap();
        assert_eq!(
            jobs::get_job(&conn, &job.job_id).unwrap().unwrap().job_id,
            job.job_id
        );
    }
    assert_eq!(engine::take_durable_plan_loads(), 0);
    assert_eq!(CACHE.with(|cache| cache.borrow().len()), 1);
}

#[test]
fn identical_original_bytes_cannot_validate_another_expected_plan_identity() {
    let home = IsolatedAppHome::acquire("ordinary-codec-identity");
    let _runtime = Runtime;
    let (_, mut scope, original) = accepted(&home, 0);
    let _isolation = Isolation::acquire();
    require_ordinary(&scope, &original).unwrap();
    let original_hash = scope.plan_hash.clone();
    scope.plan_hash = "0".repeat(64);
    let error = require_ordinary(&scope, &original).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert!(error.is_job_local());
    scope.plan_hash = original_hash;
    scope.plan_id = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        require_ordinary(&scope, &original).unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(CACHE.with(|cache| cache.borrow().len()), 1);
}

#[test]
fn changed_bytes_with_coordinated_digest_still_require_the_full_canonical_check() {
    let home = IsolatedAppHome::acquire("ordinary-codec-changed-bytes");
    let _runtime = Runtime;
    let (_, mut scope, original) = accepted(&home, 0);
    let _isolation = Isolation::acquire();
    require_ordinary(&scope, &original).unwrap();
    let mut changed: serde_json::Value = serde_json::from_str(&original).unwrap();
    changed["options"] = serde_json::json!({"includeSecrets": true});
    let changed = changed.to_string();
    scope.plan_json_digest = hash::sha256_bytes(changed.as_bytes());
    engine::take_durable_plan_loads();
    let error = require_ordinary(&scope, &changed).unwrap_err();
    assert_eq!(error.code, ErrorCode::PlanStale);
    assert!(error.is_job_local());
    assert_eq!(engine::take_durable_plan_loads(), 1);
    assert_eq!(CACHE.with(|cache| cache.borrow().len()), 1);
    scope.plan_json_digest = hash::sha256_bytes(b"{");
    assert_eq!(
        require_ordinary(&scope, "{").unwrap_err().code,
        ErrorCode::PlanStale
    );
    assert_eq!(CACHE.with(|cache| cache.borrow().len()), 1);
}

#[test]
fn warm_codec_does_not_authorize_changed_live_job_context_or_write_on_retry() {
    let home = IsolatedAppHome::acquire("ordinary-codec-live-context");
    let _runtime = Runtime;
    let (job, _, _) = accepted(&home, 0);
    let _isolation = Isolation::acquire();
    engine::job_get(&job.job_id).unwrap();
    let conn = catalog::open_catalog().unwrap();
    // Change BOTH SQL and JSON consistently: header column equality alone is
    // insufficient. Current accepted-scope/job context must still refuse it.
    conn.execute(
        "UPDATE jobs SET created_at='2000-01-01T00:00:00Z',
         json=json_set(json,'$.createdAt','2000-01-01T00:00:00Z') WHERE job_id=?1",
        [&job.job_id],
    )
    .unwrap();
    let before: (u32, String) = conn
        .query_row(
            "SELECT state_version,json FROM jobs WHERE job_id=?1",
            [&job.job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    for operation in ["get", "page", "retry"] {
        jobs::take_job_item_deserializes();
        let result = match operation {
            "get" => engine::job_get(&job.job_id),
            "page" => engine::job_items_page(&job.job_id, None, Some(1)),
            "retry" => engine::retry_failed(&job.job_id),
            _ => unreachable!(),
        };
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::PlanStale, "{operation}");
        assert!(error.is_job_local());
        assert_eq!(jobs::take_job_item_deserializes(), 0, "{operation}");
        let after: (u32, String) = conn
            .query_row(
                "SELECT state_version,json FROM jobs WHERE job_id=?1",
                [&job.job_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(after, before, "no read/retry repair: {operation}");
    }
}

#[test]
fn cache_has_fixed_small_keys_and_eviction_revalidates_genuine_original_bytes() {
    let home = IsolatedAppHome::acquire("ordinary-codec-eviction");
    let _runtime = Runtime;
    let _isolation = Isolation::acquire();
    let (_, first, original) = accepted(&home, 0);
    require_ordinary(&first, &original).unwrap();
    for index in 1..=MAX_ENTRIES {
        let (_, scope, original) = accepted(&home, index);
        require_ordinary(&scope, &original).unwrap();
    }
    CACHE.with(|cache| {
        let cache = cache.borrow();
        assert_eq!(cache.len(), MAX_ENTRIES);
        assert!(cache.iter().all(|key| key.plan_id.len() == 36
            && key.plan_hash.len() == 64
            && key.json_digest.len() == 64));
        assert!(cache.iter().all(|key| key.plan_id != first.plan_id));
    });
    engine::take_durable_plan_loads();
    require_ordinary(&first, &original).unwrap();
    assert_eq!(engine::take_durable_plan_loads(), 1);
    assert_eq!(CACHE.with(|cache| cache.borrow().len()), MAX_ENTRIES);
}

#[test]
fn another_thread_requires_its_own_original_codec_check() {
    let home = IsolatedAppHome::acquire("ordinary-codec-thread");
    let _runtime = Runtime;
    let (_, scope, original) = accepted(&home, 0);
    let _isolation = Isolation::acquire();
    require_ordinary(&scope, &original).unwrap();
    std::thread::scope(|threads| {
        threads.spawn(|| {
            assert!(CACHE.with(|cache| cache.borrow().is_empty()));
            engine::take_durable_plan_loads();
            require_ordinary(&scope, &original).unwrap();
            assert_eq!(engine::take_durable_plan_loads(), 1);
        });
    });
    engine::take_durable_plan_loads();
    require_ordinary(&scope, &original).unwrap();
    assert_eq!(engine::take_durable_plan_loads(), 0);
}
