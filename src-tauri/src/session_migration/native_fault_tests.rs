use super::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

const RUN: &str = "39476a7f-3d22-4583-a2fe-36bd1e558ca6";

fn home() -> PathBuf {
    std::env::temp_dir().join(format!("grok-import-audit-{RUN}"))
}

fn configured(point: &str) -> Config {
    Config::parse(
        Some("mock"),
        Some("1"),
        Some(&home()),
        Some(point),
        Some(RUN),
    )
    .unwrap()
}

#[test]
fn all_six_boundary_names_round_trip_without_aliases() {
    for name in [
        "queued",
        "running",
        "after_rename",
        "after_published_receipt",
        "after_catalog_commit",
        "before_completion_event",
    ] {
        let point = Point::parse(name).unwrap();
        assert_eq!(
            serde_json::to_string(&point).unwrap(),
            format!("\"{name}\"")
        );
    }
    for bad in [
        "",
        "queued ",
        "RUNNING",
        "before_rename",
        "after_receipt_published",
        "*",
        "../queued",
    ] {
        assert!(Point::parse(bad).is_none());
    }
}

#[test]
fn each_runtime_gate_is_required() {
    let home = home();
    assert!(Config::parse(
        Some("mock"),
        Some("1"),
        Some(&home),
        Some("queued"),
        Some(RUN)
    )
    .is_some());
    for acp in [None, Some("real"), Some("MOCK"), Some("")] {
        assert!(Config::parse(acp, Some("1"), Some(&home), Some("queued"), Some(RUN)).is_none());
    }
    for roots in [None, Some("0"), Some("true"), Some("")] {
        assert!(
            Config::parse(Some("mock"), roots, Some(&home), Some("queued"), Some(RUN)).is_none()
        );
    }
    assert!(Config::parse(Some("mock"), Some("1"), None, Some("queued"), Some(RUN)).is_none());
    for point in [None, Some("unknown"), Some("")] {
        assert!(Config::parse(Some("mock"), Some("1"), Some(&home), point, Some(RUN)).is_none());
    }
    for run in [
        None,
        Some("invalid"),
        Some(""),
        Some("00000000-0000-0000-0000-000000000000"),
    ] {
        assert!(Config::parse(Some("mock"), Some("1"), Some(&home), Some("queued"), run).is_none());
    }
}

#[test]
fn ordinary_relative_mismatched_and_traversing_homes_are_rejected() {
    for home in [
        PathBuf::from(format!("grok-import-audit-{RUN}")),
        std::env::temp_dir().join("app-home"),
        std::env::temp_dir().join("grok-import-audit-79476a7f-3d22-4583-a2fe-36bd1e558ca6"),
        std::env::temp_dir()
            .join("nested/..")
            .join(format!("grok-import-audit-{RUN}")),
    ] {
        assert!(Config::parse(
            Some("mock"),
            Some("1"),
            Some(&home),
            Some("queued"),
            Some(RUN)
        )
        .is_none());
    }
}

#[test]
fn unrelated_boundary_does_not_consume_one_shot() {
    let gate = Gate::new(configured("running"));
    gate.reach(
        Point::Queued,
        "job",
        |_| panic!("wrong point emitted"),
        |_| panic!("wrong point waited"),
    );
    assert!(!gate.fired.load(Ordering::Acquire));
    let emitted = RefCell::new(Vec::new());
    gate.reach(
        Point::Running,
        "job",
        |m| {
            emitted.borrow_mut().push(m.status);
            Ok(())
        },
        |duration| assert_eq!(duration, Duration::from_secs(120)),
    );
    assert_eq!(*emitted.borrow(), vec!["paused", "expired"]);
    gate.reach(
        Point::Running,
        "job-2",
        |_| panic!("second attempt emitted"),
        |_| panic!("second wait"),
    );
}

#[test]
fn marker_failure_never_waits_or_rearms() {
    let gate = Gate::new(configured("queued"));
    gate.reach(
        Point::Queued,
        "job",
        |_| Err(io::Error::other("synthetic marker failure")),
        |_| panic!("unobservable pause"),
    );
    gate.reach(
        Point::Queued,
        "job",
        |_| panic!("failure rearmed"),
        |_| panic!("failure waited"),
    );
}

#[test]
fn expiry_is_reported_after_exact_bounded_wait() {
    let gate = Gate::new(configured("after_catalog_commit"));
    let sequence = RefCell::new(Vec::new());
    gate.reach(
        Point::AfterCatalogCommit,
        "job",
        |m| {
            assert_eq!(m.pause_seconds, 120);
            sequence.borrow_mut().push(m.status);
            Ok(())
        },
        |duration| {
            assert_eq!(duration, PAUSE);
            sequence.borrow_mut().push("wait");
        },
    );
    assert_eq!(*sequence.borrow(), vec!["paused", "wait", "expired"]);
}

#[test]
fn expiry_marker_failure_does_not_repeat_pause() {
    let gate = Gate::new(configured("after_rename"));
    let waits = AtomicUsize::new(0);
    gate.reach(
        Point::AfterRename,
        "job",
        |m| {
            if m.status == "expired" {
                Err(io::Error::other("synthetic expiry failure"))
            } else {
                Ok(())
            }
        },
        |_| {
            waits.fetch_add(1, Ordering::Relaxed);
        },
    );
    gate.reach(
        Point::AfterRename,
        "job",
        |_| panic!("rearmed"),
        |_| panic!("waited again"),
    );
    assert_eq!(waits.load(Ordering::Relaxed), 1);
}

#[test]
fn simultaneous_callers_pause_only_once() {
    let gate = Arc::new(Gate::new(configured("running")));
    let waits = Arc::new(AtomicUsize::new(0));
    let markers = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (gate, waits, markers, start) =
                (gate.clone(), waits.clone(), markers.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                gate.reach(
                    Point::Running,
                    "job",
                    |_| {
                        markers.fetch_add(1, Ordering::Relaxed);
                        Ok(())
                    },
                    |_| {
                        waits.fetch_add(1, Ordering::Relaxed);
                    },
                );
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert_eq!(waits.load(Ordering::Relaxed), 1);
    assert_eq!(markers.load(Ordering::Relaxed), 2);
}

#[test]
fn marker_contains_hash_not_raw_identifier_or_private_path() {
    let gate = Gate::new(configured("before_completion_event"));
    let private = "synthetic-private-job-path-and-body";
    gate.reach(
        Point::BeforeCompletionEvent,
        private,
        |m| {
            let value = serde_json::to_value(m).unwrap();
            assert_eq!(value.as_object().unwrap().len(), 7);
            assert_eq!(
                value["jobSha256"],
                crate::session_migration::hash::sha256_bytes(private.as_bytes())
            );
            assert_eq!(value["run"], RUN);
            assert_eq!(value["processId"], std::process::id());
            assert!(!value.to_string().contains(private));
            assert!(!value.to_string().contains("grok-import-audit-"));
            Ok(())
        },
        |_| {},
    );
}
