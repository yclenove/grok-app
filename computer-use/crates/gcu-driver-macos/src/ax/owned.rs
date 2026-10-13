//! One-shot Core Foundation ownership.
//!
//! A created or retained object is +1. Dropping [`Owned`] releases it once.
//! Taking the value out transfers that +1 and Drop does nothing. The unit test
//! uses a counter instead of a real `CFRelease`. Live AX code follows the same
//! rule and says so at each retain and release.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Releases one owned value. Called at most once.
pub trait Releaser<T> {
    /// Release `value`. The caller will not use it again.
    fn release(&self, value: T);
}

/// Owns `T` until drop or [`Owned::take`].
pub struct Owned<T, R: Releaser<T>> {
    value: Option<T>,
    releaser: R,
}

impl<T, R: Releaser<T>> Owned<T, R> {
    /// Take ownership of a +1 value.
    pub fn new(value: T, releaser: R) -> Self {
        Self {
            value: Some(value),
            releaser,
        }
    }

    /// Borrow the value. `None` after [`Owned::take`].
    pub fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Transfer the +1 out. Drop will not release it.
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }
}

impl<T, R: Releaser<T>> Drop for Owned<T, R> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            self.releaser.release(value);
        }
    }
}

/// Counts releases. Shared so a test can observe Drop.
#[derive(Clone, Debug, Default)]
pub struct CountRelease {
    hits: Arc<AtomicUsize>,
}

impl CountRelease {
    /// A counter that starts at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many times [`Releaser::release`] ran.
    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

impl Releaser<u64> for CountRelease {
    fn release(&self, _value: u64) {
        self.hits.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_releases_once_and_take_does_not() {
        let counter = CountRelease::new();
        let owned = Owned::new(1, counter.clone());
        drop(owned);
        assert_eq!(counter.hits(), 1);

        let mut taken = Owned::new(2, counter.clone());
        assert_eq!(taken.take(), Some(2));
        drop(taken);
        assert_eq!(counter.hits(), 1);
    }
}
