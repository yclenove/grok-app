//! Press-before-send accounting. Stop cannot race a down past its matching up.
use super::api::{Event, Target};
use crate::uia::patterns::ActionContext;
use gcu_core::driver::{Delivery, Result};
use std::sync::Mutex;

#[derive(Clone)]
struct Held {
    target: Target,
    event: Event,
    delivery: Delivery,
}
/// Held synthetic inputs. Only short, nonblocking PostMessage/SendInput calls
/// run under the lock; provider reads and activation never hold it.
#[derive(Default)]
pub struct InputLedger {
    held: Mutex<Vec<Held>>,
}
impl InputLedger {
    pub(crate) fn reserve(&self, target: &Target, events: &[Event], delivery: Delivery) -> usize {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        let previous = held.len();
        held.extend(events.iter().filter_map(|e| e.up()).map(|event| Held {
            target: target.clone(),
            event,
            delivery,
        }));
        previous
    }
    pub(crate) fn acknowledge(
        &self,
        target: &Target,
        events: &[Event],
        delivery: Delivery,
        previous: usize,
        delivered: bool,
    ) {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        if !delivered {
            held.truncate(previous);
            return;
        }
        for event in events.iter().filter(|e| e.is_up()) {
            if let Some(i) = held.iter().rposition(|h| {
                h.target.hwnd == target.hwnd
                    && h.delivery == delivery
                    && same_input(h.event, *event)
            }) {
                held.remove(i);
            }
        }
    }
    /// Whether every recorded down has an acknowledged up.
    pub fn is_empty(&self) -> bool {
        self.held
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_empty()
    }
    /// Atomically record down, check the closed/cancelled dispatch token, send,
    /// then forget acknowledged up. A failed up stays pending for cleanup.
    /// `send` must report whether a failed native call could have delivered.
    pub fn dispatch(
        &self,
        target: &Target,
        event: Event,
        delivery: Delivery,
        context: &ActionContext,
        send: impl FnOnce(bool) -> Result<()>,
    ) -> Result<()> {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        let previous = context.begin()?;
        let pending = held.len();
        if let Some(up) = event.up() {
            held.push(Held {
                target: target.clone(),
                event: up,
                delivery,
            });
        }
        let result = send(
            event.up().is_some()
                && held.iter().any(|h| {
                    h.target.hwnd == target.hwnd
                        && event.up().is_some_and(|up| same_input(h.event, up))
                }),
        );
        if result.as_ref().is_err_and(|e| !e.maybe_executed()) {
            context.restore(previous);
            held.truncate(pending);
        }
        if result.is_ok() && event.is_up() {
            if let Some(i) = held.iter().rposition(|h| {
                h.target.hwnd == target.hwnd && h.delivery == delivery && same_input(h.event, event)
            }) {
                held.remove(i);
            }
        }
        result.map_err(|e| context.error(e))
    }
    /// Account an entire SendInput packet before its one nonblocking insertion.
    /// A partial insertion leaves every possible down pending for cleanup.
    pub fn dispatch_packet(
        &self,
        target: &Target,
        events: &[Event],
        delivery: Delivery,
        context: &ActionContext,
        send: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        let previous = context.begin()?;
        let pending = held.len();
        for event in events {
            if let Some(up) = event.up() {
                held.push(Held {
                    target: target.clone(),
                    event: up,
                    delivery,
                });
            }
        }
        let result = send();
        if result.as_ref().is_err_and(|e| !e.maybe_executed()) {
            context.restore(previous);
            held.truncate(pending);
        }
        if result.is_ok() {
            for event in events.iter().filter(|e| e.is_up()) {
                if let Some(i) = held.iter().rposition(|h| {
                    h.target.hwnd == target.hwnd
                        && h.delivery == delivery
                        && same_input(h.event, *event)
                }) {
                    held.remove(i);
                }
            }
        }
        result.map_err(|e| context.error(e))
    }
    /// Send all pending up events in reverse order without cancellation. Failed
    /// releases remain pending for the next explicit cleanup, never replay downs.
    pub fn release(&self, mut send: impl FnMut(&Target, Event, Delivery) -> Result<()>) {
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        let mut i = held.len();
        while i > 0 {
            i -= 1;
            let h = &held[i];
            match send(&h.target, h.event, h.delivery) {
                Ok(()) => {
                    held.remove(i);
                }
                Err(error) => {
                    tracing::warn!(code = ?error.error_code(), "Windows input release failed")
                }
            }
        }
    }
}
fn same_input(a: Event, b: Event) -> bool {
    match (a, b) {
        (Event::Button(a, false, ..), Event::Button(b, false, ..)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gcu_core::driver::{Cancel, DriverError};
    fn target() -> Target {
        Target {
            window: crate::testing::window(8, 7),
            hwnd: 9,
            class: "Edit".into(),
            message_capable: true,
        }
    }
    #[test]
    fn partial_unicode_packet_retains_every_possible_up_in_reverse_order() {
        let ledger = InputLedger::default();
        let context = ActionContext::new(&Cancel::default());
        let packet = [
            Event::Unicode(0xd83d, true),
            Event::Unicode(0xd83d, false),
            Event::Unicode(0xde00, true),
            Event::Unicode(0xde00, false),
        ];
        assert_eq!(
            ledger.dispatch_packet(&target(), &packet, Delivery::Foreground, &context, || Err(
                DriverError::Timeout {
                    maybe_executed: true
                }
            )),
            Err(DriverError::Timeout {
                maybe_executed: true
            })
        );
        let mut ups = Vec::new();
        ledger.release(|_, event, _| {
            ups.push(event);
            Ok(())
        });
        assert_eq!(
            ups,
            vec![Event::Unicode(0xde00, false), Event::Unicode(0xd83d, false)]
        );
        assert!(ledger.is_empty());
    }
    #[test]
    fn zero_packet_insertion_proves_no_effect_and_needs_no_release() {
        let ledger = InputLedger::default();
        let context = ActionContext::new(&Cancel::default());
        let packet = [
            Event::Key(17, true, false, false),
            Event::Key(17, false, false, false),
        ];
        assert_eq!(
            ledger.dispatch_packet(&target(), &packet, Delivery::Foreground, &context, || Err(
                DriverError::Timeout {
                    maybe_executed: false
                }
            )),
            Err(DriverError::Timeout {
                maybe_executed: false
            })
        );
        assert!(ledger.is_empty());
        assert!(!context.maybe_executed());
    }
    #[test]
    fn expired_dispatch_deadline_prevents_even_immediate_packet_insertion() {
        let ledger = InputLedger::default();
        let context = ActionContext::until(&Cancel::default(), std::time::Instant::now());
        let packet = [Event::Key(17, true, false, false)];
        let mut called = false;
        assert_eq!(
            ledger.dispatch_packet(&target(), &packet, Delivery::Foreground, &context, || {
                called = true;
                Ok(())
            }),
            Err(DriverError::Timeout {
                maybe_executed: false
            })
        );
        assert!(!called);
        assert!(ledger.is_empty());
    }
}
