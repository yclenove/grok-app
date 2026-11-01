//! S11b caller-driven machine. Runs inside the existing re-exec watchdog.

mod edges;
mod faults;
mod harness;
mod recovery;
mod review;

pub(super) fn run(failures: &mut Vec<String>) {
    for (name, test) in edges::TESTS
        .iter()
        .chain(faults::TESTS.iter())
        .chain(review::TESTS.iter())
    {
        if let Err(err) = test() {
            failures.push(format!("{name}: {err}"));
        }
    }
}

/// Registered checks. The edge matrix is one check, not one per scenario.
pub(super) fn report(legacy: usize) {
    let edges = edges::TESTS.len();
    let faults = faults::TESTS.len();
    let review = review::TESTS.len();
    eprintln!(
        "tart_offline ok: {legacy} legacy, {} s11b (edges {edges}, faults {faults}, review {review})",
        edges + faults + review
    );
}
