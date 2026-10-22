//! Time for durable plan intent and accepted job creation, not authorization.
//! Production always uses UTC; only tests can freeze/advance this thread's time.
use chrono::{DateTime, Utc};

pub(super) fn now() -> DateTime<Utc> {
    #[cfg(test)]
    if let Some(value) = FIXED.with(std::cell::Cell::get) {
        return value;
    }
    Utc::now()
}

#[cfg(test)]
thread_local! {
    static FIXED: std::cell::Cell<Option<DateTime<Utc>>> = const { std::cell::Cell::new(None) };
}

/// Not Send: a guard must restore the same thread, including nested overrides.
#[cfg(test)]
pub(super) struct Fixed {
    previous: Option<DateTime<Utc>>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(test)]
impl Fixed {
    pub(super) fn at(value: DateTime<Utc>) -> Self {
        Self {
            previous: FIXED.with(|slot| slot.replace(Some(value))),
            _thread: std::marker::PhantomData,
        }
    }
}

#[cfg(test)]
impl Drop for Fixed {
    fn drop(&mut self) {
        FIXED.with(|slot| slot.set(self.previous));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_is_nested_thread_local_and_restored() {
        assert!(FIXED.with(std::cell::Cell::get).is_none());
        let start = Utc::now();
        {
            let _outer = Fixed::at(start);
            assert_eq!(now(), start);
            {
                let _inner = Fixed::at(start + chrono::Duration::hours(1));
                assert_eq!(now(), start + chrono::Duration::hours(1));
            }
            assert_eq!(now(), start);
            std::thread::spawn(|| assert!(FIXED.with(std::cell::Cell::get).is_none()))
                .join()
                .unwrap();
        }
        assert!(FIXED.with(std::cell::Cell::get).is_none());
    }
}
