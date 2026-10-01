//! Native acceptance only. The release build contains no failure injection.
use std::cell::RefCell;
use std::collections::VecDeque;

thread_local! {
    static FAILURES: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
}

pub(super) fn reject(format: u32) -> bool {
    FAILURES.with_borrow_mut(|queue| {
        if queue.front() == Some(&format) {
            queue.pop_front();
            true
        } else {
            false
        }
    })
}

pub(super) struct Faults;

impl Faults {
    pub fn new(formats: &[u32]) -> Self {
        FAILURES.with_borrow_mut(|queue| {
            assert!(queue.is_empty(), "nested clipboard fixture faults");
            queue.extend(formats);
        });
        Self
    }

    pub fn verify_consumed(&self) -> Result<(), String> {
        if FAILURES.with_borrow(VecDeque::is_empty) {
            Ok(())
        } else {
            Err("clipboard publication fault was not exercised".into())
        }
    }
}

impl Drop for Faults {
    fn drop(&mut self) {
        FAILURES.with_borrow_mut(VecDeque::clear);
    }
}
