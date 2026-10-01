//! Retain the exact native reply subscription after the caller's soft deadline.
//! No input worker/retry/reset: only an authenticated provider reply with the
//! original serial AND destination proves this RPC has returned. Bus errors,
//! socket closure, elapsed time and a later readback do not prove completion.
use super::{err, Connection, Duration, Instant};
use futures_lite::{
    future::{block_on, poll_once},
    StreamExt,
};
use grok_computer_use_core::native_action::{NativeActionGuard, NativeActionRecovery};
use std::num::NonZeroU32;
use zbus::{message::Type, MatchRule, Message, MessageStream};

#[derive(Default)]
pub(crate) struct NativeCalls {
    pub uncertain: bool,
    pending: Option<Pending>,
    recovery: Option<NativeActionRecovery>,
}

struct Pending {
    serial: NonZeroU32,
    sender: String,
    destination: String,
    streams: [MessageStream; 2],
    ended: [bool; 2],
    failed: bool,
}

impl Pending {
    fn prepare(connection: &Connection, call: &Message) -> Result<Self, String> {
        let header = call.header();
        let sender = header
            .destination()
            .ok_or("missing native destination")?
            .to_string();
        if !sender.starts_with(':') {
            return Err("native input needs a unique provider".into());
        }
        let destination = connection
            .unique_name()
            .ok_or("missing native bus identity")?
            .to_string();
        let stream = |kind| {
            let rule = MatchRule::builder()
                .msg_type(kind)
                .sender(sender.as_str())
                .map_err(err)?
                .destination(destination.as_str())
                .map_err(err)?
                .build();
            // Non-signal rules are local subscriptions, not bus AddMatch calls.
            block_on(MessageStream::for_match_rule(
                rule,
                connection.inner(),
                Some(8),
            ))
            .map_err(err)
        };
        Ok(Self {
            serial: call.primary_header().serial_num(),
            streams: [stream(Type::MethodReturn)?, stream(Type::Error)?],
            sender,
            destination,
            ended: [false; 2],
            failed: false,
        })
    }

    fn poll(&mut self) -> Option<Result<bool, String>> {
        if self.failed {
            return None;
        }
        // Bound each poll, including a hostile provider's unrelated messages.
        for (index, stream) in self.streams.iter_mut().enumerate() {
            if self.ended[index] {
                continue;
            }
            for _ in 0..8 {
                match block_on(poll_once(stream.next())) {
                    None => break,
                    Some(Some(Ok(message))) => {
                        if let Some(result) =
                            classify(&message, self.serial, &self.sender, &self.destination)
                        {
                            return Some(result);
                        }
                    }
                    Some(None | Some(Err(_))) => {
                        self.ended[index] = true;
                        break;
                    }
                }
            }
        }
        // A valid provider Error may already be queued when the return stream
        // observes socket closure. Drain both bounded queues before concluding
        // there can be no completion evidence; never discard the other queue.
        self.failed = self.ended.iter().all(|ended| *ended);
        None
    }
}

fn classify(
    message: &Message,
    serial: NonZeroU32,
    sender: &str,
    destination: &str,
) -> Option<Result<bool, String>> {
    let header = message.header();
    if header.reply_serial() != Some(serial)
        || header.sender().map(|s| s.as_str()) != Some(sender)
        || header.destination().map(|s| s.as_str()) != Some(destination)
    {
        return None;
    }
    match header.message_type() {
        Type::MethodReturn => message.body().deserialize::<bool>().ok().map(Ok),
        Type::Error if header.error_name().is_some() => {
            // A provider error is completion, not success. Bus-generated errors
            // never pass the unique provider identity test above.
            message
                .body()
                .deserialize::<String>()
                .ok()
                .map(|_| Err("AT-SPI provider returned an error; no replay".into()))
        }
        _ => None,
    }
}

impl NativeCalls {
    pub fn call(
        &mut self,
        connection: &Connection,
        message: Message,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        if self.uncertain || self.pending.is_some() {
            return Err("native input still pending".into());
        }
        let pending = Pending::prepare(connection, &message)?;
        // Subscription is installed before send; no native input occurs before
        // the final local authority/cancellation/focus fence.
        validate()?;
        self.pending = Some(pending);
        self.uncertain = true;
        connection.send(&message).map_err(err)?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            let pending = self.pending.as_mut().expect("pending native call");
            if let Some(result) = pending.poll() {
                self.pending = None;
                self.uncertain = false;
                return result;
            }
            if pending.failed || Instant::now() >= deadline {
                return Err("AT-SPI completion unknown; exact reply retained, no replay".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    pub fn defer(&mut self, owner: &mut NativeActionGuard<'_>) {
        if self.uncertain && self.pending.is_some() && self.recovery.is_none() {
            self.recovery = Some(owner.defer_until_native_completion());
        }
    }

    pub fn take_completion(&mut self) -> Option<NativeActionRecovery> {
        self.recovery.as_ref()?;
        let _completed_result = self.pending.as_mut()?.poll()?;
        // Result may be false/error: only native occupancy is recovered. Never
        // upgrade the action outcome, continue its remaining steps, or replay.
        self.pending = None;
        self.uncertain = false;
        self.recovery.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call() -> Message {
        Message::method_call("/field", "DeleteText")
            .unwrap()
            .sender(":1.10")
            .unwrap()
            .destination(":1.20")
            .unwrap()
            .build(&(2i32, 6i32))
            .unwrap()
    }
    #[test]
    fn only_exact_provider_serial_destination_and_boolean_reply_is_completion() {
        let call = call();
        let serial = call.primary_header().serial_num();
        for value in [true, false] {
            let reply = Message::method_return(&call.header())
                .unwrap()
                .sender(":1.20")
                .unwrap()
                .build(&value)
                .unwrap();
            assert_eq!(classify(&reply, serial, ":1.20", ":1.10"), Some(Ok(value)));
            assert!(classify(&reply, serial, ":1.21", ":1.10").is_none());
            assert!(classify(&reply, serial, ":1.20", ":1.11").is_none());
            assert!(classify(
                &reply,
                NonZeroU32::new(serial.get() + 1).unwrap(),
                ":1.20",
                ":1.10"
            )
            .is_none());
        }
        let malformed = Message::method_return(&call.header())
            .unwrap()
            .sender(":1.20")
            .unwrap()
            .build(&"not a bool")
            .unwrap();
        assert!(classify(&malformed, serial, ":1.20", ":1.10").is_none());
    }
    #[test]
    fn provider_error_is_completion_but_bus_no_reply_is_not() {
        let call = call();
        let serial = call.primary_header().serial_num();
        for sender in [":1.20", "org.freedesktop.DBus"] {
            let reply = Message::error(&call.header(), "org.freedesktop.DBus.Error.NoReply")
                .unwrap()
                .sender(sender)
                .unwrap()
                .build(&"error")
                .unwrap();
            let result = classify(&reply, serial, ":1.20", ":1.10");
            if sender == ":1.20" {
                assert!(result.unwrap().is_err());
            } else {
                assert!(result.is_none());
            }
        }
    }
}
