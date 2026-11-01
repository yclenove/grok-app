//! S11b caller-driven machine. Runs inside the existing re-exec watchdog.

mod edges;
mod faults;
mod harness;
mod recovery;

pub(super) fn run(failures: &mut Vec<String>) {
    for (name, test) in edges::TESTS.iter().chain(faults::TESTS.iter()) {
        if let Err(err) = test() {
            failures.push(format!("{name}: {err}"));
        }
    }
}
