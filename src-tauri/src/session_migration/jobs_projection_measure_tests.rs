//! Measures the durable job JSON header. Does not change save or recovery.

use std::time::Instant;

use super::jobs::{JobItem, JobRecord, JobState};

fn sample(count: usize) -> JobRecord {
    JobRecord {
        job_id: "job".into(),
        request_id: "request".into(),
        plan_hash: "plan".into(),
        plan_id: "plan-id".into(),
        state: JobState::Queued,
        state_version: 1,
        items: (0..count)
            .map(|index| JobItem {
                candidate_id: format!("candidate-{index}"),
                title: None,
                phase: "queued".into(),
                import_id: None,
                revision_id: None,
                outcome: None,
                error: None,
                mapping_project_id: None,
            })
            .collect(),
        created_at: "2026-09-22T00:00:00Z".into(),
        item_count: u32::try_from(count).unwrap_or(u32::MAX),
        next_item_cursor: None,
        progress: None,
    }
}

fn median_serialize(job: &JobRecord) -> (usize, u128) {
    let mut times = [0u128; 3];
    let mut bytes = 0usize;
    for slot in &mut times {
        let started = Instant::now();
        let json = serde_json::to_string(job).expect("job header serializes");
        *slot = started.elapsed().as_micros();
        bytes = json.len();
    }
    times.sort_unstable();
    (bytes, times[1])
}

#[test]
fn whole_job_json_header_stays_linear_for_the_measured_sizes() {
    let (bytes_50, us_50) = median_serialize(&sample(50));
    let (bytes_1000, us_1000) = median_serialize(&sample(1_000));
    let (bytes_10000, us_10000) = median_serialize(&sample(10_000));
    eprintln!(
        "JOB_HEADER n=50 bytes={bytes_50} median_us={us_50}; n=1000 bytes={bytes_1000} median_us={us_1000}; n=10000 bytes={bytes_10000} median_us={us_10000}"
    );
    assert!(bytes_1000 > bytes_50);
    assert!(bytes_10000 > bytes_1000);
    assert!(
        bytes_10000 < 8 * 1024 * 1024,
        "10k header is {bytes_10000} bytes"
    );
    assert!(
        us_10000 < 2_000_000,
        "10k header median serialize is {us_10000} us"
    );
}
