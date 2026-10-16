//! Clock-bounded operations shared by text and clipboard, including Stop.

use super::{clock::Clock, hotkey::Guards};
use gcu_core::driver::{Cancel, DriverError, Result};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub(crate) struct Operation<'a> {
    pub cancel: &'a Cancel,
    pub sent: &'a AtomicBool,
    pub clock: &'a dyn Clock,
    pub deadline: Duration,
    pub guards: Option<Arc<Guards>>,
    pub foreground: bool,
}

impl<'a> Operation<'a> {
    pub fn new(
        cancel: &'a Cancel,
        sent: &'a AtomicBool,
        clock: &'a dyn Clock,
        budget: Duration,
        guards: Option<Arc<Guards>>,
    ) -> Self {
        Self {
            cancel,
            sent,
            clock,
            deadline: clock.now() + budget,
            guards,
            foreground: false,
        }
    }
    pub fn foreground(&self) -> Self {
        Self {
            foreground: true,
            guards: self.guards.clone(),
            ..*self
        }
    }
    pub fn check(&self) -> Result<()> {
        let maybe_executed = self.sent.load(Ordering::Acquire);
        if self.cancel.is_cancelled() {
            return Err(DriverError::Cancelled { maybe_executed });
        }
        let checked = match &self.guards {
            Some(guards) => guards.check(self.foreground),
            None if self.foreground => Err(DriverError::CapabilityUnavailable),
            None => Ok(()),
        };
        checked.map_err(|error| {
            if maybe_executed {
                super::foreground::uncertain(error)
            } else {
                error
            }
        })?;
        if self.clock.now() >= self.deadline {
            return Err(DriverError::Timeout { maybe_executed });
        }
        Ok(())
    }
    pub async fn guard<T>(&self, future: impl Future<Output = Result<T>>) -> Result<T> {
        self.check()?;
        let result = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(DriverError::Cancelled { maybe_executed: self.sent.load(Ordering::Acquire) }),
            _ = async { if let Some(guards) = &self.guards { guards.stopped().await } else { std::future::pending().await } } => Err(DriverError::Cancelled { maybe_executed: self.sent.load(Ordering::Acquire) }),
            _ = self.clock.wait_until(self.deadline) => Err(DriverError::Timeout { maybe_executed: self.sent.load(Ordering::Acquire) }),
            result = future => result,
        };
        if result.is_ok() {
            self.check()?;
        }
        result
    }
}
